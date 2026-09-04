//! Test-only adapters kept outside production media paths.

use bytes::Bytes;
use picoo_media_decode::AccessUnitDecoder;

use super::decoder_worker::{DecoderWorker, EncodedAccessUnit, FrameKind};
use super::ReceiverSession;
use crate::ReceiverError;

impl ReceiverSession {
    /// Inject a synthetic decoder without adding fallback behavior to builds.
    pub fn set_decoder_for_test(&mut self, decoder: Box<dyn AccessUnitDecoder>) {
        self.decoder_worker = DecoderWorker::with_decoder(decoder);
    }

    /// Publish a recovery fixture without constructing a network timeline.
    pub(crate) fn publish_access_unit(
        &mut self,
        access_unit: Bytes,
        keyframe: bool,
    ) -> Result<(), ReceiverError> {
        let now_us = self.timing_origin.elapsed().as_micros() as u64;
        self.publish_timeline_access_unit(EncodedAccessUnit {
            connection_generation: self
                .transport
                .active_session()
                .map_or(0, |session| session.0),
            stream_generation: self
                .current_stream_config
                .as_ref()
                .map_or(0, |config| u64::from(config.stream_epoch)),
            frame_id: 0,
            source_pts_us: 0,
            encoded_at_us: 0,
            received_at_us: now_us,
            decode_submitted_at_us: now_us,
            kind: if keyframe {
                FrameKind::Key
            } else {
                FrameKind::ReferenceDelta
            },
            data: access_unit,
        })
    }
}
