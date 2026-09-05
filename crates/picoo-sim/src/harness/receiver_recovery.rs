//! Production-equivalent Decoder refresh admission for the virtual Receiver.

use picoo_jitter::FrontFrameDescriptor;
use picoo_receiver::media_scheduler::RecoveryAdmission;

use super::ReceiverCore;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) struct RefreshCandidate {
    pub(super) connection_generation: u64,
    pub(super) stream_generation: u64,
    pub(super) frame_id: u64,
}

impl ReceiverCore {
    pub(super) fn recovery_admission(&self, frame: FrontFrameDescriptor) -> RecoveryAdmission {
        if !self.waiting_for_idr {
            return RecoveryAdmission::Ready;
        }
        let Some(candidate) = self.refresh_candidate else {
            return if frame.keyframe {
                RecoveryAdmission::Ready
            } else {
                RecoveryAdmission::Drop
            };
        };
        if frame.keyframe {
            RecoveryAdmission::Ready
        } else if frame.discardable {
            RecoveryAdmission::Drop
        } else if self.active_generation == Some(candidate.connection_generation)
            && frame.stream_generation == candidate.stream_generation
            && frame.frame_id > candidate.frame_id
        {
            RecoveryAdmission::WaitForRefresh
        } else {
            RecoveryAdmission::Drop
        }
    }

    pub(super) fn enter_recovery(&mut self) {
        self.reference_chain_intact = false;
        self.waiting_for_idr = true;
        self.refresh_candidate = None;
        self.jitter.discard_queued();
        self.decoder.reset();
    }
}
