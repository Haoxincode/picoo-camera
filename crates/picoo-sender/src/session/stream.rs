use picoo_protocol::control::control_envelope::Payload as ControlPayload;
use picoo_rate_control::BitrateAction;
use picoo_transport::PicooTransport;

use super::{EncoderDirectiveKind, SenderSession, MAX_STREAM_EPOCH};
use crate::stream_config::StreamConfigParams;
use crate::SenderError;

impl<T: PicooTransport> SenderSession<T> {
    pub fn set_stream_config(&mut self, mut config: StreamConfigParams) {
        // Allow re-send when SPS/PPS arrive late or resolution/mirror changes (PUC-005/006).
        config.stream_epoch = self.current_stream_epoch;
        self.pending_stream_config = Some(config);
        self.stream_config_sent = false;
        if self.reconfiguration_rollback.is_some() {
            self.stream_config_staged_during_reconfiguration = true;
        }
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
        if self.pending_local_stream_epoch == Some(stream_epoch) {
            self.commit_stream_epoch(stream_epoch, normalized_height);
        } else if self.pending_local_stream_epoch.is_some()
            || self.pending_encoder_directive.is_some()
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
    pub fn begin_stream_reconfiguration(&mut self) -> u32 {
        // The platform must explicitly ACK/NACK/cancel the existing transition.
        // Silently replacing it would let a late native callback commit the
        // wrong generation.
        if self.pending_local_stream_epoch.is_some() || self.pending_encoder_directive.is_some() {
            return 0;
        }
        let epoch = self.allocate_stream_epoch();
        if epoch == 0 {
            return 0;
        }
        self.begin_reconfiguration_transaction();
        self.pending_local_stream_epoch = Some(epoch);
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

    pub(super) fn commit_stream_epoch(&mut self, epoch: u32, actual_height: u32) {
        // Keep only a config explicitly staged during this transaction and
        // matching the native encoder output. The old epoch's config must
        // never be relabelled and sent for the new epoch.
        let staged_config = self
            .stream_config_staged_during_reconfiguration
            .then(|| self.pending_stream_config.clone())
            .flatten()
            .filter(|config| config.height == actual_height)
            .map(|mut config| {
                config.stream_epoch = epoch;
                config
            });
        self.current_stream_epoch = epoch;
        self.pending_local_stream_epoch = None;
        self.committed_encoder_height = actual_height;
        self.pending_stream_config = staged_config;
        self.stream_config_sent = false;
        self.media_blocked_for_stream_config = true;
        self.keyframe_requested = true;
        self.reconfiguration_rollback = None;
        self.stream_config_staged_during_reconfiguration = false;
    }

    pub fn cancel_stream_reconfiguration(&mut self, stream_epoch: u32) -> bool {
        if self.pending_local_stream_epoch != Some(stream_epoch) {
            return false;
        }
        self.pending_local_stream_epoch = None;
        self.rollback_reconfiguration_transaction();
        true
    }

    pub(super) fn send_pending_stream_config(&mut self) -> Result<(), SenderError> {
        if self.stream_config_sent
            || self.pending_local_stream_epoch.is_some()
            || self.pending_encoder_directive.is_some()
        {
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
        if let Some(directive) = self.pending_encoder_directive.take() {
            match directive.kind {
                EncoderDirectiveKind::AbrDownshift => self
                    .bitrate
                    .reject_resolution_change(BitrateAction::DownshiftResolution),
                EncoderDirectiveKind::AbrUpshift => self
                    .bitrate
                    .reject_resolution_change(BitrateAction::UpshiftResolution),
            }
        }
        self.pending_local_stream_epoch = None;
        self.rollback_reconfiguration_transaction();
    }

    pub(super) fn begin_reconfiguration_transaction(&mut self) {
        debug_assert!(self.reconfiguration_rollback.is_none());
        self.reconfiguration_rollback =
            Some((self.pending_stream_config.clone(), self.stream_config_sent));
        self.stream_config_staged_during_reconfiguration = false;
    }

    pub(super) fn rollback_reconfiguration_transaction(&mut self) {
        if let Some((config, sent)) = self.reconfiguration_rollback.take() {
            self.pending_stream_config = config;
            self.stream_config_sent = sent;
        }
        self.stream_config_staged_during_reconfiguration = false;
    }
}
