//! Deterministic asynchronous Decoder adapter for the production scheduler harness.

use std::collections::VecDeque;
use std::time::Duration;

use picoo_jitter::Frame as JitterFrame;
use picoo_receiver::media_scheduler::DecoderAdmission;

const MAX_PENDING_DECODE_JOBS: usize = 2;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum SimFrameKind {
    Key,
    ReferenceDelta,
    DiscardableDelta,
}

impl SimFrameKind {
    fn from_frame(frame: &JitterFrame) -> Self {
        if frame.keyframe {
            Self::Key
        } else if frame.discardable {
            Self::DiscardableDelta
        } else {
            Self::ReferenceDelta
        }
    }
}

#[derive(Debug)]
pub(super) struct SimDecodeJob {
    pub(super) connection_generation: u64,
    pub(super) frame: JitterFrame,
    pub(super) width: u32,
    pub(super) height: u32,
    pub(super) rotation: u32,
}

#[derive(Debug)]
struct ActiveDecode {
    job: SimDecodeJob,
    started_at_us: u64,
    completes_at_us: u64,
}

#[derive(Debug)]
pub(super) struct SimulatedDecoder {
    latency_us: u64,
    active: Option<ActiveDecode>,
    pending: VecDeque<SimDecodeJob>,
}

impl SimulatedDecoder {
    pub(super) fn new(latency: Duration) -> Self {
        Self {
            latency_us: latency.as_micros().min(u128::from(u64::MAX)) as u64,
            active: None,
            pending: VecDeque::with_capacity(MAX_PENDING_DECODE_JOBS),
        }
    }

    pub(super) fn admission(&self, keyframe: bool, discardable: bool) -> DecoderAdmission {
        let kind = if keyframe {
            SimFrameKind::Key
        } else if discardable {
            SimFrameKind::DiscardableDelta
        } else {
            SimFrameKind::ReferenceDelta
        };
        if kind == SimFrameKind::Key || self.pending.len() < MAX_PENDING_DECODE_JOBS {
            return DecoderAdmission::Ready;
        }
        if kind == SimFrameKind::ReferenceDelta
            && self
                .pending
                .iter()
                .any(|job| SimFrameKind::from_frame(&job.frame) == SimFrameKind::DiscardableDelta)
        {
            DecoderAdmission::Ready
        } else if kind == SimFrameKind::DiscardableDelta {
            DecoderAdmission::DropDiscardable
        } else {
            DecoderAdmission::WaitForCapacity
        }
    }

    pub(super) fn submit(&mut self, job: SimDecodeJob, now_us: u64) {
        match SimFrameKind::from_frame(&job.frame) {
            SimFrameKind::Key => self.pending.clear(),
            SimFrameKind::ReferenceDelta if self.pending.len() >= MAX_PENDING_DECODE_JOBS => {
                if let Some(index) = self.pending.iter().position(|pending| {
                    SimFrameKind::from_frame(&pending.frame) == SimFrameKind::DiscardableDelta
                }) {
                    self.pending.remove(index);
                }
            }
            SimFrameKind::ReferenceDelta | SimFrameKind::DiscardableDelta => {}
        }
        debug_assert!(
            self.pending.len() < MAX_PENDING_DECODE_JOBS,
            "scheduler submitted without Decoder admission"
        );
        self.pending.push_back(job);
        self.start_next(now_us);
    }

    pub(super) fn take_completed(&mut self, now_us: u64) -> Option<(SimDecodeJob, u64, u64)> {
        let active = self
            .active
            .as_ref()
            .filter(|active| active.completes_at_us <= now_us)?;
        let completed_at_us = active.completes_at_us;
        let active = self.active.take().expect("ready active decode");
        let elapsed_us = completed_at_us.saturating_sub(active.started_at_us);
        self.start_next(completed_at_us);
        Some((active.job, active.started_at_us, elapsed_us))
    }

    pub(super) fn reset(&mut self) {
        self.active = None;
        self.pending.clear();
    }

    pub(super) fn pending_depth(&self) -> usize {
        self.pending.len()
    }

    pub(super) fn is_active(&self) -> bool {
        self.active.is_some()
    }

    fn start_next(&mut self, now_us: u64) {
        if self.active.is_some() {
            return;
        }
        let Some(job) = self.pending.pop_front() else {
            return;
        };
        self.active = Some(ActiveDecode {
            job,
            started_at_us: now_us,
            completes_at_us: now_us.saturating_add(self.latency_us),
        });
    }
}
