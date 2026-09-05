//! LatestFrameStore, Shared Frame Ring, placeholders, and H.264 decode publish.
//!
//! REQ-PICOO-FRAME-*, REQ-PICOO-MEDIA-004/006/009/017/023.

use bytes::Bytes;
use picoo_frame_hub::{PlaceholderMode, PLACEHOLDER_HEIGHT, PLACEHOLDER_WIDTH};
use picoo_jitter::{Frame as JitterFrame, PushOutcome};
use picoo_media_decode::DecodedFrame;
use picoo_packet::AssembledAccessUnit;
#[cfg(test)]
use picoo_protocol::VideoPacket;
#[cfg(test)]
use picoo_transport::ReceivedVideoPacketBatch;
#[cfg(test)]
use std::sync::Arc;
use std::time::Instant;

#[cfg(test)]
use super::decoder_worker::{AccessUnitTimeline, DecoderWorker, FrameKind};
use super::decoder_worker::{DecodeSubmitOutcome, EncodedAccessUnit};
use super::media_publish::FrameTimeline;
use super::recovery::RecoveryReason;
use super::ReceiverSession;
use crate::{ReceiverError, DEFAULT_SHARED_RING_NAME};

impl ReceiverSession {
    pub(super) fn release_pending_stream_config_idr(
        &mut self,
        stream_epoch: u32,
    ) -> Result<(), ReceiverError> {
        if !self
            .pending_stream_config_idr
            .as_ref()
            .is_some_and(|access_unit| access_unit.stream_epoch == stream_epoch)
        {
            return Ok(());
        }
        let access_unit = self
            .pending_stream_config_idr
            .take()
            .expect("matching pending StreamConfig IDR exists");
        self.queue_assembled_access_unit(access_unit)?;
        Ok(())
    }

    pub(super) fn queue_assembled_access_unit(
        &mut self,
        access_unit: AssembledAccessUnit,
    ) -> Result<(), ReceiverError> {
        let pts_us = access_unit.pts_us;
        let completed_at = Instant::now();
        if access_unit.keyframe {
            tracing::warn!(
                bytes = access_unit.data.len(),
                fragments = access_unit.fragment_count,
                assembly_ms = completed_at
                    .saturating_duration_since(access_unit.first_fragment_at)
                    .as_secs_f64()
                    * 1_000.0,
                "complete keyframe reached receiver reassembly"
            );
        }
        self.interarrival_jitter.observe(completed_at, pts_us);
        let first_fragment_at_us = access_unit
            .first_fragment_at
            .saturating_duration_since(self.timing_origin)
            .as_micros() as u64;
        let completed_at_us = completed_at
            .saturating_duration_since(self.timing_origin)
            .as_micros() as u64;
        let outcome = self.jitter.push_at(
            JitterFrame {
                stream_generation: u64::from(access_unit.stream_epoch),
                frame_id: access_unit.frame_id,
                pts_us,
                encoded_at_us: access_unit.encoded_at_us,
                received_at_us: completed_at_us,
                data: access_unit.data,
                keyframe: access_unit.keyframe,
                discardable: access_unit.discardable,
            },
            first_fragment_at_us,
            completed_at_us,
        );
        match outcome {
            PushOutcome::AcceptedAfterReferenceDrop => {
                self.ingress.recovery_jitter_capacity =
                    self.ingress.recovery_jitter_capacity.saturating_add(1);
                self.enter_decoder_recovery(RecoveryReason::ReferenceAccessUnitLate, true)?
            }
            PushOutcome::DroppedLate {
                requires_refresh: true,
            } => {
                self.ingress.recovery_arrived_after_playout = self
                    .ingress
                    .recovery_arrived_after_playout
                    .saturating_add(1);
                self.enter_decoder_recovery(RecoveryReason::ReferenceAccessUnitLate, true)?
            }
            PushOutcome::Accepted
            | PushOutcome::DroppedLate {
                requires_refresh: false,
            } => {}
        }
        Ok(())
    }

    /// Attach a cross-process Shared Frame Ring for VCam consumption (REQ-PICOO-FRAME-003).
    pub fn attach_shared_ring(&mut self, name: &str) -> Result<(), ReceiverError> {
        let name = name.to_owned();
        let use_platform_ring = name == DEFAULT_SHARED_RING_NAME;
        let ring = picoo_frame_hub::SharedFrameRingWriter::start(move || {
            #[cfg(target_os = "windows")]
            if use_platform_ring {
                return picoo_frame_hub::SharedFrameRingProducer::open_or_create_file(
                    picoo_frame_hub::windows_shared_ring_path(&name),
                    picoo_frame_hub::DEFAULT_MAX_FRAME_BYTES,
                );
            }
            #[cfg(target_os = "macos")]
            if use_platform_ring {
                let path = picoo_frame_hub::macos_app_group_ring_path(&name)?;
                return picoo_frame_hub::SharedFrameRingProducer::open_or_create_file(
                    path,
                    picoo_frame_hub::DEFAULT_MAX_FRAME_BYTES,
                );
            }
            #[cfg(not(any(target_os = "macos", target_os = "windows")))]
            let _ = use_platform_ring;
            picoo_frame_hub::SharedFrameRingProducer::open_or_create(
                &name,
                picoo_frame_hub::DEFAULT_MAX_FRAME_BYTES,
            )
        })?;
        self.shared_ring = Some(ring);
        self.last_shared_ring_error = None;
        self.publish_waiting_placeholder()?;
        Ok(())
    }

    pub(super) fn drain_shared_ring_events(&mut self) {
        let Some(ring) = self.shared_ring.as_ref() else {
            return;
        };
        while let Some(event) = ring.poll_event() {
            match event {
                picoo_frame_hub::SharedRingWriterEvent::Published { .. } => {
                    self.last_shared_ring_error = None;
                }
                picoo_frame_hub::SharedRingWriterEvent::Failed { error, .. } => {
                    tracing::warn!(%error, "Shared Frame Ring output failed");
                    self.last_shared_ring_error = Some(error.to_string());
                }
            }
        }
    }

    pub fn publish_waiting_placeholder(&mut self) -> Result<(), ReceiverError> {
        let nv12 = self.placeholder_mode.waiting_frame();
        self.publish_decoded_frame(
            FrameTimeline::default(),
            DecodedFrame::cpu_nv12(
                PLACEHOLDER_WIDTH,
                PLACEHOLDER_HEIGHT,
                PLACEHOLDER_WIDTH,
                0,
                0,
                Bytes::from(nv12),
            ),
        )
    }

    /// Publish reconnect-branded placeholder (REQ-PICOO-FRAME-005).
    pub fn publish_reconnecting_placeholder(&mut self) -> Result<(), ReceiverError> {
        let nv12 = self.placeholder_mode.reconnecting_frame();
        self.publish_decoded_frame(
            FrameTimeline::default(),
            DecodedFrame::cpu_nv12(
                PLACEHOLDER_WIDTH,
                PLACEHOLDER_HEIGHT,
                PLACEHOLDER_WIDTH,
                0,
                0,
                Bytes::from(nv12),
            ),
        )
    }

    pub fn set_placeholder_mode(&mut self, mode: PlaceholderMode) {
        self.placeholder_mode = mode;
    }

    pub fn placeholder_mode(&self) -> PlaceholderMode {
        self.placeholder_mode
    }

    /// Decode one typed H.264 access unit into one shared VideoFrame.
    pub(super) fn publish_timeline_access_unit(
        &mut self,
        access_unit: EncodedAccessUnit,
    ) -> Result<(), ReceiverError> {
        self.ingress.access_units += 1;
        if self
            .current_stream_config
            .as_ref()
            .is_some_and(|config| u64::from(config.stream_epoch) != access_unit.stream_generation)
        {
            self.ingress.recovery_dropped_access_units =
                self.ingress.recovery_dropped_access_units.saturating_add(1);
            return Ok(());
        }
        let timeline = access_unit.timeline();
        match self
            .decoder_worker
            .submit(access_unit, self.current_stream_config.clone())
        {
            DecodeSubmitOutcome::Queued => {
                self.decoder_recovery.note_refresh_submitted(timeline);
            }
            DecodeSubmitOutcome::Dropped { requires_refresh } => {
                self.ingress.recovery_dropped_access_units =
                    self.ingress.recovery_dropped_access_units.saturating_add(1);
                if requires_refresh {
                    self.enter_decoder_recovery(RecoveryReason::DecoderQueuePressure, true)?;
                }
            }
        }
        Ok(())
    }
}

#[cfg(test)]
#[path = "media_tests.rs"]
mod tests;
