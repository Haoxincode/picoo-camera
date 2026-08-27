//! Device discovery — REQ-PICOO-DISCOVERY-001..004, ARCH-PICOO-DISCOVERY-001.

mod advertise;
mod browse;
mod host;
mod qr;
mod types;

pub use advertise::{DiscoveryError, MdnsAdvertiser};
pub use browse::{BrowseError, DiscoveredReceiver, MdnsBrowser};
pub use host::{
    local_advertise_host, local_advertise_ipv4, select_advertise_ipv4, DEFAULT_QUIC_PORT,
};
pub use qr::{
    generate_nonce, QrConnectPayload, QrPayloadError, DEFAULT_QR_TTL_MS, QR_PAYLOAD_VERSION,
};
pub use types::{PairingState, ReceiverAdvertisement, SERVICE_TYPE};
