//! Decoder prediction-chain recovery — REQ-PICOO-SESSION-010.

use std::time::{Duration, Instant};

use picoo_jitter::FrontFrameDescriptor;
use picoo_session::StreamState;

use super::decoder_worker::AccessUnitTimeline;
use super::ReceiverSession;
use crate::media_scheduler::RecoveryAdmission;
use crate::ReceiverError;

const KEYFRAME_REQUEST_MIN_INTERVAL: Duration = Duration::from_secs(1);

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum RecoveryReason {
    InitialConfig,
    EpochChanged,
    ReferenceAccessUnitLost,
    ReferenceAccessUnitLate,
    DecoderError,
    DecoderQueuePressure,
    ManualRepair,
}

impl RecoveryReason {
    fn label(self) -> &'static str {
        match self {
            Self::InitialConfig => "initial_config",
            Self::EpochChanged => "epoch_changed",
            Self::ReferenceAccessUnitLost => "reference_access_unit_lost",
            Self::ReferenceAccessUnitLate => "reference_access_unit_late",
            Self::DecoderError => "decoder_error",
            Self::DecoderQueuePressure => "decoder_queue_pressure",
            Self::ManualRepair => "manual_repair",
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
struct RefreshCandidate {
    connection_generation: u64,
    stream_generation: u64,
    frame_id: u64,
}

impl RefreshCandidate {
    fn from_timeline(timeline: AccessUnitTimeline) -> Self {
        Self {
            connection_generation: timeline.connection_generation,
            stream_generation: timeline.stream_generation,
            frame_id: timeline.frame_id,
        }
    }

    fn matches(self, timeline: AccessUnitTimeline) -> bool {
        self.connection_generation == timeline.connection_generation
            && self.stream_generation == timeline.stream_generation
            && self.frame_id == timeline.frame_id
    }
}

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
enum DecoderRecoveryPhase {
    #[default]
    Healthy,
    AwaitingRefresh,
    RefreshInFlight(RefreshCandidate),
}

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
struct RecoveryTransition {
    newly_awaiting: bool,
    invalidated_candidate: bool,
}

impl RecoveryTransition {
    fn requires_media_cleanup(self) -> bool {
        self.newly_awaiting || self.invalidated_candidate
    }
}

#[derive(Debug)]
pub(super) struct DecoderRecovery {
    phase: DecoderRecoveryPhase,
    reason: Option<RecoveryReason>,
    last_request_at: Option<Instant>,
}

impl DecoderRecovery {
    pub(super) fn new() -> Self {
        Self {
            phase: DecoderRecoveryPhase::Healthy,
            reason: None,
            last_request_at: None,
        }
    }

    fn enter(&mut self, reason: RecoveryReason) -> RecoveryTransition {
        let transition = RecoveryTransition {
            newly_awaiting: self.phase == DecoderRecoveryPhase::Healthy,
            invalidated_candidate: matches!(self.phase, DecoderRecoveryPhase::RefreshInFlight(_)),
        };
        self.phase = DecoderRecoveryPhase::AwaitingRefresh;
        self.reason = Some(reason);
        transition
    }

    pub(super) fn admission(
        &self,
        connection_generation: u64,
        frame: FrontFrameDescriptor,
    ) -> RecoveryAdmission {
        match self.phase {
            DecoderRecoveryPhase::Healthy => RecoveryAdmission::Ready,
            DecoderRecoveryPhase::AwaitingRefresh => {
                if frame.keyframe {
                    RecoveryAdmission::Ready
                } else {
                    RecoveryAdmission::Drop
                }
            }
            DecoderRecoveryPhase::RefreshInFlight(candidate) => {
                if frame.keyframe {
                    RecoveryAdmission::Ready
                } else if frame.discardable {
                    RecoveryAdmission::Drop
                } else if candidate.connection_generation == connection_generation
                    && candidate.stream_generation == frame.stream_generation
                    && frame.frame_id > candidate.frame_id
                {
                    RecoveryAdmission::WaitForRefresh
                } else {
                    RecoveryAdmission::Drop
                }
            }
        }
    }

    pub(super) fn accepts_completion(&self, timeline: AccessUnitTimeline) -> bool {
        match self.phase {
            DecoderRecoveryPhase::Healthy => true,
            DecoderRecoveryPhase::AwaitingRefresh => false,
            DecoderRecoveryPhase::RefreshInFlight(candidate) => candidate.matches(timeline),
        }
    }

    pub(super) fn is_refresh_candidate(&self, timeline: AccessUnitTimeline) -> bool {
        matches!(
            self.phase,
            DecoderRecoveryPhase::RefreshInFlight(candidate) if candidate.matches(timeline)
        )
    }

    pub(super) fn note_refresh_submitted(&mut self, timeline: AccessUnitTimeline) {
        if timeline.kind.is_keyframe() && self.phase != DecoderRecoveryPhase::Healthy {
            self.phase =
                DecoderRecoveryPhase::RefreshInFlight(RefreshCandidate::from_timeline(timeline));
        }
    }

    fn request_due(&self, now: Instant) -> bool {
        self.is_awaiting_refresh()
            && self.last_request_at.is_none_or(|last| {
                now.saturating_duration_since(last) >= KEYFRAME_REQUEST_MIN_INTERVAL
            })
    }

    pub(super) fn next_request_at(&self, now: Instant) -> Option<Instant> {
        self.is_awaiting_refresh().then(|| {
            self.last_request_at
                .map_or(now, |last| last + KEYFRAME_REQUEST_MIN_INTERVAL)
        })
    }

    fn note_request(&mut self, now: Instant) {
        self.last_request_at = Some(now);
    }

    pub(super) fn mark_recovered(&mut self, timeline: AccessUnitTimeline) -> bool {
        if !matches!(
            self.phase,
            DecoderRecoveryPhase::RefreshInFlight(candidate) if candidate.matches(timeline)
        ) {
            return false;
        }
        self.phase = DecoderRecoveryPhase::Healthy;
        self.reason = None;
        true
    }

    pub(super) fn reset_session(&mut self) {
        self.phase = DecoderRecoveryPhase::Healthy;
        self.reason = None;
        self.last_request_at = None;
    }

    fn is_awaiting_refresh(&self) -> bool {
        self.phase != DecoderRecoveryPhase::Healthy
    }

    #[cfg(test)]
    pub(super) fn awaiting_refresh(&self) -> bool {
        self.is_awaiting_refresh()
    }

    #[cfg(test)]
    fn refresh_in_flight(&self) -> bool {
        matches!(self.phase, DecoderRecoveryPhase::RefreshInFlight(_))
    }
}

impl ReceiverSession {
    pub(crate) fn enter_decoder_recovery(
        &mut self,
        reason: RecoveryReason,
        reset_decoder: bool,
    ) -> Result<(), ReceiverError> {
        let transition = self.decoder_recovery.enter(reason);
        if self.lifecycle.runtime.stream().is_streaming() {
            let generation = self
                .current_stream_config
                .as_ref()
                .map_or(0, |config| config.stream_epoch);
            self.lifecycle
                .runtime
                .set_stream(StreamState::AwaitingRefresh { generation });
        }
        if transition.newly_awaiting {
            match reason {
                RecoveryReason::ReferenceAccessUnitLost => {
                    self.ingress.recovery_reference_lost =
                        self.ingress.recovery_reference_lost.saturating_add(1);
                }
                RecoveryReason::ReferenceAccessUnitLate => {
                    self.ingress.recovery_reference_late =
                        self.ingress.recovery_reference_late.saturating_add(1);
                }
                RecoveryReason::DecoderError => {
                    self.ingress.recovery_decoder_errors =
                        self.ingress.recovery_decoder_errors.saturating_add(1);
                }
                RecoveryReason::DecoderQueuePressure => {}
                RecoveryReason::InitialConfig
                | RecoveryReason::EpochChanged
                | RecoveryReason::ManualRepair => {}
            }
            tracing::warn!(reason = reason.label(), "decoder awaiting fresh IDR");
        }

        if transition.requires_media_cleanup() {
            self.reassembly.clear_pending();
            let _ = self.reassembly.take_reference_chain_loss();
            // Decoder recovery discards dependent media but preserves the
            // current network/decode timing estimate for this stream epoch.
            self.jitter.discard_queued();
        }

        if reset_decoder
            && (transition.requires_media_cleanup() || reason == RecoveryReason::DecoderError)
        {
            self.ingress.decoder_resets = self.ingress.decoder_resets.saturating_add(1);
            self.decoder_worker.reset();
        }

        self.maybe_request_recovery_keyframe()
    }

    pub(crate) fn maybe_request_recovery_keyframe(&mut self) -> Result<(), ReceiverError> {
        let now = Instant::now();
        if !self.decoder_recovery.request_due(now) {
            return Ok(());
        }
        let Some(session) = self.transport.active_session() else {
            return Ok(());
        };
        self.send_request_keyframe_now(session)?;
        self.decoder_recovery.note_request(now);
        Ok(())
    }

    pub(super) fn mark_decoder_refresh_accepted(&mut self, timeline: AccessUnitTimeline) {
        if !self.decoder_recovery.mark_recovered(timeline) {
            return;
        }
        if matches!(
            self.lifecycle.runtime.stream(),
            StreamState::AwaitingRefresh { .. }
        ) {
            let generation = self
                .current_stream_config
                .as_ref()
                .map_or(0, |config| config.stream_epoch);
            self.lifecycle
                .runtime
                .set_stream(StreamState::Streaming { generation });
        }
    }

    pub(crate) fn force_decoder_recovery_request(
        &mut self,
        reason: RecoveryReason,
    ) -> Result<(), ReceiverError> {
        let transition = self.decoder_recovery.enter(reason);
        if self.lifecycle.runtime.stream().is_streaming() {
            let generation = self
                .current_stream_config
                .as_ref()
                .map_or(0, |config| config.stream_epoch);
            self.lifecycle
                .runtime
                .set_stream(StreamState::AwaitingRefresh { generation });
        }
        if transition.requires_media_cleanup() {
            self.reassembly.clear_pending();
            let _ = self.reassembly.take_reference_chain_loss();
            self.jitter.discard_queued();
        }
        let session = self
            .transport
            .active_session()
            .ok_or(ReceiverError::NotListening)?;
        self.send_request_keyframe_now(session)?;
        self.decoder_recovery.note_request(Instant::now());
        Ok(())
    }

    #[cfg(test)]
    pub(crate) fn awaiting_decoder_refresh_for_test(&self) -> bool {
        self.decoder_recovery.awaiting_refresh()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn request_window_coalesces_automatic_keyframe_requests() {
        let start = Instant::now();
        let mut state = DecoderRecovery::new();
        assert!(
            state
                .enter(RecoveryReason::ReferenceAccessUnitLost)
                .newly_awaiting
        );
        assert!(state.request_due(start));
        state.note_request(start);
        assert!(!state.request_due(start + Duration::from_millis(999)));
        assert!(state.request_due(start + Duration::from_secs(1)));
    }

    #[test]
    fn awaiting_refresh_holds_following_reference_until_matching_keyframe_completion() {
        let mut state = DecoderRecovery::new();
        state.enter(RecoveryReason::DecoderError);
        let key = timeline(1, 2, 100, true);
        assert_eq!(
            state.admission(1, descriptor(2, 99, false, false)),
            RecoveryAdmission::Drop
        );
        assert_eq!(
            state.admission(1, descriptor(2, 100, true, false)),
            RecoveryAdmission::Ready
        );
        state.note_refresh_submitted(key);
        assert_eq!(
            state.admission(1, descriptor(2, 101, false, false)),
            RecoveryAdmission::WaitForRefresh
        );
        assert!(!state.mark_recovered(timeline(1, 2, 99, true)));
        assert!(state.mark_recovered(key));
        assert_eq!(
            state.admission(1, descriptor(2, 101, false, false)),
            RecoveryAdmission::Ready
        );
    }

    #[test]
    fn recovery_does_not_reset_the_global_request_rate_limit() {
        let start = Instant::now();
        let mut state = DecoderRecovery::new();
        state.enter(RecoveryReason::DecoderError);
        state.note_request(start);
        let key = timeline(1, 2, 100, true);
        state.note_refresh_submitted(key);
        assert!(state.mark_recovered(key));
        state.enter(RecoveryReason::DecoderError);

        assert!(!state.request_due(start + Duration::from_millis(100)));
        assert!(state.request_due(start + Duration::from_secs(1)));
    }

    #[test]
    fn reference_loss_invalidates_an_in_flight_candidate() {
        let mut state = DecoderRecovery::new();
        state.enter(RecoveryReason::DecoderError);
        let key = timeline(1, 2, 100, true);
        state.note_refresh_submitted(key);

        let transition = state.enter(RecoveryReason::ReferenceAccessUnitLost);

        assert!(transition.invalidated_candidate);
        assert!(!state.refresh_in_flight());
        assert!(!state.mark_recovered(key));
        assert!(state.awaiting_refresh());
    }

    fn timeline(
        connection_generation: u64,
        stream_generation: u64,
        frame_id: u64,
        keyframe: bool,
    ) -> AccessUnitTimeline {
        AccessUnitTimeline {
            connection_generation,
            stream_generation,
            frame_id,
            source_pts_us: 0,
            encoded_at_us: 0,
            received_at_us: 0,
            decode_submitted_at_us: 0,
            kind: if keyframe {
                super::super::decoder_worker::FrameKind::Key
            } else {
                super::super::decoder_worker::FrameKind::ReferenceDelta
            },
        }
    }

    fn descriptor(
        stream_generation: u64,
        frame_id: u64,
        keyframe: bool,
        discardable: bool,
    ) -> FrontFrameDescriptor {
        FrontFrameDescriptor {
            stream_generation,
            frame_id,
            keyframe,
            discardable,
        }
    }
}
