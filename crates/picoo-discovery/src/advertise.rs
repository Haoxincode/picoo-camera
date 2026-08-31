//! mDNS/DNS-SD advertiser for desktop Receiver.

use std::net::IpAddr;
use std::time::Duration;

use mdns_sd::{DaemonEvent, IfKind, Receiver, ServiceDaemon, ServiceInfo};
use thiserror::Error;

use crate::types::{ReceiverAdvertisement, SERVICE_TYPE};

#[derive(Debug, Error)]
pub enum DiscoveryError {
    #[error("mdns: {0}")]
    Mdns(String),
    #[error("invalid host IP: {0}")]
    InvalidHost(String),
}

pub struct MdnsAdvertiser {
    daemon: ServiceDaemon,
    monitor: Receiver<DaemonEvent>,
    /// Full DNS-SD instance name currently registered (`instance._picoocam._udp.local.`).
    fullname: Option<String>,
    registered: bool,
    advertise_ip: Option<IpAddr>,
    last_error: Option<String>,
}

impl MdnsAdvertiser {
    pub fn new() -> Result<Self, DiscoveryError> {
        let daemon = ServiceDaemon::new().map_err(|e| DiscoveryError::Mdns(e.to_string()))?;
        let monitor = daemon
            .monitor()
            .map_err(|e| DiscoveryError::Mdns(e.to_string()))?;
        Ok(Self {
            daemon,
            monitor,
            fullname: None,
            registered: false,
            advertise_ip: None,
            last_error: None,
        })
    }

    /// Register (or replace) the receiver advertisement. Keeps the daemon alive across renames.
    pub fn register(
        &mut self,
        host_ip: &str,
        advertisement: &ReceiverAdvertisement,
    ) -> Result<(), DiscoveryError> {
        // Unregister prior instance so display_name / TXT updates take effect (DISCOVERY-001).
        self.unregister_service()?;

        let ip: IpAddr = host_ip
            .parse()
            .map_err(|_| DiscoveryError::InvalidHost(host_ip.into()))?;
        self.advertise_ip = Some(ip);
        self.registered = false;
        self.last_error = None;

        // Advertise only on the interface that owns the LAN address selected by
        // `local_advertise_ipv4`. Leaving the daemon on its all-interface default
        // lets VPN/Hyper-V/WSL adapters become mDNS egress candidates on desktop
        // platforms even though the TXT/A record contains the Wi-Fi address.
        self.daemon
            .disable_interface(IfKind::All)
            .map_err(|e| DiscoveryError::Mdns(e.to_string()))?;
        self.daemon
            .enable_interface(ip)
            .map_err(|e| DiscoveryError::Mdns(e.to_string()))?;

        let hostname = format!("{}.local.", advertisement.receiver_id);
        let instance = advertisement.display_name.clone();
        let txt = advertisement.to_txt_properties();
        let properties: Vec<(&str, &str)> = txt.iter().map(|(k, v)| (*k, v.as_str())).collect();

        let info = ServiceInfo::new(
            SERVICE_TYPE,
            &instance,
            &hostname,
            ip,
            advertisement.quic_port,
            &properties[..],
        )
        .map_err(|e| DiscoveryError::Mdns(e.to_string()))?;

        let fullname = info.get_fullname().to_string();
        self.daemon
            .register(info)
            .map_err(|e| DiscoveryError::Mdns(e.to_string()))?;
        self.fullname = Some(fullname);
        Ok(())
    }

    /// Drain daemon events and update the real announcement state.
    ///
    /// `ServiceDaemon::register` only queues work. Treating that return value as
    /// an active broadcast made the desktop report discovery online even when
    /// Windows Firewall or an interface error prevented any announcement.
    pub fn poll(&mut self) -> bool {
        let before = self.registered;
        while let Ok(event) = self.monitor.try_recv() {
            match event {
                // This daemon owns one Picoo service, so a successful announce
                // confirms that the current registration reached an interface.
                DaemonEvent::Announce(_, _) => {
                    self.registered = self.fullname.is_some();
                    self.last_error = None;
                }
                DaemonEvent::Error(error) => {
                    self.registered = false;
                    self.last_error = Some(error.to_string());
                }
                DaemonEvent::IpDel(ip) if self.advertise_ip == Some(ip) => {
                    self.registered = false;
                    self.last_error = Some(format!("advertise interface {ip} disappeared"));
                }
                _ => {}
            }
        }
        before != self.registered
    }

    /// Drop the current service registration without shutting down the daemon.
    pub fn unregister(&mut self) -> Result<(), DiscoveryError> {
        self.unregister_service()
    }

    fn unregister_service(&mut self) -> Result<(), DiscoveryError> {
        let Some(fullname) = self.fullname.take() else {
            self.registered = false;
            return Ok(());
        };
        match self.daemon.unregister(&fullname) {
            Ok(rx) => {
                // Wait briefly so a subsequent register with a new instance name is clean.
                let _ = rx.recv_timeout(Duration::from_secs(2));
            }
            Err(_) => {
                // Best-effort: daemon may already have dropped the instance.
            }
        }
        self.registered = false;
        Ok(())
    }

    pub fn is_registered(&self) -> bool {
        self.registered
    }

    pub fn fullname(&self) -> Option<&str> {
        self.fullname.as_deref()
    }

    pub fn last_error(&self) -> Option<&str> {
        self.last_error.as_deref()
    }
}

impl Drop for MdnsAdvertiser {
    fn drop(&mut self) {
        let _ = self.unregister_service();
        let _ = self.daemon.shutdown();
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::types::ReceiverAdvertisement;

    #[test]
    fn mdns_register_localhost() {
        let mut advertiser = MdnsAdvertiser::new().expect("daemon");
        let ad = ReceiverAdvertisement::new("picoo-test-recv", "Picoo Test", 4433, "deadbeef");
        advertiser
            .register("127.0.0.1", &ad)
            .expect("register localhost");
        wait_until_announced(&mut advertiser);
        advertiser.unregister().expect("unregister");
        assert!(!advertiser.is_registered());
    }

    #[test]
    fn mdns_reregister_with_new_display_name() {
        let mut advertiser = MdnsAdvertiser::new().expect("daemon");
        let first = ReceiverAdvertisement::new("picoo-rename-recv", "Old Name", 4433, "abcd1234");
        advertiser
            .register("127.0.0.1", &first)
            .expect("register first");
        let first_fullname = advertiser.fullname().unwrap().to_string();
        assert!(
            first_fullname.contains("Old Name") || first_fullname.to_lowercase().contains("old")
        );

        let second = ReceiverAdvertisement::new("picoo-rename-recv", "New Name", 4433, "abcd1234");
        advertiser
            .register("127.0.0.1", &second)
            .expect("register renamed");
        wait_until_announced(&mut advertiser);
        let second_fullname = advertiser.fullname().unwrap();
        assert!(
            second_fullname.contains("New Name") || second_fullname.to_lowercase().contains("new"),
            "fullname should reflect rename: {second_fullname}"
        );
        assert_ne!(first_fullname, second_fullname);
        advertiser.unregister().expect("unregister");
    }

    fn wait_until_announced(advertiser: &mut MdnsAdvertiser) {
        let deadline = std::time::Instant::now() + Duration::from_secs(3);
        while std::time::Instant::now() < deadline {
            advertiser.poll();
            if advertiser.is_registered() {
                return;
            }
            std::thread::sleep(Duration::from_millis(20));
        }
        panic!("mDNS was not announced: {:?}", advertiser.last_error());
    }
}
