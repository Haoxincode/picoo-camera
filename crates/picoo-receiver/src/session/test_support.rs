//! Test-only adapters kept outside production media paths.

use bytes::Bytes;
use picoo_jitter::FrontFrameDescriptor;
use picoo_media_decode::AccessUnitDecoder;
use picoo_protocol::control::StreamConfig;
use std::sync::Arc;

use super::decoder_worker::{DecoderWorker, EncodedAccessUnit, FrameKind};
use super::ReceiverSession;
use crate::media_scheduler::RecoveryAdmission;
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
        let connection_generation = self.control_generation.unwrap_or(1);
        self.control_generation = Some(connection_generation);
        let stream_generation = self
            .current_stream_config
            .as_ref()
            .map_or(1, |config| u64::from(config.stream_epoch));
        if self.current_stream_config.is_none() {
            self.current_stream_config = Some(Arc::new(StreamConfig {
                codec: "h264".into(),
                width: 1280,
                height: 720,
                fps: 30,
                stream_epoch: stream_generation as u32,
                ..Default::default()
            }));
        }
        let recovery_admission = self.decoder_recovery.admission(
            connection_generation,
            FrontFrameDescriptor {
                stream_generation,
                frame_id: 0,
                keyframe,
                discardable: false,
            },
        );
        if recovery_admission != RecoveryAdmission::Ready {
            self.ingress.recovery_dropped_access_units =
                self.ingress.recovery_dropped_access_units.saturating_add(1);
            return Ok(());
        }
        let now_us = self.timing_origin.elapsed().as_micros() as u64;
        self.publish_timeline_access_unit(EncodedAccessUnit {
            connection_generation,
            stream_generation,
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
