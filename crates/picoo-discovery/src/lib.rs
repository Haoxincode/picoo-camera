//! Device discovery — REQ-PICOO-DISCOVERY-001..004, ARCH-PICOO-DISCOVERY-001.

mod advertise;
mod browse;
mod qr;
mod types;

pub use advertise::{DiscoveryError, MdnsAdvertiser};
pub use browse::{BrowseError, DiscoveredReceiver, MdnsBrowser};
pub use qr::{
    generate_nonce, QrConnectPayload, QrPayloadError, DEFAULT_QR_TTL_MS, QR_PAYLOAD_VERSION,
};
pub use types::{PairingState, ReceiverAdvertisement, SERVICE_TYPE};
