use crate::{
    AccessUnitDecoder, AccessUnitTimeline, DecodeError, DecodeOutcome, DecodeSubmission,
    DecodeToken, FrameKind,
};
use picoo_protocol::control::StreamConfig;

/// Explicit fixture adapter. Production always supplies a complete submission token.
pub trait DecodeFixture: AccessUnitDecoder {
    fn decode_fixture(
        &mut self,
        access_unit: &[u8],
        config: Option<&StreamConfig>,
    ) -> Result<DecodeOutcome, DecodeError> {
        self.submit(DecodeSubmission {
            access_unit,
            token: std::sync::Arc::new(DecodeToken {
                timeline: AccessUnitTimeline {
                    connection_generation: 1,
                    stream_generation: config.map_or(1, |c| u64::from(c.stream_epoch)),
                    frame_id: 1,
                    source_pts_us: 0,
                    encoded_at_us: 0,
                    received_at_us: 0,
                    decode_submitted_at_us: 0,
                    kind: FrameKind::Key,
                },
                decoder_generation: 0,
                config_revision: 0,
                stream_config: config.cloned().map(std::sync::Arc::new),
            }),
        })
    }
}
impl<T: AccessUnitDecoder + ?Sized> DecodeFixture for T {}
