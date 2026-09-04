use std::time::Instant;

use picoo_protocol::control::control_envelope::Payload as ControlPayload;
use picoo_rate_control::{BitrateAction, BitrateLadder};
use picoo_transport::PicooTransport;

use super::encoder_transaction::{
    EncoderFailureTransition, EncoderRollback, EncoderTransaction, EncoderTransactionEvent,
    EncoderTransactionTransition,
};
use super::{
    EncoderDirective, EncoderDirectiveKind, EncoderFailureOutcome, SenderSession, MAX_STREAM_EPOCH,
};
use crate::stream_config::StreamConfigParams;
use crate::SenderError;

impl<T: PicooTransport> SenderSession<T> {
    pub fn encoder_transaction_id_for_epoch(&self, stream_epoch: u32) -> u64 {
        self.encoder_apply_state
            .transaction_id_for_epoch(stream_epoch)
    }

    pub fn report_encoder_started(
        &mut self,
        transaction_id: u64,
        encoder_generation: u64,
        stream_epoch: u32,
        height: u32,
    ) -> bool {
        if encoder_generation == 0
            || height == 0
            || height != picoo_rate_control::normalize_height(height)
        {
            return false;
        }
        if transaction_id != 0 {
            return self.encoder_apply_state.report_started(
                transaction_id,
                encoder_generation,
                stream_epoch,
                height,
            );
        }
        if self.encoder_apply_state.is_applying() || stream_epoch != self.current_stream_epoch {
            return false;
        }
        if self.committed_encoder_generation == 0 {
            if self
                .pending_stream_config
                .as_ref()
                .is_some_and(|config| config.height != height)
            {
                return false;
            }
            self.committed_encoder_generation = encoder_generation;
            self.committed_encoder_height = height;
            self.bitrate.sync_encode_height(height);
            return true;
        }
        self.committed_encoder_generation == encoder_generation
            && self.committed_encoder_height == height
    }

    pub fn report_encoder_failed(
        &mut self,
        transaction_id: u64,
        encoder_generation: u64,
    ) -> EncoderFailureOutcome {
        if transaction_id == 0 {
            if self.encoder_apply_state.is_applying()
                || encoder_generation == 0
                || encoder_generation != self.committed_encoder_generation
            {
                return EncoderFailureOutcome::Ignored;
            }
            return self.start_committed_encoder_recovery();
        }
        let transition = self
            .encoder_apply_state
            .report_failed(transaction_id, encoder_generation);
        self.handle_encoder_failure_transition(transition)
    }

    pub(super) fn expire_encoder_transaction(&mut self, now: Instant) {
        let transition = self.encoder_apply_state.expire(now);
        let _ = self.handle_encoder_failure_transition(transition);
    }

    pub fn set_stream_config(&mut self, mut config: StreamConfigParams) {
        // Allow re-send when SPS/PPS arrive late or resolution/mirror changes (PUC-005/006).
        config.stream_epoch = self.current_stream_epoch;
        self.pending_stream_config = Some(config);
        self.stream_config_sent = false;
        self.encoder_apply_state
            .reduce(EncoderTransactionEvent::StreamConfigStaged);
    }

    /// Allocate a fresh stream generation before a native encoder discontinuity.
    pub fn begin_stream_reconfiguration(&mut self, target_height: u32) -> u32 {
        if self.encoder_apply_state.is_applying() {
            match self.encoder_apply_state.kind() {
                Some(EncoderDirectiveKind::AbrDownshift | EncoderDirectiveKind::AbrUpshift) => {
                    // A user/camera transition supersedes ABR inside the Rust
                    // authority. Late native facts retain the old transaction
                    // id and therefore cannot commit the replacement.
                    let transition = self
                        .encoder_apply_state
                        .reduce(EncoderTransactionEvent::Abort);
                    let EncoderTransactionTransition::Rollback(transaction) = transition else {
                        return 0;
                    };
                    self.reject_transaction_bitrate(transaction.directive.kind);
                    self.rollback_encoder_transaction(transaction);
                }
                Some(EncoderDirectiveKind::Local | EncoderDirectiveKind::Recovery) | None => {
                    return 0;
                }
            }
        }
        if target_height == 0 {
            return 0;
        }
        let target_height = picoo_rate_control::normalize_height(target_height);
        let id = self.next_encoder_directive_id;
        let Some(next_id) = id.checked_add(1) else {
            self.last_session_error = Some("ENCODER_DIRECTIVE_ID_EXHAUSTED".into());
            return 0;
        };
        let epoch = self.allocate_stream_epoch();
        if epoch == 0 {
            return 0;
        }
        let directive = EncoderDirective {
            id,
            kind: EncoderDirectiveKind::Local,
            target_height,
            target_bitrate_bps: BitrateLadder::for_height(target_height).initial_bps,
            stream_epoch: epoch,
        };
        if !self.begin_encoder_transaction(directive) {
            return 0;
        }
        self.next_encoder_directive_id = next_id;
        self.keyframe_requested = true;
        epoch
    }

    pub(super) fn allocate_stream_epoch(&mut self) -> u32 {
        if self.last_allocated_stream_epoch >= MAX_STREAM_EPOCH {
            self.last_session_error = Some("STREAM_EPOCH_EXHAUSTED".into());
            return 0;
        }
        let Some(next) = self.last_allocated_stream_epoch.checked_add(1) else {
            self.last_session_error = Some("STREAM_EPOCH_EXHAUSTED".into());
            return 0;
        };
        self.last_allocated_stream_epoch = next;
        next
    }

    pub(super) fn commit_stream_epoch(
        &mut self,
        transaction: EncoderTransaction,
        actual_height: u32,
        encoder_generation: u64,
        committed_config: StreamConfigParams,
    ) {
        debug_assert!(transaction.stream_config_staged);
        debug_assert_eq!(committed_config.height, actual_height);
        debug_assert_eq!(
            committed_config.stream_epoch,
            transaction.directive.stream_epoch
        );
        self.current_stream_epoch = transaction.directive.stream_epoch;
        self.committed_encoder_height = actual_height;
        self.committed_encoder_generation = encoder_generation;
        self.pending_stream_config = Some(committed_config);
        self.stream_config_sent = true;
        self.media_blocked_for_stream_config = false;
        self.keyframe_requested = true;
    }

    pub(super) fn send_pending_stream_config(&mut self) -> Result<(), SenderError> {
        if self.stream_config_sent || self.encoder_apply_state.is_applying() {
            return Ok(());
        }
        let Some(config) = self.pending_stream_config.clone() else {
            return Ok(());
        };
        if self.media_blocked_for_stream_config && config.height != self.committed_encoder_height {
            self.last_session_error = Some("STREAM_CONFIG_HEIGHT_MISMATCH".into());
            return Err(SenderError::StreamConfigHeightMismatch {
                expected: self.committed_encoder_height,
                got: config.height,
            });
        }
        self.send_stream_config(&config)?;
        self.stream_config_sent = true;
        self.media_blocked_for_stream_config = false;
        Ok(())
    }

    fn send_stream_config(&mut self, config: &StreamConfigParams) -> Result<(), SenderError> {
        let mut config = config.clone();
        config.stream_epoch = self.current_stream_epoch;
        self.send_stream_config_for_epoch(&config, self.current_stream_epoch)?;
        self.pending_stream_config = Some(config);
        Ok(())
    }

    pub(super) fn send_stream_config_for_epoch(
        &mut self,
        config: &StreamConfigParams,
        stream_epoch: u32,
    ) -> Result<(), SenderError> {
        let session = self.active_session().ok_or(SenderError::NotConnected)?;
        let mut wire_config = config.clone();
        wire_config.stream_epoch = stream_epoch;
        self.send_control_payload(
            session,
            ControlPayload::StreamConfig(wire_config.to_proto()),
        )
    }

    pub(super) fn abort_pending_reconfiguration(&mut self) {
        let transition = self
            .encoder_apply_state
            .reduce(EncoderTransactionEvent::Abort);
        let EncoderTransactionTransition::Rollback(transaction) = transition else {
            return;
        };
        self.reject_transaction_bitrate(transaction.directive.kind);
        self.committed_encoder_generation = transaction.rollback.encoder_generation;
        self.rollback_encoder_transaction(transaction);
    }

    pub(super) fn begin_encoder_transaction(&mut self, directive: EncoderDirective) -> bool {
        self.encoder_apply_state.begin(
            directive,
            EncoderRollback {
                stream_config: self.pending_stream_config.clone(),
                stream_config_sent: self.stream_config_sent,
                encoder_generation: self.committed_encoder_generation,
            },
        )
    }

    pub(super) fn rollback_encoder_transaction(&mut self, transaction: EncoderTransaction) {
        self.pending_stream_config = transaction.rollback.stream_config;
        self.stream_config_sent = transaction.rollback.stream_config_sent;
    }

    pub(super) fn commit_encoder_recovery(
        &mut self,
        transaction: EncoderTransaction,
        actual_height: u32,
        encoder_generation: u64,
        committed_config: StreamConfigParams,
    ) {
        debug_assert_eq!(transaction.directive.kind, EncoderDirectiveKind::Recovery);
        debug_assert!(transaction.stream_config_staged);
        debug_assert_eq!(committed_config.height, actual_height);
        debug_assert_eq!(committed_config.stream_epoch, self.current_stream_epoch);
        self.committed_encoder_height = actual_height;
        self.committed_encoder_generation = encoder_generation;
        self.pending_stream_config = Some(committed_config);
        self.stream_config_sent = true;
        self.media_blocked_for_stream_config = false;
        self.keyframe_requested = true;
    }

    fn handle_encoder_failure_transition(
        &mut self,
        transition: EncoderFailureTransition,
    ) -> EncoderFailureOutcome {
        match transition {
            EncoderFailureTransition::Ignored => EncoderFailureOutcome::Ignored,
            EncoderFailureTransition::Rollback(transaction) => {
                self.reject_transaction_bitrate(transaction.directive.kind);
                self.committed_encoder_generation = transaction.rollback.encoder_generation;
                self.rollback_encoder_transaction(transaction);
                EncoderFailureOutcome::RolledBack
            }
            EncoderFailureTransition::Recover(transaction) => {
                self.reject_transaction_bitrate(transaction.directive.kind);
                self.committed_encoder_generation = transaction.rollback.encoder_generation;
                self.rollback_encoder_transaction(transaction);
                self.start_committed_encoder_recovery()
            }
            EncoderFailureTransition::Disconnect(transaction) => {
                self.committed_encoder_generation = transaction.rollback.encoder_generation;
                self.rollback_encoder_transaction(transaction);
                self.last_session_error = Some("ENCODER_RECOVERY_FAILED".into());
                self.disconnect();
                EncoderFailureOutcome::Disconnected
            }
        }
    }

    fn start_committed_encoder_recovery(&mut self) -> EncoderFailureOutcome {
        if self.committed_encoder_height == 0 || self.encoder_apply_state.is_applying() {
            self.last_session_error = Some("ENCODER_RECOVERY_UNAVAILABLE".into());
            self.disconnect();
            return EncoderFailureOutcome::Disconnected;
        }
        let id = self.next_encoder_directive_id;
        let Some(next_id) = id.checked_add(1) else {
            self.last_session_error = Some("ENCODER_DIRECTIVE_ID_EXHAUSTED".into());
            self.disconnect();
            return EncoderFailureOutcome::Disconnected;
        };
        let directive = EncoderDirective {
            id,
            kind: EncoderDirectiveKind::Recovery,
            target_height: self.committed_encoder_height,
            target_bitrate_bps: self.current_bitrate_bps(),
            stream_epoch: self.current_stream_epoch,
        };
        if !self.begin_encoder_transaction(directive) {
            self.last_session_error = Some("ENCODER_RECOVERY_STATE_CONFLICT".into());
            self.disconnect();
            return EncoderFailureOutcome::Disconnected;
        }
        self.next_encoder_directive_id = next_id;
        self.keyframe_requested = true;
        EncoderFailureOutcome::RecoveryRequested
    }

    fn reject_transaction_bitrate(&mut self, kind: EncoderDirectiveKind) {
        match kind {
            EncoderDirectiveKind::AbrDownshift => self
                .bitrate
                .reject_resolution_change(BitrateAction::DownshiftResolution),
            EncoderDirectiveKind::AbrUpshift => self
                .bitrate
                .reject_resolution_change(BitrateAction::UpshiftResolution),
            EncoderDirectiveKind::Local | EncoderDirectiveKind::Recovery => {}
        }
    }
}
