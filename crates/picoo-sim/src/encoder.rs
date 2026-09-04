use bytes::Bytes;
use thiserror::Error;

#[derive(Debug, Clone)]
pub struct CameraFrame {
    pub data: Bytes,
    pub keyframe: bool,
    pub pts_us: u64,
    pub encoded_at_us: u64,
    pub stream_epoch: u32,
    pub encoder_generation: u64,
    pub width: u32,
    pub height: u32,
}

impl CameraFrame {
    pub fn synthetic(marker: u8, encoded_bytes: usize, keyframe: bool, pts_us: u64) -> Self {
        Self {
            data: Bytes::from(vec![marker; encoded_bytes.max(1)]),
            keyframe,
            pts_us,
            encoded_at_us: pts_us.saturating_add(2_000),
            stream_epoch: 1,
            encoder_generation: 1,
            width: 16,
            height: 16,
        }
    }

    pub fn for_encoder(
        mut self,
        stream_epoch: u32,
        encoder_generation: u64,
        width: u32,
        height: u32,
    ) -> Self {
        self.stream_epoch = stream_epoch;
        self.encoder_generation = encoder_generation;
        self.width = width;
        self.height = height;
        self
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct EncoderCommit {
    pub transaction_id: u64,
    pub stream_epoch: u32,
    pub encoder_generation: u64,
    pub height: u32,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum EncoderFailure {
    Ignored,
    RolledBack,
    RecoveryRequired,
}

#[derive(Debug, Clone, Copy)]
pub(crate) struct EncoderConfig {
    pub(crate) stream_epoch: u32,
    pub(crate) encoder_generation: u64,
    pub(crate) width: u32,
    pub(crate) height: u32,
}

#[derive(Debug, Clone, Copy)]
struct EncoderApply {
    transaction_id: u64,
    target: EncoderConfig,
    rollback: EncoderConfig,
    started: bool,
}

#[derive(Debug)]
pub(crate) struct ScriptedEncoder {
    pub(crate) committed: EncoderConfig,
    applying: Option<EncoderApply>,
    pub(crate) last_commit: Option<EncoderCommit>,
}

impl ScriptedEncoder {
    pub(crate) fn new() -> Self {
        Self {
            committed: EncoderConfig {
                stream_epoch: 1,
                encoder_generation: 1,
                width: 16,
                height: 16,
            },
            applying: None,
            last_commit: None,
        }
    }

    pub(crate) fn begin(&mut self, transaction_id: u64, target: EncoderConfig) -> bool {
        if transaction_id == 0 || self.applying.is_some() {
            return false;
        }
        self.applying = Some(EncoderApply {
            transaction_id,
            target,
            rollback: self.committed,
            started: false,
        });
        true
    }

    pub(crate) fn report_started(&mut self, transaction_id: u64, generation: u64) -> bool {
        let Some(apply) = self.applying.as_mut() else {
            return false;
        };
        if apply.transaction_id != transaction_id || apply.target.encoder_generation != generation {
            return false;
        }
        apply.started = true;
        true
    }

    pub(crate) fn fail(&mut self, transaction_id: u64, generation: u64) -> EncoderFailure {
        let Some(apply) = self.applying else {
            return EncoderFailure::Ignored;
        };
        if apply.transaction_id != transaction_id
            || (generation != 0 && generation != apply.target.encoder_generation)
        {
            return EncoderFailure::Ignored;
        }
        self.applying = None;
        self.committed = apply.rollback;
        if generation == 0 || !apply.started {
            EncoderFailure::RolledBack
        } else {
            EncoderFailure::RecoveryRequired
        }
    }

    pub(crate) fn accept(&mut self, frame: &CameraFrame) -> Result<(), SimError> {
        if let Some(apply) = self.applying {
            let matches = apply.started
                && apply.target.stream_epoch == frame.stream_epoch
                && apply.target.encoder_generation == frame.encoder_generation
                && apply.target.width == frame.width
                && apply.target.height == frame.height;
            if !matches {
                return Err(SimError::StaleEncoderFact);
            }
            if !frame.keyframe {
                return Err(SimError::EncoderRefreshPending);
            }
            self.committed = apply.target;
            self.applying = None;
            self.last_commit = Some(EncoderCommit {
                transaction_id: apply.transaction_id,
                stream_epoch: apply.target.stream_epoch,
                encoder_generation: apply.target.encoder_generation,
                height: apply.target.height,
            });
            return Ok(());
        }
        let committed = self.committed;
        if committed.stream_epoch != frame.stream_epoch
            || committed.encoder_generation != frame.encoder_generation
            || committed.width != frame.width
            || committed.height != frame.height
        {
            return Err(SimError::StaleEncoderFact);
        }
        Ok(())
    }
}

#[derive(Debug, Error, PartialEq, Eq)]
pub enum SimError {
    #[error("encoded access unit does not match the scripted encoder transaction")]
    StaleEncoderFact,
    #[error("encoder transaction is waiting for its first matching IDR")]
    EncoderRefreshPending,
    #[error("sender packetization failed: {0}")]
    Packetization(String),
}
