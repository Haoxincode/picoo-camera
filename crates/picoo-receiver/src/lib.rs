//! Desktop receiver session: QUIC ingress → reassembly → FrameHub.
//!
//! REQ-PICOO-FRAME-001, REQ-PICOO-MEDIA-005/006 via picoo-media-decode.
//! REQ-PICOO-PAIRING-*: ClientHello/ServerHello gate before video ingress.

mod session;

use std::time::Duration;

use picoo_media_decode::DecodeError;
use picoo_pairing::{DeviceIdentity, IdentityError, PairingError, StoreError};
use picoo_transport::TransportError;
use thiserror::Error;

pub use session::{
    run_loopback_access_unit, run_paired_loopback_access_unit, ReceiverSession,
    TrustedIdentityCandidate, TrustedIdentityReplacement,
};

pub const DEFAULT_SHARED_RING_NAME: &str = "picoo-camera-v1";

/// Pairing short-code / challenge lifetime (matches Android PairingScreen TTL).
pub const PAIRING_CHALLENGE_TTL: Duration = Duration::from_secs(60);

#[derive(Debug, Error)]
pub enum ReceiverError {
    #[error("transport: {0}")]
    Transport(#[from] TransportError),
    #[error("sender: {0}")]
    Sender(#[from] picoo_sender::SenderError),
    #[error("frame hub: {0}")]
    FrameHub(#[from] picoo_frame_hub::FrameHubError),
    #[error("pairing: {0}")]
    Pairing(#[from] PairingError),
    #[error("protocol: {0}")]
    Protocol(String),
    #[error("shared ring: {0}")]
    SharedRing(#[from] picoo_frame_hub::SharedRingError),
    #[error("pairing store: {0}")]
    Store(#[from] StoreError),
    #[error("device identity: {0}")]
    Identity(#[from] IdentityError),
    #[error("decode: {0}")]
    Decode(#[from] DecodeError),
    #[error("not listening")]
    NotListening,
    #[error("trusted identity replacement decision is stale")]
    StaleTrustedIdentityReplacement,
    #[error("loopback timeout")]
    LoopbackTimeout,
}

#[derive(Debug, Clone)]
pub struct ReceiverIdentity {
    signer: DeviceIdentity,
}

impl ReceiverIdentity {
    pub fn from_device_identity(identity: DeviceIdentity) -> Self {
        Self { signer: identity }
    }

    pub fn receiver_id(&self) -> &str {
        self.signer.device_id()
    }

    pub fn display_name(&self) -> &str {
        self.signer.device_name()
    }

    pub fn public_key(&self) -> &[u8] {
        self.signer.public_key()
    }

    pub fn set_display_name(&mut self, display_name: impl Into<String>) {
        let display_name = display_name.into();
        self.signer.set_device_name(&display_name);
    }

    fn signer(&self) -> &DeviceIdentity {
        &self.signer
    }
}

impl Default for ReceiverIdentity {
    fn default() -> Self {
        let identity = DeviceIdentity::generate("Picoo Camera")
            .expect("OS CSPRNG must be available when constructing a Receiver identity");
        Self::from_device_identity(identity)
    }
}

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct IngressStats {
    pub access_units: u64,
    pub packets_received: u64,
    /// Data fragments reconstructed from PCP FEC parity before AU expiry.
    pub fec_recovered_fragments: u64,
    /// Incomplete AUs for which Receiver observed at least one data fragment.
    pub reassembly_partial_access_unit_drops: u64,
    /// Missing whole AUs inferred from a discontinuity in Sender frame ids.
    pub reassembly_whole_access_unit_gap_drops: u64,
    pub packets_dropped_unpaired: u64,
    /// Times the decoder was invoked (REQ-PICOO-MEDIA-006: once per AU).
    pub decode_invocations: u64,
    /// Frames successfully decoded and committed to FrameHub.
    pub decoded_frames: u64,
    /// Delta AUs discarded while the decoder waits for a fresh IDR.
    pub recovery_dropped_access_units: u64,
    /// Decoder prediction-state resets after epoch changes or decode failures.
    pub decoder_resets: u64,
    /// Recovery entries caused by an incomplete/expired reference AU.
    pub recovery_reference_lost: u64,
    /// Recovery entries caused by a complete AU missing its playout deadline.
    pub recovery_reference_late: u64,
    /// Complete reference AUs evicted because the bounded Jitter Buffer filled.
    pub recovery_jitter_capacity: u64,
    /// Complete reference AUs that arrived after a newer AU was already emitted.
    pub recovery_arrived_after_playout: u64,
    /// Complete reference AUs that remained queued beyond the local hard deadline.
    pub recovery_jitter_expired: u64,
    /// Recovery entries caused by a platform decoder error.
    pub recovery_decoder_errors: u64,
    /// Keyframe requests successfully queued on the reliable control stream.
    pub keyframe_requests: u64,
    /// StartStream / CameraCommand rejected while unpaired (PAIRING-003).
    pub control_rejected_unpaired: u64,
}

#[cfg(test)]
mod tests;
