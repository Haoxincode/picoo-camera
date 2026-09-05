//! Production scheduler simulation — REQ-PICOO-STACK-009/011.

use std::sync::Arc;
use std::time::Duration;

use bytes::Bytes;
use picoo_frame_hub::{nv12_byte_size, LatestFrameStore, VideoFrame};
use picoo_jitter::{Frame as JitterFrame, JitterBuffer, PushOutcome};
use picoo_packet::{AssembledAccessUnit, ReassemblyMap};
use picoo_protocol::control::{control_envelope::Payload as ControlPayload, StreamConfig};
use picoo_protocol::{decode_control_envelope, encode_control_envelope};
use picoo_receiver::media_scheduler::{
    schedule_media, DecoderAdmission, MediaScheduleDecision, MediaScheduleInput,
};
use picoo_sender::{FecProtection, SenderPipeline};

use crate::encoder::{
    CameraFrame, EncoderCommit, EncoderConfig, EncoderFailure, ScriptedEncoder, SimError,
};
use crate::sim_decoder::{SimDecodeJob, SimulatedDecoder};
use crate::{NetworkScript, SimDelivery, SimulatedNetwork, VirtualClock};

const REASSEMBLY_CAPACITY: usize = 8;
const REASSEMBLY_MAX_FRAGMENTS: u16 = 1_024;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SimTimingMode {
    Fast,
    ProductionEquivalent { decoder_latency: Duration },
}

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct PipelineCounters {
    pub captured: u64,
    pub camera_suspended_drops: u64,
    pub encoded: u64,
    pub unauthenticated_media_drops: u64,
    pub illegal_control_drops: u64,
    pub replayed_control_drops: u64,
    pub stale_generation_drops: u64,
    pub incomplete_access_unit_drops: u64,
    pub pre_refresh_delta_drops: u64,
    pub decoded: u64,
    pub duplicate_decode_attempts: u64,
    pub decoder_discardable_drops: u64,
    pub privileged_controls: u64,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct SimSnapshot {
    pub connection_generation: Option<u64>,
    pub authenticated: bool,
    pub streaming: bool,
    pub configured_stream_epoch: Option<u32>,
    pub committed_encoder_epoch: u32,
    pub committed_encoder_generation: u64,
    pub waiting_for_idr: bool,
    pub reference_chain_intact: bool,
    pub network_in_flight: usize,
    pub jitter_depth: usize,
    pub decoder_pending_depth: usize,
    pub decoder_active: bool,
    pub latest_sequence: u64,
    pub counters: PipelineCounters,
}

struct ReceiverCore {
    active_generation: Option<u64>,
    authenticated: bool,
    streaming: bool,
    last_control_message_id: u64,
    stream_config: Option<StreamConfig>,
    waiting_for_idr: bool,
    reference_chain_intact: bool,
    pending_future_idr: Option<AssembledAccessUnit>,
    reassembly: ReassemblyMap,
    jitter: JitterBuffer,
    decoder: SimulatedDecoder,
    last_decoded: Option<(u64, u32, u64)>,
    latest: LatestFrameStore,
    counters: PipelineCounters,
}

impl ReceiverCore {
    fn new(timing: SimTimingMode) -> Self {
        let mut jitter = JitterBuffer::new();
        let decoder_latency = match timing {
            SimTimingMode::Fast => {
                jitter.set_fixed_target_ms(Some(0));
                Duration::ZERO
            }
            SimTimingMode::ProductionEquivalent { decoder_latency } => decoder_latency,
        };
        Self {
            active_generation: None,
            authenticated: false,
            streaming: false,
            last_control_message_id: 0,
            stream_config: None,
            waiting_for_idr: false,
            reference_chain_intact: true,
            pending_future_idr: None,
            reassembly: ReassemblyMap::new(REASSEMBLY_CAPACITY, REASSEMBLY_MAX_FRAGMENTS),
            jitter,
            decoder: SimulatedDecoder::new(decoder_latency),
            last_decoded: None,
            latest: LatestFrameStore::new(),
            counters: PipelineCounters::default(),
        }
    }

    fn connect(&mut self, generation: u64) {
        self.active_generation = Some(generation);
        self.authenticated = false;
        self.streaming = false;
        self.last_control_message_id = 0;
        self.stream_config = None;
        self.waiting_for_idr = false;
        self.reference_chain_intact = true;
        self.pending_future_idr = None;
        self.reassembly = ReassemblyMap::new(REASSEMBLY_CAPACITY, REASSEMBLY_MAX_FRAGMENTS);
        self.jitter.clear();
        self.decoder.reset();
        self.last_decoded = None;
    }

    fn authenticate(&mut self) {
        if self.active_generation.is_some() {
            self.authenticated = true;
        }
    }

    fn receive(&mut self, now: &VirtualClock, delivery: SimDelivery) {
        match delivery {
            SimDelivery::Control {
                connection_generation,
                bytes,
            } => self.receive_control(now, connection_generation, &bytes),
            SimDelivery::Video {
                connection_generation,
                packet,
            } => self.receive_video(now, connection_generation, packet),
            SimDelivery::Disconnected {
                connection_generation,
            } => {
                if self.active_generation == Some(connection_generation) {
                    self.active_generation = None;
                    self.authenticated = false;
                    self.streaming = false;
                    self.stream_config = None;
                    self.pending_future_idr = None;
                    self.reassembly =
                        ReassemblyMap::new(REASSEMBLY_CAPACITY, REASSEMBLY_MAX_FRAGMENTS);
                    self.jitter.clear();
                    self.decoder.reset();
                    self.last_decoded = None;
                } else {
                    self.counters.stale_generation_drops =
                        self.counters.stale_generation_drops.saturating_add(1);
                }
            }
        }
    }

    fn receive_control(&mut self, now: &VirtualClock, generation: u64, bytes: &[u8]) {
        if self.active_generation != Some(generation) {
            self.counters.stale_generation_drops =
                self.counters.stale_generation_drops.saturating_add(1);
            return;
        }
        let Ok(envelope) = decode_control_envelope(bytes) else {
            self.counters.illegal_control_drops =
                self.counters.illegal_control_drops.saturating_add(1);
            return;
        };
        if envelope.connection_generation != generation {
            self.counters.stale_generation_drops =
                self.counters.stale_generation_drops.saturating_add(1);
            return;
        }
        if envelope.message_id <= self.last_control_message_id {
            self.counters.replayed_control_drops =
                self.counters.replayed_control_drops.saturating_add(1);
            return;
        }
        self.last_control_message_id = envelope.message_id;
        let Some(payload) = envelope.payload else {
            self.counters.illegal_control_drops =
                self.counters.illegal_control_drops.saturating_add(1);
            return;
        };
        if !self.authenticated {
            self.counters.illegal_control_drops =
                self.counters.illegal_control_drops.saturating_add(1);
            return;
        }
        match payload {
            ControlPayload::StreamConfig(config) => {
                let previous_epoch = self
                    .stream_config
                    .as_ref()
                    .map(|current| current.stream_epoch);
                if previous_epoch.is_some_and(|epoch| config.stream_epoch < epoch) {
                    return;
                }
                let epoch = config.stream_epoch;
                self.stream_config = Some(config);
                self.streaming = true;
                if previous_epoch != Some(epoch) {
                    self.waiting_for_idr = true;
                    self.reference_chain_intact = false;
                    self.jitter.discard_queued();
                    self.decoder.reset();
                }
                if self
                    .pending_future_idr
                    .as_ref()
                    .is_some_and(|frame| frame.stream_epoch == epoch)
                {
                    if let Some(frame) = self.pending_future_idr.take() {
                        self.accept_access_unit(now, frame);
                    }
                } else {
                    self.pending_future_idr = None;
                }
            }
            ControlPayload::StartStream(_) if !self.streaming => {
                self.streaming = true;
            }
            ControlPayload::StopStream(_) if self.streaming => {
                self.streaming = false;
                self.stream_config = None;
                self.pending_future_idr = None;
                self.reassembly.clear_pending();
                self.jitter.clear();
                self.decoder.reset();
            }
            _ => {
                self.counters.illegal_control_drops =
                    self.counters.illegal_control_drops.saturating_add(1);
            }
        }
    }

    fn receive_video(
        &mut self,
        now: &VirtualClock,
        generation: u64,
        packet: picoo_protocol::VideoPacket,
    ) {
        if self.active_generation != Some(generation) {
            self.counters.stale_generation_drops =
                self.counters.stale_generation_drops.saturating_add(1);
            return;
        }
        if !self.authenticated || !self.streaming {
            self.counters.unauthenticated_media_drops =
                self.counters.unauthenticated_media_drops.saturating_add(1);
            return;
        }
        match self.reassembly.ingest_at(packet, now.instant()) {
            Ok(Some(frame)) => self.route_access_unit(now, frame),
            Ok(None) => {}
            Err(_) => {
                self.counters.incomplete_access_unit_drops =
                    self.counters.incomplete_access_unit_drops.saturating_add(1);
            }
        }
    }

    fn route_access_unit(&mut self, now: &VirtualClock, frame: AssembledAccessUnit) {
        let configured_epoch = self
            .stream_config
            .as_ref()
            .map(|config| config.stream_epoch);
        match configured_epoch {
            Some(epoch) if frame.stream_epoch == epoch => self.accept_access_unit(now, frame),
            Some(epoch) if frame.stream_epoch > epoch && frame.keyframe => {
                let replace = self
                    .pending_future_idr
                    .as_ref()
                    .is_none_or(|pending| frame.stream_epoch >= pending.stream_epoch);
                if replace {
                    self.pending_future_idr = Some(frame);
                }
            }
            None if frame.keyframe => self.pending_future_idr = Some(frame),
            _ => {
                self.counters.pre_refresh_delta_drops =
                    self.counters.pre_refresh_delta_drops.saturating_add(1);
            }
        }
    }

    fn accept_access_unit(&mut self, now: &VirtualClock, frame: AssembledAccessUnit) {
        if (self.waiting_for_idr || !self.reference_chain_intact) && !frame.keyframe {
            self.counters.pre_refresh_delta_drops =
                self.counters.pre_refresh_delta_drops.saturating_add(1);
            return;
        }
        if frame.keyframe {
            self.waiting_for_idr = false;
            self.reference_chain_intact = true;
        }
        let first_fragment_at_us = now.micros_since_origin(frame.first_fragment_at);
        let jitter_frame = JitterFrame {
            stream_generation: u64::from(frame.stream_epoch),
            frame_id: frame.frame_id,
            pts_us: frame.pts_us,
            encoded_at_us: frame.encoded_at_us,
            received_at_us: now.now_us(),
            data: frame.data,
            keyframe: frame.keyframe,
            discardable: frame.discardable,
        };
        match self
            .jitter
            .push_at(jitter_frame, first_fragment_at_us, now.now_us())
        {
            PushOutcome::Accepted => {}
            PushOutcome::AcceptedAfterReferenceDrop
            | PushOutcome::DroppedLate {
                requires_refresh: true,
            } => {
                self.reference_chain_intact = false;
                self.waiting_for_idr = true;
            }
            PushOutcome::DroppedLate {
                requires_refresh: false,
            } => {}
        }
        self.drive_media(now);
    }

    fn drive_media(&mut self, now: &VirtualClock) {
        loop {
            let completed = self.drain_decoder(now);
            let advanced = self.drain_jitter(now);
            if !completed && !advanced {
                break;
            }
        }
    }

    fn drain_jitter(&mut self, now: &VirtualClock) -> bool {
        let mut advanced = false;
        loop {
            let max_queue_age_us = 300_000;
            let decoder_admission = self
                .jitter
                .front_frame_flags()
                .map_or(DecoderAdmission::Ready, |(keyframe, discardable)| {
                    self.decoder.admission(keyframe, discardable)
                });
            let decision = schedule_media(MediaScheduleInput {
                front_frame_id: self.jitter.front_frame_id(),
                oldest_unresolved_frame_id: self.reassembly.oldest_unresolved_frame_id(),
                release_delay: self
                    .jitter
                    .next_release_delay_us(now.now_us())
                    .map(Duration::from_micros),
                expiration_delay: self
                    .jitter
                    .next_expiration_delay_us(now.now_us(), max_queue_age_us)
                    .map(Duration::from_micros),
                decoder_admission,
            });
            match decision {
                MediaScheduleDecision::DiscardExpired => {
                    if self.jitter.drop_expired(now.now_us(), max_queue_age_us) {
                        self.reference_chain_intact = false;
                        self.waiting_for_idr = true;
                        self.jitter.discard_queued();
                        self.decoder.reset();
                    }
                    advanced = true;
                    continue;
                }
                MediaScheduleDecision::DiscardReadyFrame => {
                    let _ = self.jitter.pop_ready(now.now_us());
                    self.counters.decoder_discardable_drops =
                        self.counters.decoder_discardable_drops.saturating_add(1);
                    advanced = true;
                    continue;
                }
                MediaScheduleDecision::DecodeReadyFrame => {}
                MediaScheduleDecision::WaitUntil { .. }
                | MediaScheduleDecision::WaitForEvent(_)
                | MediaScheduleDecision::Idle => break,
            }
            let Some(frame) = self.jitter.pop_ready(now.now_us()) else {
                break;
            };
            let Some(config) = self.stream_config.as_ref() else {
                continue;
            };
            let key = (
                self.active_generation.unwrap_or_default(),
                config.stream_epoch,
                frame.frame_id,
            );
            if self
                .last_decoded
                .is_some_and(|last| last.0 == key.0 && last.1 == key.1 && key.2 <= last.2)
            {
                self.counters.duplicate_decode_attempts =
                    self.counters.duplicate_decode_attempts.saturating_add(1);
                continue;
            }
            self.last_decoded = Some(key);
            self.decoder.submit(
                SimDecodeJob {
                    connection_generation: key.0,
                    frame,
                    width: config.width.max(2) & !1,
                    height: config.height.max(2) & !1,
                    rotation: config.rotation,
                },
                now.now_us(),
            );
            advanced = true;
        }
        advanced
    }

    fn drain_decoder(&mut self, now: &VirtualClock) -> bool {
        let mut completed_any = false;
        while let Some((job, decode_submitted_at_us, decode_time_us)) =
            self.decoder.take_completed(now.now_us())
        {
            completed_any = true;
            self.jitter.observe_decode_time_us(decode_time_us);
            if self.active_generation != Some(job.connection_generation)
                || self.stream_config.as_ref().is_none_or(|config| {
                    u64::from(config.stream_epoch) != job.frame.stream_generation
                })
            {
                continue;
            }
            let pixel_len = nv12_byte_size(job.width, job.height);
            let marker = job.frame.data.first().copied().unwrap_or(0);
            self.latest.publish(VideoFrame::new(
                job.frame.stream_generation,
                job.frame.frame_id,
                job.frame.pts_us,
                job.frame.encoded_at_us,
                job.frame.received_at_us,
                decode_submitted_at_us,
                now.instant(),
                now.now_us(),
                job.width,
                job.height,
                job.width,
                job.rotation,
                Bytes::from(vec![marker; pixel_len]),
            ));
            self.counters.decoded = self.counters.decoded.saturating_add(1);
        }
        completed_any
    }

    fn expire_reassembly(&mut self, now: &VirtualClock, max_age: Duration) {
        let before = self.reassembly.drop_count();
        self.reassembly
            .expire_incomplete_older_than(now.instant(), max_age);
        self.counters.incomplete_access_unit_drops = self
            .counters
            .incomplete_access_unit_drops
            .saturating_add(self.reassembly.drop_count().saturating_sub(before));
        if self.reassembly.take_reference_chain_loss() {
            self.reference_chain_intact = false;
            self.waiting_for_idr = true;
            self.jitter.discard_queued();
            self.decoder.reset();
        }
        self.drive_media(now);
    }
}

/// End-to-end deterministic contract harness.
pub struct SimHarness {
    clock: VirtualClock,
    timing: SimTimingMode,
    camera_active: bool,
    encoder: ScriptedEncoder,
    sender: SenderPipeline,
    network: SimulatedNetwork,
    receiver: ReceiverCore,
    next_control_message_id: u64,
    preview_last_sequence: u64,
    vcam_last_sequence: u64,
}

impl SimHarness {
    pub fn new(script: NetworkScript) -> Self {
        Self::with_timing(script, SimTimingMode::Fast)
    }

    pub fn new_production_equivalent(script: NetworkScript, decoder_latency: Duration) -> Self {
        Self::with_timing(
            script,
            SimTimingMode::ProductionEquivalent { decoder_latency },
        )
    }

    fn with_timing(script: NetworkScript, timing: SimTimingMode) -> Self {
        Self {
            clock: VirtualClock::new(),
            timing,
            camera_active: true,
            encoder: ScriptedEncoder::new(),
            sender: SenderPipeline::default(),
            network: SimulatedNetwork::new(script),
            receiver: ReceiverCore::new(timing),
            next_control_message_id: 1,
            preview_last_sequence: 0,
            vcam_last_sequence: 0,
        }
    }

    pub fn clock(&self) -> &VirtualClock {
        &self.clock
    }

    pub fn network_mut(&mut self) -> &mut SimulatedNetwork {
        &mut self.network
    }

    pub fn connect(&mut self, generation: u64) {
        self.receiver.connect(generation);
        self.next_control_message_id = 1;
    }

    pub fn restart_receiver(&mut self, generation: u64) {
        self.receiver = ReceiverCore::new(self.timing);
        self.receiver.connect(generation);
        self.next_control_message_id = 1;
        self.preview_last_sequence = 0;
        self.vcam_last_sequence = 0;
    }

    pub fn authenticate(&mut self) {
        self.receiver.authenticate();
    }

    pub fn suspend_camera(&mut self) {
        self.camera_active = false;
    }

    pub fn resume_camera(&mut self) {
        self.camera_active = true;
    }

    pub fn begin_encoder_reconfiguration(
        &mut self,
        transaction_id: u64,
        stream_epoch: u32,
        encoder_generation: u64,
        width: u32,
        height: u32,
    ) -> bool {
        self.encoder.begin(
            transaction_id,
            EncoderConfig {
                stream_epoch,
                encoder_generation,
                width,
                height,
            },
        )
    }

    pub fn report_encoder_started(&mut self, transaction_id: u64, generation: u64) -> bool {
        self.encoder.report_started(transaction_id, generation)
    }

    pub fn report_encoder_failed(
        &mut self,
        transaction_id: u64,
        generation: u64,
    ) -> EncoderFailure {
        self.encoder.fail(transaction_id, generation)
    }

    pub fn take_encoder_commit(&mut self) -> Option<EncoderCommit> {
        self.encoder.last_commit.take()
    }

    pub fn queue_control(
        &mut self,
        payload: ControlPayload,
        extra_delay: Duration,
        duplicate: bool,
    ) -> u64 {
        let generation = self.receiver.active_generation.unwrap_or(1);
        let message_id = self.next_control_message_id;
        self.next_control_message_id = self.next_control_message_id.saturating_add(1);
        self.queue_control_with_identity(payload, message_id, generation, extra_delay, duplicate);
        message_id
    }

    pub fn queue_control_with_identity(
        &mut self,
        payload: ControlPayload,
        message_id: u64,
        generation: u64,
        extra_delay: Duration,
        duplicate: bool,
    ) {
        let bytes = encode_control_envelope(payload, message_id, generation);
        self.network.send_control(
            self.clock.now_us(),
            generation,
            bytes,
            extra_delay.as_micros().min(u128::from(u64::MAX)) as u64,
            duplicate,
        );
    }

    pub fn queue_start_stream(&mut self) {
        self.queue_control(
            ControlPayload::StartStream(picoo_protocol::control::StartStream {}),
            Duration::ZERO,
            false,
        );
    }

    /// Receiver-originated privileged control is permitted only for an
    /// authenticated live session. No control is emitted on rejection.
    pub fn issue_camera_command(&mut self) -> bool {
        if !self.receiver.authenticated || !self.receiver.streaming {
            return false;
        }
        self.receiver.counters.privileged_controls =
            self.receiver.counters.privileged_controls.saturating_add(1);
        true
    }

    pub fn queue_stream_config(
        &mut self,
        stream_epoch: u32,
        width: u32,
        height: u32,
        extra_delay: Duration,
    ) {
        self.queue_control(
            ControlPayload::StreamConfig(StreamConfig {
                codec: "h264".into(),
                profile: "baseline".into(),
                level: "3.1".into(),
                width,
                height,
                fps: 30,
                bitrate: 1_000_000,
                rotation: 0,
                mirrored: false,
                color_range: "limited".into(),
                sps: vec![1],
                pps: vec![2],
                stream_epoch,
            }),
            extra_delay,
            false,
        );
    }

    pub fn submit_camera_frame(
        &mut self,
        frame: CameraFrame,
        fec: FecProtection,
    ) -> Result<bool, SimError> {
        self.receiver.counters.captured = self.receiver.counters.captured.saturating_add(1);
        if !self.camera_active {
            self.receiver.counters.camera_suspended_drops = self
                .receiver
                .counters
                .camera_suspended_drops
                .saturating_add(1);
            return Ok(false);
        }
        self.encoder.accept(&frame)?;
        self.sender
            .ingest_timed_access_unit(
                &frame.data,
                frame.keyframe,
                frame.pts_us,
                frame.encoded_at_us,
                frame.stream_epoch,
                fec,
            )
            .map_err(|error| SimError::Packetization(error.to_string()))?;
        let generation = self.receiver.active_generation.unwrap_or(1);
        for batch in self.sender.take_pending_batches() {
            self.network
                .send_video_batch(self.clock.now_us(), generation, batch);
        }
        self.receiver.counters.encoded = self.receiver.counters.encoded.saturating_add(1);
        Ok(true)
    }

    pub fn disconnect(&mut self, generation: u64) {
        self.network.disconnect(self.clock.now_us(), generation);
    }

    pub fn advance(&mut self, duration: Duration) {
        self.clock.advance(duration);
        self.run_ready();
    }

    pub fn run_ready(&mut self) {
        let deliveries = self.network.drain_ready(self.clock.now_us());
        for delivery in deliveries {
            self.receiver.receive(&self.clock, delivery);
        }
        self.receiver.drive_media(&self.clock);
    }

    pub fn expire_reassembly(&mut self, max_age: Duration) {
        self.receiver.expire_reassembly(&self.clock, max_age);
    }

    pub fn consume_preview_latest(&mut self) -> Option<Arc<VideoFrame>> {
        let frame = Arc::clone(self.receiver.latest.latest()?);
        self.preview_last_sequence = frame.sequence;
        Some(frame)
    }

    /// Simulate one VCam request. It always has a valid negotiated-size output:
    /// the latest frame when present, otherwise a generated placeholder.
    pub fn consume_virtual_camera(&mut self, width: u32, height: u32) -> (u64, usize) {
        let sequence = self
            .receiver
            .latest
            .latest()
            .map_or(self.vcam_last_sequence, |frame| frame.sequence);
        self.vcam_last_sequence = sequence;
        (sequence, nv12_byte_size(width, height))
    }

    pub fn snapshot(&self) -> SimSnapshot {
        SimSnapshot {
            connection_generation: self.receiver.active_generation,
            authenticated: self.receiver.authenticated,
            streaming: self.receiver.streaming,
            configured_stream_epoch: self
                .receiver
                .stream_config
                .as_ref()
                .map(|config| config.stream_epoch),
            committed_encoder_epoch: self.encoder.committed.stream_epoch,
            committed_encoder_generation: self.encoder.committed.encoder_generation,
            waiting_for_idr: self.receiver.waiting_for_idr,
            reference_chain_intact: self.receiver.reference_chain_intact,
            network_in_flight: self.network.in_flight(),
            jitter_depth: self.receiver.jitter.len(),
            decoder_pending_depth: self.receiver.decoder.pending_depth(),
            decoder_active: self.receiver.decoder.is_active(),
            latest_sequence: self.receiver.latest.latest_sequence(),
            counters: self.receiver.counters,
        }
    }
}
