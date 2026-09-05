//! Decoder prediction-chain recovery — REQ-PICOO-SESSION-010.

use std::time::{Duration, Instant};

use picoo_session::StreamState;

use super::ReceiverSession;
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

#[derive(Debug)]
pub(crate) struct DecoderRecovery {
    awaiting_refresh: bool,
    reason: Option<RecoveryReason>,
    last_request_at: Option<Instant>,
}

impl DecoderRecovery {
    pub(crate) fn new() -> Self {
        Self {
            awaiting_refresh: false,
            reason: None,
            last_request_at: None,
        }
    }

    fn enter(&mut self, reason: RecoveryReason) -> bool {
        let newly_awaiting = !self.awaiting_refresh;
        self.awaiting_refresh = true;
        self.reason = Some(reason);
        newly_awaiting
    }

    pub(crate) fn accepts(&self, keyframe: bool) -> bool {
        !self.awaiting_refresh || keyframe
    }

    fn request_due(&self, now: Instant) -> bool {
        self.awaiting_refresh
            && self.last_request_at.is_none_or(|last| {
                now.saturating_duration_since(last) >= KEYFRAME_REQUEST_MIN_INTERVAL
            })
    }

    pub(crate) fn next_request_at(&self, now: Instant) -> Option<Instant> {
        self.awaiting_refresh.then(|| {
            self.last_request_at
                .map_or(now, |last| last + KEYFRAME_REQUEST_MIN_INTERVAL)
        })
    }

    fn note_request(&mut self, now: Instant) {
        self.last_request_at = Some(now);
    }

    pub(crate) fn mark_recovered(&mut self) {
        self.awaiting_refresh = false;
        self.reason = None;
    }

    pub(crate) fn reset_session(&mut self) {
        self.awaiting_refresh = false;
        self.reason = None;
        self.last_request_at = None;
    }

    #[cfg(test)]
    pub(crate) fn awaiting_refresh(&self) -> bool {
        self.awaiting_refresh
    }
}

impl ReceiverSession {
    pub(crate) fn enter_decoder_recovery(
        &mut self,
        reason: RecoveryReason,
        reset_decoder: bool,
    ) -> Result<(), ReceiverError> {
        let newly_awaiting = self.decoder_recovery.enter(reason);
        if self.lifecycle.runtime.stream().is_streaming() {
            let generation = self
                .current_stream_config
                .as_ref()
                .map_or(0, |config| config.stream_epoch);
            self.lifecycle
                .runtime
                .set_stream(StreamState::AwaitingRefresh { generation });
        }
        if newly_awaiting {
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
            self.reassembly.clear_pending();
            let _ = self.reassembly.take_reference_chain_loss();
            // Decoder recovery discards dependent media but preserves the
            // current network/decode timing estimate for this stream epoch.
            self.jitter.discard_queued();
            tracing::warn!(reason = reason.label(), "decoder awaiting fresh IDR");
        }

        if reset_decoder && (newly_awaiting || reason == RecoveryReason::DecoderError) {
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

    pub(crate) fn mark_decoder_refresh_accepted(&mut self) {
        self.decoder_recovery.mark_recovered();
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

    pub(crate) fn accepts_access_unit_for_decode(&self, keyframe: bool) -> bool {
        self.decoder_recovery.accepts(keyframe)
    }

    pub(crate) fn force_decoder_recovery_request(
        &mut self,
        reason: RecoveryReason,
    ) -> Result<(), ReceiverError> {
        let newly_awaiting = self.decoder_recovery.enter(reason);
        if self.lifecycle.runtime.stream().is_streaming() {
            let generation = self
                .current_stream_config
                .as_ref()
                .map_or(0, |config| config.stream_epoch);
            self.lifecycle
                .runtime
                .set_stream(StreamState::AwaitingRefresh { generation });
        }
        if newly_awaiting {
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
        assert!(state.enter(RecoveryReason::ReferenceAccessUnitLost));
        assert!(state.request_due(start));
        state.note_request(start);
        assert!(!state.request_due(start + Duration::from_millis(999)));
        assert!(state.request_due(start + Duration::from_secs(1)));
    }

    #[test]
    fn awaiting_refresh_rejects_delta_until_keyframe_is_accepted() {
        let mut state = DecoderRecovery::new();
        state.enter(RecoveryReason::DecoderError);
        assert!(!state.accepts(false));
        assert!(state.accepts(true));
        state.mark_recovered();
        assert!(state.accepts(false));
    }

    #[test]
    fn recovery_does_not_reset_the_global_request_rate_limit() {
        let start = Instant::now();
        let mut state = DecoderRecovery::new();
        state.enter(RecoveryReason::DecoderError);
        state.note_request(start);
        state.mark_recovered();
        state.enter(RecoveryReason::DecoderError);

        assert!(!state.request_due(start + Duration::from_millis(100)));
        assert!(state.request_due(start + Duration::from_secs(1)));
    }
}
