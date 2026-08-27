//! mDNS browser for Sender-side receiver discovery — REQ-PICOO-DISCOVERY-001.

use std::collections::HashMap;
use std::time::Duration;

use mdns_sd::{Receiver, ServiceDaemon, ServiceEvent};
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
        let daemon = ServiceDaemon::new().map_err(|e| BrowseError::Mdns(e.to_string()))?;
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
                let ad = ReceiverAdvertisement::from_txt_properties(&props)
                    .map_err(|e| BrowseError::InvalidAd(e.to_string()))?;
                let host = info
                    .get_addresses()
                    .iter()
                    .next()
                    .map(|ip| ip.to_string())
                    .unwrap_or_else(|| info.get_hostname().trim_end_matches('.').into());
                let fullname = info.get_fullname().to_string();
                self.receivers.insert(
                    fullname.clone(),
                    DiscoveredReceiver {
                        fullname,
                        advertisement: ad,
                        host,
                    },
                );
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
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::types::ReceiverAdvertisement;

    #[test]
    fn browser_starts_without_error() {
        let browser = MdnsBrowser::new();
        assert!(browser.is_ok());
    }

    #[test]
    fn resolved_service_populates_list() {
        let mut browser = MdnsBrowser::new().expect("browser");
        let ad = ReceiverAdvertisement::new("recv-test", "Office PC", 4433, "abcd1234");
        browser.receivers.insert(
            "Office PC._picoocam._udp.local.".into(),
            DiscoveredReceiver {
                fullname: "Office PC._picoocam._udp.local.".into(),
                advertisement: ad,
                host: "192.168.1.20".into(),
            },
        );
        assert_eq!(browser.list().count(), 1);
        assert!(browser.find("recv-test").is_some());
    }

    /// Requires working mDNS on the host — run manually on LAN (`cargo test -- --ignored`).
    #[test]
    #[ignore = "mDNS loopback is unreliable in CI/cloud VMs"]
    fn browser_discovers_local_advertiser() {
        use std::thread;
        use std::time::Duration;

        use crate::MdnsAdvertiser;

        let ad = ReceiverAdvertisement::new("browse-test-recv", "Browse Test PC", 4433, "cafebabe");
        let mut advertiser = MdnsAdvertiser::new().expect("advertiser");
        advertiser
            .register("127.0.0.1", &ad)
            .expect("register localhost");

        let mut browser = MdnsBrowser::new().expect("browser");
        let deadline = std::time::Instant::now() + Duration::from_secs(5);
        while std::time::Instant::now() < deadline {
            browser.poll(Duration::from_millis(200)).expect("poll");
            if browser.find("browse-test-recv").is_some() {
                break;
            }
            thread::sleep(Duration::from_millis(50));
        }

        let discovered = browser
            .find("browse-test-recv")
            .expect("discovered receiver");
        assert_eq!(discovered.advertisement.display_name, "Browse Test PC");
        assert_eq!(discovered.host, "127.0.0.1");

        advertiser.unregister().expect("unregister");
    }
}
