//! Device discovery — REQ-PICOO-DISCOVERY-001/002/005, ARCH-PICOO-DISCOVERY-001.

mod advertise;
mod browse;
mod host;
mod types;

pub use advertise::{DiscoveryError, MdnsAdvertiser};
pub use browse::{BrowseError, DiscoveredReceiver, MdnsBrowser};
pub use host::{
    local_advertise_host, local_advertise_ipv4, select_advertise_ipv4, DEFAULT_QUIC_PORT,
};
pub use types::{PairingState, ReceiverAdvertisement, ReceiverPlatform, SERVICE_TYPE};
