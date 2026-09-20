//! REQ-PICOO-NEXT-011: immutable original submission identity and configuration.
use picoo_protocol::control::StreamConfig;
use std::sync::Arc;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum FrameKind {
    Key,
    ReferenceDelta,
    DiscardableDelta,
}

impl FrameKind {
    pub fn is_keyframe(self) -> bool {
        self == Self::Key
    }

    pub fn requires_refresh_when_dropped(self) -> bool {
        self != Self::DiscardableDelta
    }
}

#[derive(Debug, Clone, Copy)]
pub struct AccessUnitTimeline {
    pub connection_generation: u64,
    pub stream_generation: u64,
    pub frame_id: u64,
    pub source_pts_us: u64,
    pub encoded_at_us: u64,
    pub received_at_us: u64,
    pub decode_submitted_at_us: u64,
    pub kind: FrameKind,
}

#[derive(Debug)]
pub struct DecodeToken {
    pub timeline: AccessUnitTimeline,
    pub decoder_generation: u64,
    pub config_revision: u64,
    pub stream_config: Option<Arc<StreamConfig>>,
}

/// Only compressed bytes are borrowed. Delayed output retains the immutable token,
/// never the compressed AU allocation or mutable Receiver state.
pub struct DecodeSubmission<'a> {
    pub access_unit: &'a [u8],
    pub token: Arc<DecodeToken>,
}
