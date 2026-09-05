//! mDNS browser for Sender-side receiver discovery — REQ-PICOO-DISCOVERY-001.

use std::collections::HashMap;
use std::time::Duration;

use mdns_sd::{IfKind, Receiver, ServiceDaemon, ServiceEvent};
use thiserror::Error;

use crate::types::{ReceiverAdvertisement, SERVICE_TYPE};

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DiscoveredReceiver {
    pub fullname: String,
    pub advertisement: ReceiverAdvertisement,
    pub host: String,
}

#[derive(Debug, Error)]
pub enum BrowseError {
    #[error("mdns: {0}")]
    Mdns(String),
    #[error("invalid advertisement: {0}")]
    InvalidAd(String),
}

pub struct MdnsBrowser {
    #[allow(dead_code)]
    daemon: ServiceDaemon,
    event_receiver: Receiver<ServiceEvent>,
    receivers: HashMap<String, DiscoveredReceiver>,
}

impl MdnsBrowser {
    pub fn new() -> Result<Self, BrowseError> {
        Self::new_with_interface(None)
    }

    /// Browse only on one platform-selected physical LAN interface.
    ///
    /// iOS supplies the current Wi-Fi interface from Network.framework so a VPN tunnel cannot
    /// become the mDNS browse boundary (REQ-PICOO-DISCOVERY-008).
    pub fn new_on_interface(interface_name: &str) -> Result<Self, BrowseError> {
        if interface_name.is_empty() {
            return Err(BrowseError::Mdns("empty interface name".into()));
        }
        Self::new_with_interface(Some(interface_name))
    }

    fn new_with_interface(interface_name: Option<&str>) -> Result<Self, BrowseError> {
        let daemon = ServiceDaemon::new().map_err(|e| BrowseError::Mdns(e.to_string()))?;
        if let Some(interface_name) = interface_name {
            daemon
                .disable_interface(IfKind::All)
                .map_err(|e| BrowseError::Mdns(e.to_string()))?;
            daemon
                .enable_interface(interface_name)
                .map_err(|e| BrowseError::Mdns(e.to_string()))?;
        }
        let event_receiver = daemon
            .browse(SERVICE_TYPE)
            .map_err(|e| BrowseError::Mdns(e.to_string()))?;
        Ok(Self {
            daemon,
            event_receiver,
            receivers: HashMap::new(),
        })
    }

    pub fn poll(&mut self, timeout: Duration) -> Result<(), BrowseError> {
        match self.event_receiver.recv_timeout(timeout) {
            Ok(event) => self.handle_event(event),
            Err(_) => Ok(()),
        }
    }

    fn handle_event(&mut self, event: ServiceEvent) -> Result<(), BrowseError> {
        match event {
            ServiceEvent::ServiceResolved(info) => {
                let props: Vec<(String, String)> = info
                    .get_properties()
                    .iter()
                    .map(|prop| (prop.key().to_string(), prop.val_str().to_string()))
                    .collect();
                let host = info
                    .get_addresses()
                    .iter()
                    .next()
                    .map(|ip| ip.to_string())
                    .unwrap_or_else(|| info.get_hostname().trim_end_matches('.').into());
                self.apply_resolved_txt(info.get_fullname().to_string(), host, &props)?;
            }
            ServiceEvent::ServiceRemoved(_service_type, fullname) => {
                self.receivers.remove(&fullname);
            }
            _ => {}
        }
        Ok(())
    }

    pub fn list(&self) -> impl Iterator<Item = &DiscoveredReceiver> {
        self.receivers.values()
    }

    pub fn find(&self, receiver_id: &str) -> Option<&DiscoveredReceiver> {
        self.receivers
            .values()
            .find(|entry| entry.advertisement.receiver_id == receiver_id)
    }

    /// Apply a resolved TXT record set (same path as mDNS `ServiceResolved` / Android NSD).
    ///
    /// Used by Sender list updates and deterministic cache behavior tests (no multicast).
    pub fn apply_resolved_txt(
        &mut self,
        fullname: impl Into<String>,
        host: impl Into<String>,
        props: &[(String, String)],
    ) -> Result<(), BrowseError> {
        let ad = ReceiverAdvertisement::from_txt_properties(props)
            .map_err(|e| BrowseError::InvalidAd(e.to_string()))?;
        let fullname = fullname.into();
        self.receivers.insert(
            fullname.clone(),
            DiscoveredReceiver {
                fullname,
                advertisement: ad,
                host: host.into(),
            },
        );
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::types::{ReceiverAdvertisement, ReceiverPlatform};
    use mdns_sd::ServiceInfo;

    #[test]
    fn browser_starts_without_error() {
        let browser = MdnsBrowser::new();
        assert!(browser.is_ok());
    }

    #[test]
    fn interface_scoped_browser_rejects_an_empty_name() {
        assert!(MdnsBrowser::new_on_interface("").is_err());
    }

    #[test]
    fn resolved_service_event_updates_and_removes_cache() {
        let mut browser = MdnsBrowser::new().expect("browser");
        let ad = ReceiverAdvertisement::new(
            "recv-test",
            "Office PC",
            ReceiverPlatform::Windows,
            4433,
            "abcd1234",
        );
        let txt = ad.to_txt_properties();
        let properties: Vec<(&str, &str)> = txt
            .iter()
            .map(|(key, value)| (*key, value.as_str()))
            .collect();
        let info = ServiceInfo::new(
            SERVICE_TYPE,
            "Office PC",
            "recv-test.local.",
            "192.168.1.20",
            4433,
            &properties[..],
        )
        .expect("service info");
        let fullname = info.get_fullname().to_string();

        browser
            .handle_event(ServiceEvent::ServiceResolved(info))
            .expect("resolved event");

        assert_eq!(browser.list().count(), 1);
        let discovered = browser.find("recv-test").expect("receiver mapping");
        assert_eq!(discovered.fullname, fullname);
        assert_eq!(discovered.advertisement.display_name, "Office PC");
        assert_eq!(discovered.advertisement.quic_port, 4433);
        assert_eq!(discovered.host, "192.168.1.20");

        browser
            .handle_event(ServiceEvent::ServiceRemoved(SERVICE_TYPE.into(), fullname))
            .expect("removed event");
        assert_eq!(browser.list().count(), 0);
        assert!(browser.find("recv-test").is_none());
    }

    /// Requires working mDNS on the host — run manually on LAN (`cargo test -- --ignored`).
    /// REQ-PICOO-DISCOVERY-006: advertise→browse should land under P50 < 2s on healthy LAN.
    #[test]
    #[ignore = "requires LAN multicast permission and a second mDNS socket"]
    fn browser_discovers_local_advertiser_under_two_seconds() {
        use std::thread;
        use std::time::{Duration, Instant};

        use crate::{local_advertise_ipv4, MdnsAdvertiser};

        let ad = ReceiverAdvertisement::new(
            "browse-test-recv",
            "Browse Test PC",
            ReceiverPlatform::Windows,
            4433,
            "cafebabe",
        );
        let lan_ip = local_advertise_ipv4().expect("LAN IPv4 required for ignored mDNS test");
        let mut advertiser = MdnsAdvertiser::new().expect("advertiser");
        advertiser
            .register(&lan_ip.to_string(), &ad)
            .expect("register LAN address");

        let mut browser = MdnsBrowser::new().expect("browser");
        let t0 = Instant::now();
        let deadline = t0 + Duration::from_secs(5);
        while Instant::now() < deadline {
            browser.poll(Duration::from_millis(50)).expect("poll");
            if browser.find("browse-test-recv").is_some() {
                break;
            }
            thread::sleep(Duration::from_millis(20));
        }

        let discovered = browser
            .find("browse-test-recv")
            .expect("discovered receiver");
        let elapsed_ms = t0.elapsed().as_secs_f64() * 1000.0;
        eprintln!("mdns advertise→browse latency_ms={elapsed_ms:.2}");
        assert_eq!(discovered.advertisement.display_name, "Browse Test PC");
        assert_eq!(discovered.host, lan_ip.to_string());
        assert!(
            elapsed_ms < 2_000.0,
            "discovery {elapsed_ms}ms exceeds 2s P50 budget"
        );

        advertiser.unregister().expect("unregister");
    }

    #[test]
    fn resolved_txt_updates_cache() {
        let mut browser = MdnsBrowser::new().expect("browser");
        let mut ad = ReceiverAdvertisement::new(
            "recv-test",
            "Office PC",
            ReceiverPlatform::Windows,
            4433,
            "abcd1234",
        );
        let props = ad
            .to_txt_properties()
            .into_iter()
            .map(|(key, value)| (key.to_string(), value))
            .collect::<Vec<_>>();
        let fullname = "Office PC._picoocam._udp.local.";
        browser
            .apply_resolved_txt(fullname, "192.168.1.20", &props)
            .expect("initial TXT");

        ad.display_name = "Studio PC".into();
        let updated_props = ad
            .to_txt_properties()
            .into_iter()
            .map(|(key, value)| (key.to_string(), value))
            .collect::<Vec<_>>();
        browser
            .apply_resolved_txt(fullname, "192.168.1.21", &updated_props)
            .expect("updated TXT");

        assert_eq!(browser.list().count(), 1);
        let discovered = browser.find("recv-test").expect("updated receiver");
        assert_eq!(discovered.advertisement.display_name, "Studio PC");
        assert_eq!(discovered.host, "192.168.1.21");
    }
}
