//! PCP protocol — enveloped Protobuf control plane and FEC-protected VideoPacket.
//!
//! Requirement mapping: REQ-PICOO-PROTOCOL-*

use bytes::Bytes;
use prost::Message;
use thiserror::Error;

pub mod control {
    include!(concat!(env!("OUT_DIR"), "/picoo.camera.rs"));
}

mod control_gate;
mod video_fec;
mod video_packet;

pub use control_gate::{receiver_payload_allowed, ReceiverControlPhase};

pub use video_fec::{
    fec_group_ranges, make_fec_parity, reconstruct_fec_group, FecParityShard, FEC_DATA_SHARDS,
    FEC_PARITY_PREFIX_SIZE, FEC_PARITY_SHARDS,
};
pub use video_packet::{VideoPacket, VideoPacketError, VideoPacketFlags};

/// QUIC Application-Layer Protocol Negotiation identifier for PCP.
pub const ALPN: &str = "picoocam";

#[derive(Debug, Error)]
pub enum ControlEnvelopeError {
    #[error("control envelope decode: {0}")]
    Decode(#[from] prost::DecodeError),
    #[error("control envelope message_id must be non-zero")]
    MissingMessageId,
    #[error("control envelope connection_generation must be non-zero")]
    MissingConnectionGeneration,
    #[error("control envelope payload is missing")]
    MissingPayload,
}

pub fn encode_control_envelope(
    payload: control::control_envelope::Payload,
    message_id: u64,
    connection_generation: u64,
) -> Bytes {
    debug_assert_ne!(message_id, 0);
    debug_assert_ne!(connection_generation, 0);
    Bytes::from(
        control::ControlEnvelope {
            message_id,
            connection_generation,
            payload: Some(payload),
        }
        .encode_to_vec(),
    )
}

pub fn decode_control_envelope(
    bytes: &[u8],
) -> Result<control::ControlEnvelope, ControlEnvelopeError> {
    let envelope = control::ControlEnvelope::decode(bytes)?;
    if envelope.message_id == 0 {
        return Err(ControlEnvelopeError::MissingMessageId);
    }
    if envelope.connection_generation == 0 {
        return Err(ControlEnvelopeError::MissingConnectionGeneration);
    }
    if envelope.payload.is_none() {
        return Err(ControlEnvelopeError::MissingPayload);
    }
    Ok(envelope)
}

/// Maximum QUIC datagram size (path MTU safe).
pub const MAX_DATAGRAM_SIZE: usize = 1150;
/// Bounded H.264 access-unit size: about 1.1 MiB at the current datagram MTU.
/// This accommodates product-resolution IDRs while keeping reassembly memory finite.
pub const MAX_VIDEO_FRAGMENTS_PER_ACCESS_UNIT: u16 = 1024;

/// Fixed header size in bytes.
pub const VIDEO_PACKET_HEADER_SIZE: usize = 33;
/// Data fragments leave room for the FEC parity metadata prefix so parity and
/// data always fit the same path-MTU-safe QUIC Datagram size.
pub const MAX_FEC_FRAGMENT_PAYLOAD: usize =
    MAX_DATAGRAM_SIZE - VIDEO_PACKET_HEADER_SIZE - FEC_PARITY_PREFIX_SIZE;

#[cfg(test)]
mod control_envelope_tests {
    use super::*;
    use control::control_envelope::Payload;

    #[test]
    fn envelope_roundtrip_preserves_identity_and_payload() {
        let encoded = encode_control_envelope(Payload::StartStream(control::StartStream {}), 7, 11);
        let decoded = decode_control_envelope(&encoded).expect("valid envelope");
        assert_eq!(decoded.message_id, 7);
        assert_eq!(decoded.connection_generation, 11);
        assert!(matches!(decoded.payload, Some(Payload::StartStream(_))));
    }

    #[test]
    fn rejects_missing_message_id_generation_and_payload() {
        let encode = |envelope: control::ControlEnvelope| envelope.encode_to_vec();
        assert!(matches!(
            decode_control_envelope(&encode(control::ControlEnvelope {
                message_id: 0,
                connection_generation: 1,
                payload: Some(Payload::StopStream(control::StopStream {})),
            })),
            Err(ControlEnvelopeError::MissingMessageId)
        ));
        assert!(matches!(
            decode_control_envelope(&encode(control::ControlEnvelope {
                message_id: 1,
                connection_generation: 0,
                payload: Some(Payload::StopStream(control::StopStream {})),
            })),
            Err(ControlEnvelopeError::MissingConnectionGeneration)
        ));
        assert!(matches!(
            decode_control_envelope(&encode(control::ControlEnvelope {
                message_id: 1,
                connection_generation: 1,
                payload: None,
            })),
            Err(ControlEnvelopeError::MissingPayload)
        ));
    }

    #[test]
    fn rejects_bare_control_payload() {
        let bare = control::StartStream {}.encode_to_vec();
        assert!(decode_control_envelope(&bare).is_err());
    }
}
