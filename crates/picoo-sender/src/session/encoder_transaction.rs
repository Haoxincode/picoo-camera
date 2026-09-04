//! Rust-owned encoder apply transaction state — REQ-PICOO-MEDIA-003/010/016.

use crate::stream_config::StreamConfigParams;

use super::{EncoderDirective, EncoderDirectiveKind};

#[derive(Debug, Clone)]
pub(super) struct EncoderRollback {
    pub(super) stream_config: Option<StreamConfigParams>,
    pub(super) stream_config_sent: bool,
}

#[derive(Debug, Clone)]
pub(super) struct EncoderTransaction {
    pub(super) directive: EncoderDirective,
    pub(super) rollback: EncoderRollback,
    pub(super) stream_config_staged: bool,
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
    CommitLocal { stream_epoch: u32, height: u32 },
    CommitDirective { id: u64, height: u32 },
    CancelLocal { stream_epoch: u32 },
    RejectDirective { id: u64 },
    Abort,
}

#[derive(Debug, Clone)]
pub(super) enum EncoderTransactionTransition {
    Unchanged,
    Staged,
    Commit(EncoderTransaction),
    Rollback(EncoderTransaction),
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
            event => {
                let previous = std::mem::take(self);
                let Self::Applying(transaction) = previous else {
                    return EncoderTransactionTransition::Unchanged;
                };
                let matches = match event {
                    EncoderTransactionEvent::CommitLocal {
                        stream_epoch,
                        height,
                    } => {
                        transaction.directive.kind == EncoderDirectiveKind::Local
                            && transaction.directive.stream_epoch == stream_epoch
                            && transaction.directive.target_height == height
                    }
                    EncoderTransactionEvent::CommitDirective { id, height } => {
                        transaction.directive.kind != EncoderDirectiveKind::Local
                            && transaction.directive.id == id
                            && transaction.directive.target_height == height
                    }
                    EncoderTransactionEvent::CancelLocal { stream_epoch } => {
                        transaction.directive.kind == EncoderDirectiveKind::Local
                            && transaction.directive.stream_epoch == stream_epoch
                    }
                    EncoderTransactionEvent::RejectDirective { id } => {
                        transaction.directive.kind != EncoderDirectiveKind::Local
                            && transaction.directive.id == id
                    }
                    EncoderTransactionEvent::Abort => true,
                    EncoderTransactionEvent::StreamConfigStaged => unreachable!(),
                };
                if !matches {
                    *self = Self::Applying(transaction);
                    return EncoderTransactionTransition::Unchanged;
                }
                match event {
                    EncoderTransactionEvent::CommitLocal { .. }
                    | EncoderTransactionEvent::CommitDirective { .. } => {
                        EncoderTransactionTransition::Commit(transaction)
                    }
                    EncoderTransactionEvent::CancelLocal { .. }
                    | EncoderTransactionEvent::RejectDirective { .. }
                    | EncoderTransactionEvent::Abort => {
                        EncoderTransactionTransition::Rollback(transaction)
                    }
                    EncoderTransactionEvent::StreamConfigStaged => unreachable!(),
                }
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
        }
    }

    #[test]
    fn local_and_abr_commit_are_distinct_events() {
        let mut local = EncoderApplyState::default();
        assert!(local.begin(directive(EncoderDirectiveKind::Local), rollback()));
        assert!(matches!(
            local.reduce(EncoderTransactionEvent::CommitDirective { id: 7, height: 720 }),
            EncoderTransactionTransition::Unchanged
        ));
        assert!(matches!(
            local.reduce(EncoderTransactionEvent::CommitLocal {
                stream_epoch: 4,
                height: 720,
            }),
            EncoderTransactionTransition::Commit(_)
        ));

        let mut abr = EncoderApplyState::default();
        assert!(abr.begin(directive(EncoderDirectiveKind::AbrDownshift), rollback()));
        assert!(matches!(
            abr.reduce(EncoderTransactionEvent::CommitLocal {
                stream_epoch: 4,
                height: 720,
            }),
            EncoderTransactionTransition::Unchanged
        ));
        assert!(matches!(
            abr.reduce(EncoderTransactionEvent::CommitDirective { id: 7, height: 720 }),
            EncoderTransactionTransition::Commit(_)
        ));
    }

    #[test]
    fn mismatched_event_cannot_consume_active_transaction() {
        let mut state = EncoderApplyState::default();
        assert!(state.begin(directive(EncoderDirectiveKind::AbrUpshift), rollback()));
        assert!(matches!(
            state.reduce(EncoderTransactionEvent::RejectDirective { id: 8 }),
            EncoderTransactionTransition::Unchanged
        ));
        assert_eq!(state.directive().map(|value| value.id), Some(7));
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
}
