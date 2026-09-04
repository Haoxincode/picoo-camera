use picoo_protocol::control::control_envelope::Payload as ControlPayload;
use picoo_rate_control::{BitrateAction, BitrateLadder};
use picoo_transport::PicooTransport;

use super::encoder_transaction::{
    EncoderRollback, EncoderTransaction, EncoderTransactionEvent, EncoderTransactionTransition,
};
use super::{EncoderDirective, EncoderDirectiveKind, SenderSession, MAX_STREAM_EPOCH};
use crate::stream_config::StreamConfigParams;
use crate::SenderError;

impl<T: PicooTransport> SenderSession<T> {
    pub fn set_stream_config(&mut self, mut config: StreamConfigParams) {
        // Allow re-send when SPS/PPS arrive late or resolution/mirror changes (PUC-005/006).
        config.stream_epoch = self.current_stream_epoch;
        self.pending_stream_config = Some(config);
        self.stream_config_sent = false;
        self.encoder_apply_state
            .reduce(EncoderTransactionEvent::StreamConfigStaged);
    }

    /// Host applied an encode height for the current Rust-owned generation.
    pub fn report_encoder_height(&mut self, height: u32, stream_epoch: u32) -> bool {
        if height == 0 {
            return false;
        }
        let normalized_height = picoo_rate_control::normalize_height(height);
        if height != normalized_height {
            return false;
        }
        let transition = self
            .encoder_apply_state
            .reduce(EncoderTransactionEvent::CommitLocal {
                stream_epoch,
                height: normalized_height,
            });
        if let EncoderTransactionTransition::Commit(transaction) = transition {
            self.commit_stream_epoch(transaction, normalized_height);
        } else if self.encoder_apply_state.is_applying()
            || stream_epoch != self.current_stream_epoch
        {
            return false;
        } else if self.committed_encoder_height == 0 {
            // Initial synchronization is allowed only for the StreamConfig
            // already associated with the committed epoch.
            let configured_height = self
                .pending_stream_config
                .as_ref()
                .map(|config| config.height);
            if configured_height != Some(height) {
                return false;
            }
            self.committed_encoder_height = normalized_height;
        } else if height != self.committed_encoder_height {
            // Any actual resolution change must use begin/apply/report so it
            // receives a fresh epoch and cannot mutate committed state.
            return false;
        }
        self.bitrate.sync_encode_height(height);
        true
    }

    /// Allocate a fresh stream generation before a native encoder discontinuity.
    pub fn begin_stream_reconfiguration(&mut self, target_height: u32) -> u32 {
        // The platform must explicitly ACK/NACK/cancel the existing transition.
        // Silently replacing it would let a late native callback commit the
        // wrong generation.
        if self.encoder_apply_state.is_applying() {
            return 0;
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
    ) {
        // Keep only a config explicitly staged during this transaction and
        // matching the native encoder output. The old epoch's config must
        // never be relabelled and sent for the new epoch.
        let staged_config = transaction
            .stream_config_staged
            .then(|| self.pending_stream_config.clone())
            .flatten()
            .filter(|config| config.height == actual_height)
            .map(|mut config| {
                config.stream_epoch = transaction.directive.stream_epoch;
                config
            });
        self.current_stream_epoch = transaction.directive.stream_epoch;
        self.committed_encoder_height = actual_height;
        self.pending_stream_config = staged_config;
        self.stream_config_sent = false;
        self.media_blocked_for_stream_config = true;
        self.keyframe_requested = true;
    }

    pub fn cancel_stream_reconfiguration(&mut self, stream_epoch: u32) -> bool {
        let transition = self
            .encoder_apply_state
            .reduce(EncoderTransactionEvent::CancelLocal { stream_epoch });
        let EncoderTransactionTransition::Rollback(transaction) = transition else {
            return false;
        };
        self.rollback_encoder_transaction(transaction);
        true
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
        let session = self.session.ok_or(SenderError::NotConnected)?;
        let mut config = config.clone();
        config.stream_epoch = self.current_stream_epoch;
        let msg = config.to_proto();
        self.send_control_payload(session, ControlPayload::StreamConfig(msg))?;
        self.pending_stream_config = Some(config);
        Ok(())
    }

    pub(super) fn abort_pending_reconfiguration(&mut self) {
        let transition = self
            .encoder_apply_state
            .reduce(EncoderTransactionEvent::Abort);
        let EncoderTransactionTransition::Rollback(transaction) = transition else {
            return;
        };
        let directive = transaction.directive;
        if directive.kind != EncoderDirectiveKind::Local {
            match directive.kind {
                EncoderDirectiveKind::Local => unreachable!(),
                EncoderDirectiveKind::AbrDownshift => self
                    .bitrate
                    .reject_resolution_change(BitrateAction::DownshiftResolution),
                EncoderDirectiveKind::AbrUpshift => self
                    .bitrate
                    .reject_resolution_change(BitrateAction::UpshiftResolution),
            }
        }
        self.rollback_encoder_transaction(transaction);
    }

    pub(super) fn begin_encoder_transaction(&mut self, directive: EncoderDirective) -> bool {
        self.encoder_apply_state.begin(
            directive,
            EncoderRollback {
                stream_config: self.pending_stream_config.clone(),
                stream_config_sent: self.stream_config_sent,
            },
        )
    }

    pub(super) fn rollback_encoder_transaction(&mut self, transaction: EncoderTransaction) {
        self.pending_stream_config = transaction.rollback.stream_config;
        self.stream_config_sent = transaction.rollback.stream_config_sent;
    }
}
