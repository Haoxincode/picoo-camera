//! LatestFrameStore, Shared Frame Ring, placeholders, and H.264 decode publish.
//!
//! REQ-PICOO-FRAME-*, REQ-PICOO-MEDIA-004/006/009.

use bytes::Bytes;
use picoo_frame_hub::{
    PlaceholderMode, SharedFrameRingProducer, VideoFrame, PLACEHOLDER_HEIGHT, PLACEHOLDER_WIDTH,
};
use picoo_jitter::{Frame as JitterFrame, PushOutcome};
#[cfg(test)]
use picoo_media_decode::AccessUnitDecoder;
use picoo_media_decode::DecodedFrame;
use picoo_packet::ReassemblyError;
use picoo_protocol::VideoPacket;
use std::sync::Arc;
use std::time::Instant;

use super::recovery::RecoveryReason;
use super::ReceiverSession;
use crate::ReceiverError;
#[cfg(any(target_os = "macos", target_os = "windows"))]
use crate::DEFAULT_SHARED_RING_NAME;

#[derive(Debug)]
pub(super) struct EncodedAccessUnit {
    pub(super) stream_generation: u64,
    pub(super) frame_id: u64,
    pub(super) source_pts_us: u64,
    pub(super) received_at_us: u64,
    pub(super) kind: FrameKind,
    pub(super) data: Bytes,
}

#[cfg(test)]
mod tests {
    use super::*;
    use picoo_media_decode::StubDecoder;
    use picoo_protocol::control::StreamConfig;

    fn receiver_for_generation(generation: u32) -> ReceiverSession {
        let mut receiver = ReceiverSession::new();
        receiver.decoder = Box::new(StubDecoder::new());
        receiver.current_stream_config = Some(StreamConfig {
            codec: "h264".into(),
            width: 1280,
            height: 720,
            fps: 30,
            stream_epoch: generation,
            ..Default::default()
        });
        receiver
    }

    fn access_unit(generation: u64, frame_id: u64) -> EncodedAccessUnit {
        EncodedAccessUnit {
            stream_generation: generation,
            frame_id,
            source_pts_us: 42_000,
            received_at_us: 50_000,
            kind: FrameKind::Key,
            data: Bytes::from_static(b"typed-au"),
        }
    }

    #[test]
    fn stale_generation_never_reaches_decoder_or_latest_store() {
        let mut receiver = receiver_for_generation(2);

        receiver
            .publish_timeline_access_unit(access_unit(1, 9))
            .expect("stale completion is an expected drop");

        assert_eq!(receiver.ingress.decode_invocations, 0);
        assert_eq!(receiver.ingress.recovery_dropped_access_units, 1);
        assert!(receiver.latest_frame_store.latest().is_none());
    }

    #[test]
    fn matching_generation_preserves_access_unit_timeline() {
        let mut receiver = receiver_for_generation(2);

        receiver
            .publish_timeline_access_unit(access_unit(2, 9))
            .expect("matching generation");

        let frame = receiver.latest_frame_store.latest().expect("video frame");
        assert_eq!(frame.stream_generation, 2);
        assert_eq!(frame.frame_id, 9);
        assert_eq!(frame.source_pts_us, 42_000);
        assert_eq!(frame.received_at_us, 50_000);
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) enum FrameKind {
    Key,
    ReferenceDelta,
    DiscardableDelta,
}

impl FrameKind {
    fn is_keyframe(self) -> bool {
        self == Self::Key
    }
}

#[derive(Debug, Clone, Copy, Default)]
struct FrameTimeline {
    stream_generation: u64,
    frame_id: u64,
    source_pts_us: u64,
    received_at_us: u64,
    decoded_at: Option<Instant>,
}

impl ReceiverSession {
    pub(super) fn ingest_video_packet(&mut self, packet: VideoPacket) -> Result<(), ReceiverError> {
        // Enforce the wall-clock deadline before a queued late tail gets a
        // chance to complete an already-expired AU.
        self.expire_reassembly_deadline()?;
        self.ingress.packets_received += 1;
        if !self.video_allowed() {
            self.ingress.packets_dropped_unpaired += 1;
            return Ok(());
        }

        let packet_epoch = packet.stream_epoch;
        let configured_epoch = self
            .current_stream_config
            .as_ref()
            .map(|config| config.stream_epoch);
        let configured_epoch = match configured_epoch {
            Some(epoch) => epoch,
            // Explicit test/diagnostic bypass may carry arbitrary bytes and
            // intentionally has no protocol negotiation.
            None if self.permit_unpaired_video => packet_epoch,
            None => {
                // Control and Datagram channels can reorder. Never decode
                // product media before StreamConfig establishes codec/epoch.
                self.waiting_for_stream_config_epoch = Some(packet_epoch);
                return Ok(());
            }
        };
        if configured_epoch != packet_epoch {
            // Stale datagrams from an old epoch are expected after
            // reconfiguration. A future epoch waits for reliable StreamConfig;
            // that transition owns the single rate-limited fresh-IDR request.
            if packet_epoch > configured_epoch
                && self.waiting_for_stream_config_epoch != Some(packet_epoch)
            {
                self.waiting_for_stream_config_epoch = Some(packet_epoch);
            }
            return Ok(());
        }

        self.stats_reporter.record_packet(packet.payload.len());
        let recovered_before = self.reassembly.fec_recovered_fragment_count();
        let partial_drops_before = self.reassembly.partial_access_unit_drop_count();
        let gap_drops_before = self.reassembly.whole_access_unit_gap_drop_count();
        let reassembly_result = self.reassembly.ingest(packet);
        let recovered_now = self
            .reassembly
            .fec_recovered_fragment_count()
            .saturating_sub(recovered_before);
        self.ingress.fec_recovered_fragments = self
            .ingress
            .fec_recovered_fragments
            .saturating_add(recovered_now);
        self.ingress.reassembly_partial_access_unit_drops = self
            .ingress
            .reassembly_partial_access_unit_drops
            .saturating_add(
                self.reassembly
                    .partial_access_unit_drop_count()
                    .saturating_sub(partial_drops_before),
            );
        self.ingress.reassembly_whole_access_unit_gap_drops = self
            .ingress
            .reassembly_whole_access_unit_gap_drops
            .saturating_add(
                self.reassembly
                    .whole_access_unit_gap_drop_count()
                    .saturating_sub(gap_drops_before),
            );
        match reassembly_result {
            Ok(Some(access_unit)) => {
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
            }
            Ok(None) => {}
            // Reassembly owns drop/keyframe-loss accounting. Keep protocol
            // rejects out of the decoder and continue the session.
            Err(ReassemblyError::TooManyFragments)
            | Err(ReassemblyError::DuplicateFragment)
            | Err(ReassemblyError::EpochMismatch)
            | Err(ReassemblyError::InvalidFecParity) => {}
        }
        if self.reassembly.take_reference_chain_loss() {
            self.enter_decoder_recovery(RecoveryReason::ReferenceAccessUnitLost, true)?;
        }
        Ok(())
    }

    /// Attach a cross-process Shared Frame Ring for VCam consumption (REQ-PICOO-FRAME-003).
    pub fn attach_shared_ring(&mut self, name: &str) -> Result<(), ReceiverError> {
        #[cfg(target_os = "windows")]
        let ring = if name == DEFAULT_SHARED_RING_NAME {
            SharedFrameRingProducer::open_or_create_file(
                picoo_frame_hub::windows_shared_ring_path(name),
                picoo_frame_hub::DEFAULT_MAX_FRAME_BYTES,
            )?
        } else {
            SharedFrameRingProducer::open_or_create(name, picoo_frame_hub::DEFAULT_MAX_FRAME_BYTES)?
        };
        #[cfg(target_os = "macos")]
        let ring = if name == DEFAULT_SHARED_RING_NAME {
            let path = picoo_frame_hub::macos_app_group_ring_path(name)?;
            SharedFrameRingProducer::open_or_create_file(
                path,
                picoo_frame_hub::DEFAULT_MAX_FRAME_BYTES,
            )?
        } else {
            SharedFrameRingProducer::open_or_create(name, picoo_frame_hub::DEFAULT_MAX_FRAME_BYTES)?
        };
        #[cfg(not(any(target_os = "macos", target_os = "windows")))]
        let ring = SharedFrameRingProducer::open_or_create(
            name,
            picoo_frame_hub::DEFAULT_MAX_FRAME_BYTES,
        )?;
        self.shared_ring = Some(ring);
        self.publish_waiting_placeholder()?;
        Ok(())
    }

    pub fn publish_waiting_placeholder(&mut self) -> Result<(), ReceiverError> {
        let nv12 = self.placeholder_mode.waiting_frame();
        self.publish_decoded_frame(
            FrameTimeline::default(),
            DecodedFrame {
                width: PLACEHOLDER_WIDTH,
                height: PLACEHOLDER_HEIGHT,
                stride: PLACEHOLDER_WIDTH,
                rotation: 0,
                timestamp_us: 0,
                nv12: Bytes::from(nv12),
            },
        )
    }

    /// Publish reconnect-branded placeholder (REQ-PICOO-FRAME-005).
    pub fn publish_reconnecting_placeholder(&mut self) -> Result<(), ReceiverError> {
        let nv12 = self.placeholder_mode.reconnecting_frame();
        self.publish_decoded_frame(
            FrameTimeline::default(),
            DecodedFrame {
                width: PLACEHOLDER_WIDTH,
                height: PLACEHOLDER_HEIGHT,
                stride: PLACEHOLDER_WIDTH,
                rotation: 0,
                timestamp_us: 0,
                nv12: Bytes::from(nv12),
            },
        )
    }

    /// Prefer branded waiting frame (`true`) or solid black (`false`) — PRD §16.
    /// Prefer [`set_placeholder_mode`] for Logo / Black / Bars.
    pub fn set_use_default_placeholder(&mut self, enabled: bool) {
        self.placeholder_mode = if enabled {
            PlaceholderMode::Logo
        } else {
            PlaceholderMode::Black
        };
    }

    pub fn use_default_placeholder(&self) -> bool {
        matches!(self.placeholder_mode, PlaceholderMode::Logo)
    }

    pub fn set_placeholder_mode(&mut self, mode: PlaceholderMode) {
        self.placeholder_mode = mode;
    }

    pub fn placeholder_mode(&self) -> PlaceholderMode {
        self.placeholder_mode
    }

    /// Test-only decoder injection keeps synthetic payload support outside the
    /// production platform decoder.
    #[cfg(test)]
    pub fn set_decoder_for_test(&mut self, decoder: Box<dyn AccessUnitDecoder>) {
        self.decoder = decoder;
    }

    /// Test adapter for decoder recovery fixtures without a network timeline.
    #[cfg(test)]
    pub(crate) fn publish_access_unit(
        &mut self,
        access_unit: Bytes,
        keyframe: bool,
    ) -> Result<(), ReceiverError> {
        self.publish_timeline_access_unit(EncodedAccessUnit {
            stream_generation: self
                .current_stream_config
                .as_ref()
                .map_or(0, |config| u64::from(config.stream_epoch)),
            frame_id: 0,
            source_pts_us: 0,
            received_at_us: self.timing_origin.elapsed().as_micros() as u64,
            kind: if keyframe {
                FrameKind::Key
            } else {
                FrameKind::ReferenceDelta
            },
            data: access_unit,
        })
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
            // A future Decoder Worker can complete after a generation switch.
            // Keep this gate at the session boundary even though today's
            // synchronous adapter normally clears the jitter queue first.
            self.ingress.recovery_dropped_access_units =
                self.ingress.recovery_dropped_access_units.saturating_add(1);
            return Ok(());
        }
        if !self.accepts_access_unit_for_decode(access_unit.kind.is_keyframe()) {
            self.ingress.recovery_dropped_access_units =
                self.ingress.recovery_dropped_access_units.saturating_add(1);
            return Ok(());
        }
        self.ingress.decode_invocations += 1;
        let decode_started = Instant::now();
        let decode_result = self
            .decoder
            .decode_access_unit(&access_unit.data, self.current_stream_config.as_ref());
        let decoded_at = Instant::now();
        self.jitter
            .observe_decode_time_us(decode_started.elapsed().as_micros() as u64);
        let outcome = match decode_result {
            Ok(decoded) => decoded,
            Err(error) => {
                self.stats_reporter.record_decoder_drop();
                self.last_media_error = Some(error.to_string());
                tracing::warn!("H.264 access unit decode failed: {error}");
                self.enter_decoder_recovery(super::recovery::RecoveryReason::DecoderError, true)?;
                return Ok(());
            }
        };
        if access_unit.kind.is_keyframe() && outcome.refresh_accepted {
            self.mark_decoder_refresh_accepted();
        }
        match outcome.frame {
            Some(mut frame) => {
                // Prefer StreamConfig.rotation from Sender when present (PUC-005 / MEDIA-009).
                frame.rotation = self
                    .current_stream_config
                    .as_ref()
                    .map(|c| c.rotation)
                    .unwrap_or(frame.rotation);
                self.publish_decoded_frame(
                    FrameTimeline {
                        stream_generation: access_unit.stream_generation,
                        frame_id: access_unit.frame_id,
                        source_pts_us: access_unit.source_pts_us,
                        received_at_us: access_unit.received_at_us,
                        decoded_at: Some(decoded_at),
                    },
                    frame,
                )?;
                self.ingress.decoded_frames += 1;
                self.stats_reporter.record_decoded_frame();
                self.last_media_error = None;
            }
            None => {
                self.stats_reporter.record_decoder_drop();
            }
        }
        Ok(())
    }

    fn publish_decoded_frame(
        &mut self,
        timeline: FrameTimeline,
        frame: DecodedFrame,
    ) -> Result<(), ReceiverError> {
        let DecodedFrame {
            width,
            height,
            stride,
            rotation,
            timestamp_us,
            nv12,
        } = frame;
        // REQ-PICOO-MEDIA-009: rotate before LatestFrameStore / Shared Ring / VCam.
        // REQ-PICOO-MEDIA-004: then apply remote StreamConfig.mirrored in upright space.
        let rotated_buf =
            picoo_frame_hub::nv12_rotate_clockwise(width, height, stride, rotation, &nv12);
        let (width, height, stride, pixels) = match rotated_buf {
            Some((ow, oh, os, buf)) => (ow, oh, os, Bytes::from(buf)),
            None => (width, height, stride, nv12),
        };

        let mirrored = self
            .current_stream_config
            .as_ref()
            .is_some_and(|c| c.mirrored);
        let pixels = if mirrored {
            let mut buf = pixels.to_vec();
            picoo_frame_hub::nv12_mirror_horizontal(width, height, stride, &mut buf);
            Bytes::from(buf)
        } else {
            pixels
        };

        // Pixels are upright after rotation; clear metadata so VCam does not double-rotate.
        let published_rotation = 0u32;

        self.latest_frame_store.publish(VideoFrame::new(
            timeline.stream_generation,
            timeline.frame_id,
            timeline.source_pts_us,
            timeline.received_at_us,
            timeline.decoded_at.unwrap_or_else(Instant::now),
            timestamp_us,
            width,
            height,
            stride,
            published_rotation,
            pixels.clone(),
        ));
        if let Some(ring) = self.shared_ring.as_mut() {
            ring.publish_nv12(
                width,
                height,
                stride,
                published_rotation,
                timestamp_us,
                &pixels,
            )?;
        }
        Ok(())
    }

    pub fn latest_frame(&self) -> Option<&Arc<VideoFrame>> {
        self.latest_frame_store.latest()
    }
}
