//! mDNS/DNS-SD advertiser for desktop Receiver.

use std::net::IpAddr;

use mdns_sd::{ServiceDaemon, ServiceInfo};
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
    registered: bool,
}

impl MdnsAdvertiser {
    pub fn new() -> Result<Self, DiscoveryError> {
        let daemon = ServiceDaemon::new().map_err(|e| DiscoveryError::Mdns(e.to_string()))?;
        Ok(Self {
            daemon,
            registered: false,
        })
    }

    pub fn register(
        &mut self,
        host_ip: &str,
        advertisement: &ReceiverAdvertisement,
    ) -> Result<(), DiscoveryError> {
        let ip: IpAddr = host_ip
            .parse()
            .map_err(|_| DiscoveryError::InvalidHost(host_ip.into()))?;

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

        self.daemon
            .register(info)
            .map_err(|e| DiscoveryError::Mdns(e.to_string()))?;
        self.registered = true;
        Ok(())
    }

    pub fn unregister(&mut self) -> Result<(), DiscoveryError> {
        if self.registered {
            self.daemon
                .shutdown()
                .map_err(|e| DiscoveryError::Mdns(e.to_string()))?;
            self.registered = false;
        }
        Ok(())
    }

    pub fn is_registered(&self) -> bool {
        self.registered
    }
}

impl Drop for MdnsAdvertiser {
    fn drop(&mut self) {
        let _ = self.unregister();
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
        assert!(advertiser.is_registered());
        advertiser.unregister().expect("unregister");
    }
}
