//! Rust-owned encoder apply transaction state — REQ-PICOO-MEDIA-003/010/016.

use std::time::{Duration, Instant};

use crate::stream_config::StreamConfigParams;

use super::{EncoderDirective, EncoderDirectiveKind};

pub(super) const ENCODER_APPLY_TIMEOUT: Duration = Duration::from_secs(3);

#[derive(Debug, Clone)]
pub(super) struct EncoderRollback {
    pub(super) stream_config: Option<StreamConfigParams>,
    pub(super) stream_config_sent: bool,
    pub(super) encoder_generation: u64,
}

#[derive(Debug, Clone)]
pub(super) struct EncoderTransaction {
    pub(super) directive: EncoderDirective,
    pub(super) rollback: EncoderRollback,
    pub(super) stream_config_staged: bool,
    pub(super) expected_generation: Option<u64>,
    pub(super) deadline: Instant,
}

#[derive(Debug, Clone, Default)]
pub(super) enum EncoderApplyState {
    #[default]
    Committed,
    Applying(EncoderTransaction),
}

#[derive(Debug, Clone)]
pub(super) enum EncoderTransactionEvent {
    StreamConfigStaged,
    Abort,
}

#[derive(Debug, Clone)]
pub(super) enum EncoderTransactionTransition {
    Unchanged,
    Staged,
    Rollback(EncoderTransaction),
}

#[derive(Debug, Clone)]
pub(super) enum EncoderFailureTransition {
    Ignored,
    Rollback(EncoderTransaction),
    Recover(EncoderTransaction),
    Disconnect(EncoderTransaction),
}

impl EncoderApplyState {
    pub(super) fn begin(&mut self, directive: EncoderDirective, rollback: EncoderRollback) -> bool {
        if !matches!(self, Self::Committed) {
            return false;
        }
        *self = Self::Applying(EncoderTransaction {
            directive,
            rollback,
            stream_config_staged: false,
            expected_generation: None,
            deadline: Instant::now() + ENCODER_APPLY_TIMEOUT,
        });
        true
    }

    pub(super) fn is_applying(&self) -> bool {
        matches!(self, Self::Applying(_))
    }

    pub(super) fn directive(&self) -> Option<EncoderDirective> {
        match self {
            Self::Committed => None,
            Self::Applying(transaction) => Some(transaction.directive),
        }
    }

    pub(super) fn kind(&self) -> Option<EncoderDirectiveKind> {
        self.directive().map(|directive| directive.kind)
    }

    pub(super) fn stream_config_staged(&self) -> bool {
        matches!(self, Self::Applying(transaction) if transaction.stream_config_staged)
    }

    pub(super) fn matches_native_facts(
        &self,
        transaction_id: u64,
        encoder_generation: u64,
        stream_epoch: u32,
        height: u32,
    ) -> bool {
        matches!(
            self,
            Self::Applying(transaction)
                if transaction.directive.id == transaction_id
                    && transaction.directive.stream_epoch == stream_epoch
                    && transaction.directive.target_height == height
                    && transaction.expected_generation == Some(encoder_generation)
        )
    }

    pub(super) fn transaction_id_for_epoch(&self, stream_epoch: u32) -> u64 {
        self.directive()
            .filter(|directive| directive.stream_epoch == stream_epoch)
            .map_or(0, |directive| directive.id)
    }

    pub(super) fn report_started(
        &mut self,
        transaction_id: u64,
        encoder_generation: u64,
        stream_epoch: u32,
        height: u32,
    ) -> bool {
        if transaction_id == 0 || encoder_generation == 0 || height == 0 {
            return false;
        }
        let Self::Applying(transaction) = self else {
            return false;
        };
        if transaction.directive.id != transaction_id
            || transaction.directive.stream_epoch != stream_epoch
            || transaction.directive.target_height != height
        {
            return false;
        }
        match transaction.expected_generation {
            Some(expected) => expected == encoder_generation,
            None => {
                transaction.expected_generation = Some(encoder_generation);
                true
            }
        }
    }

    pub(super) fn take_matching_keyframe(
        &mut self,
        transaction_id: u64,
        encoder_generation: u64,
        stream_epoch: u32,
        height: u32,
        is_keyframe: bool,
    ) -> Option<EncoderTransaction> {
        if !is_keyframe {
            return None;
        }
        let previous = std::mem::take(self);
        let Self::Applying(transaction) = previous else {
            return None;
        };
        let matches = transaction.directive.id == transaction_id
            && transaction.directive.stream_epoch == stream_epoch
            && transaction.directive.target_height == height
            && transaction.expected_generation == Some(encoder_generation);
        if !matches {
            *self = Self::Applying(transaction);
            return None;
        }
        Some(transaction)
    }

    pub(super) fn report_failed(
        &mut self,
        transaction_id: u64,
        encoder_generation: u64,
    ) -> EncoderFailureTransition {
        let previous = std::mem::take(self);
        let Self::Applying(transaction) = previous else {
            return EncoderFailureTransition::Ignored;
        };
        let generation_matches = encoder_generation == 0
            || match transaction.expected_generation {
                Some(expected) => expected == encoder_generation,
                None => true,
            };
        if transaction.directive.id != transaction_id || !generation_matches {
            *self = Self::Applying(transaction);
            return EncoderFailureTransition::Ignored;
        }
        if transaction.directive.kind == EncoderDirectiveKind::Recovery {
            return EncoderFailureTransition::Disconnect(transaction);
        }
        if encoder_generation == 0 {
            EncoderFailureTransition::Rollback(transaction)
        } else {
            EncoderFailureTransition::Recover(transaction)
        }
    }

    pub(super) fn expire(&mut self, now: Instant) -> EncoderFailureTransition {
        let Self::Applying(transaction) = self else {
            return EncoderFailureTransition::Ignored;
        };
        if now < transaction.deadline {
            return EncoderFailureTransition::Ignored;
        }
        let previous = std::mem::take(self);
        let Self::Applying(transaction) = previous else {
            unreachable!();
        };
        if transaction.directive.kind == EncoderDirectiveKind::Recovery {
            EncoderFailureTransition::Disconnect(transaction)
        } else {
            EncoderFailureTransition::Recover(transaction)
        }
    }

    pub(super) fn reduce(
        &mut self,
        event: EncoderTransactionEvent,
    ) -> EncoderTransactionTransition {
        match event {
            EncoderTransactionEvent::StreamConfigStaged => {
                let Self::Applying(transaction) = self else {
                    return EncoderTransactionTransition::Unchanged;
                };
                transaction.stream_config_staged = true;
                EncoderTransactionTransition::Staged
            }
            EncoderTransactionEvent::Abort => {
                let previous = std::mem::take(self);
                let Self::Applying(transaction) = previous else {
                    return EncoderTransactionTransition::Unchanged;
                };
                EncoderTransactionTransition::Rollback(transaction)
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn directive(kind: EncoderDirectiveKind) -> EncoderDirective {
        EncoderDirective {
            id: 7,
            kind,
            target_height: 720,
            target_bitrate_bps: 3_000_000,
            stream_epoch: 4,
        }
    }

    fn rollback() -> EncoderRollback {
        EncoderRollback {
            stream_config: Some(StreamConfigParams::default()),
            stream_config_sent: true,
            encoder_generation: 6,
        }
    }

    #[test]
    fn staged_config_and_rollback_snapshot_belong_to_transaction() {
        let mut state = EncoderApplyState::default();
        assert!(state.begin(directive(EncoderDirectiveKind::Local), rollback()));
        assert!(matches!(
            state.reduce(EncoderTransactionEvent::StreamConfigStaged),
            EncoderTransactionTransition::Staged
        ));
        let EncoderTransactionTransition::Rollback(transaction) =
            state.reduce(EncoderTransactionEvent::Abort)
        else {
            panic!("expected rollback");
        };
        assert!(transaction.stream_config_staged);
        assert!(transaction.rollback.stream_config_sent);
    }

    #[test]
    fn matching_idr_requires_started_generation_and_all_transaction_fields() {
        let mut state = EncoderApplyState::default();
        assert!(state.begin(directive(EncoderDirectiveKind::Local), rollback()));
        assert!(!state.report_started(7, 0, 4, 720));
        assert!(state.report_started(7, 19, 4, 720));
        assert!(state.take_matching_keyframe(7, 20, 4, 720, true).is_none());
        assert!(state.take_matching_keyframe(7, 19, 4, 720, false).is_none());
        assert!(state.take_matching_keyframe(7, 19, 4, 720, true).is_some());
    }

    #[test]
    fn failure_before_start_rolls_back_but_started_failure_requests_recovery() {
        let mut before_start = EncoderApplyState::default();
        assert!(before_start.begin(directive(EncoderDirectiveKind::Local), rollback()));
        assert!(matches!(
            before_start.report_failed(7, 0),
            EncoderFailureTransition::Rollback(_)
        ));

        let mut started = EncoderApplyState::default();
        assert!(started.begin(directive(EncoderDirectiveKind::Local), rollback()));
        assert!(started.report_started(7, 21, 4, 720));
        assert!(matches!(
            started.report_failed(7, 21),
            EncoderFailureTransition::Recover(_)
        ));
    }
}
