//! Independent compressed ingress — REQ-PICOO-MEDIA-068.
//! This channel preserves arrival order; dependency reordering belongs to the worker.
use crate::bundle::{GapReason, SourceRange};
use picoo_packet::AssembledAccessUnit;
use picoo_protocol::{control::StreamConfig, MAX_MEDIA_ACCESS_UNIT_BYTES};
use std::{
    sync::{mpsc, Arc, OnceLock},
    time::{Duration, Instant},
};

const CAPACITY: usize = 16;
const MAX_AGE: Duration = Duration::from_millis(250);
const MAX_CONFIGURATION_BYTES: usize = 64 * 1024;

#[derive(Debug)]
pub struct RecordingInput {
    pub connection_generation: u64,
    pub configuration: Arc<StreamConfig>,
    pub access_unit: AssembledAccessUnit,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum IngressFailure {
    Capacity,
    TooOld,
    InvalidInput,
    WorkerStopped,
}

enum Event {
    Input(RecordingInput),
    Gap(GapReason, Option<SourceRange>),
}
struct Queued {
    event: Event,
    accepted_at: Instant,
}

/// Single Receiver-owned producer. Dropping it requests normal drain then stop.
/// No blocking send, filesystem operation, or consumer callback is permitted here.
pub struct RecordingIngress {
    sender: mpsc::SyncSender<Queued>,
    failure: Arc<OnceLock<IngressFailure>>,
}

pub struct RecordingInbox {
    receiver: Option<mpsc::Receiver<Queued>>,
    failure: Arc<OnceLock<IngressFailure>>,
}

#[derive(Debug)]
pub enum IngressPoll {
    Input(RecordingInput),
    Gap(GapReason, Option<SourceRange>),
    Idle,
    Drained,
}

pub fn channel() -> (RecordingIngress, RecordingInbox) {
    let (sender, receiver) = mpsc::sync_channel(CAPACITY);
    let failure = Arc::new(OnceLock::new());
    (
        RecordingIngress {
            sender,
            failure: Arc::clone(&failure),
        },
        RecordingInbox {
            receiver: Some(receiver),
            failure,
        },
    )
}

impl RecordingIngress {
    pub fn offer(&self, input: RecordingInput) -> Result<(), IngressFailure> {
        self.offer_at(input, Instant::now())
    }

    fn offer_at(&self, input: RecordingInput, now: Instant) -> Result<(), IngressFailure> {
        if let Some(failure) = self.failure.get() {
            return Err(*failure);
        }
        let invalid = input.access_unit.data.is_empty()
            || input.access_unit.data.len() > MAX_MEDIA_ACCESS_UNIT_BYTES as usize
            || input.configuration.codec_configuration.len() > MAX_CONFIGURATION_BYTES
            || input.configuration.codec_configuration.is_empty()
            || input.configuration.stream_epoch != input.access_unit.stream_epoch;
        if invalid {
            let _ = self.failure.set(IngressFailure::InvalidInput);
            return Err(*self.failure.get().expect("failure set"));
        }
        self.offer_event(Event::Input(input), now)
    }

    pub fn report_gap(
        &self,
        reason: GapReason,
        source: Option<SourceRange>,
    ) -> Result<(), IngressFailure> {
        self.offer_event(Event::Gap(reason, source), Instant::now())
    }

    fn offer_event(&self, event: Event, now: Instant) -> Result<(), IngressFailure> {
        if let Some(failure) = self.failure.get() {
            return Err(*failure);
        }
        if let Err(error) = self.sender.try_send(Queued {
            event,
            accepted_at: now,
        }) {
            let failure = match error {
                mpsc::TrySendError::Full(_) => IngressFailure::Capacity,
                mpsc::TrySendError::Disconnected(_) => IngressFailure::WorkerStopped,
            };
            let _ = self.failure.set(failure);
            return Err(*self.failure.get().expect("failure set"));
        }
        Ok(())
    }

    pub fn failure(&self) -> Option<IngressFailure> {
        self.failure.get().copied()
    }
}

impl RecordingInbox {
    pub fn poll(&mut self) -> Result<IngressPoll, IngressFailure> {
        self.poll_at(Instant::now())
    }

    fn poll_at(&mut self, now: Instant) -> Result<IngressPoll, IngressFailure> {
        if let Some(failure) = self.failure.get().copied() {
            self.receiver = None;
            return Err(failure);
        }
        let Some(receiver) = &self.receiver else {
            return Ok(IngressPoll::Drained);
        };
        match receiver.try_recv() {
            Ok(queued) => {
                if now.saturating_duration_since(queued.accepted_at) > MAX_AGE {
                    let _ = self.failure.set(IngressFailure::TooOld);
                }
                if let Some(failure) = self.failure.get().copied() {
                    self.receiver = None;
                    return Err(failure);
                }
                Ok(match queued.event {
                    Event::Input(input) => IngressPoll::Input(input),
                    Event::Gap(reason, source) => IngressPoll::Gap(reason, source),
                })
            }
            Err(mpsc::TryRecvError::Empty) => Ok(IngressPoll::Idle),
            Err(mpsc::TryRecvError::Disconnected) => {
                self.receiver = None;
                Ok(IngressPoll::Drained)
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn input(id: u64) -> RecordingInput {
        RecordingInput {
            connection_generation: 7,
            configuration: Arc::new(StreamConfig {
                codec_configuration: vec![1],
                stream_epoch: 2,
                ..Default::default()
            }),
            access_unit: AssembledAccessUnit {
                data: vec![1, 2, 3].into(),
                frame_id: id,
                pts_us: id * 33_333,
                encoded_at_us: 0,
                keyframe: false,
                discardable: false,
                stream_epoch: 2,
                fragment_count: 1,
                first_fragment_at: Instant::now(),
            },
        }
    }

    #[test]
    fn normal_stop_drains_accepted_arrival_order_without_copying_payload() {
        let (sender, mut inbox) = channel();
        let first = input(2);
        let bytes = first.access_unit.data.clone();
        sender.offer(first).unwrap();
        sender.offer(input(1)).unwrap();
        drop(sender);
        let IngressPoll::Input(first) = inbox.poll().unwrap() else {
            panic!()
        };
        assert_eq!(first.access_unit.frame_id, 2);
        assert_eq!(first.access_unit.data.as_ptr(), bytes.as_ptr());
        let IngressPoll::Input(second) = inbox.poll().unwrap() else {
            panic!()
        };
        assert_eq!(second.access_unit.frame_id, 1);
        assert!(matches!(inbox.poll().unwrap(), IngressPoll::Drained));
    }

    #[test]
    fn overflow_is_terminal_and_does_not_look_like_normal_stop() {
        let (sender, mut inbox) = channel();
        for id in 0..CAPACITY {
            sender.offer(input(id as u64)).unwrap();
        }
        assert_eq!(sender.offer(input(100)), Err(IngressFailure::Capacity));
        assert_eq!(inbox.poll().unwrap_err(), IngressFailure::Capacity);
        assert_eq!(sender.offer(input(101)), Err(IngressFailure::Capacity));
        drop(sender);
        assert_eq!(inbox.poll().unwrap_err(), IngressFailure::Capacity);
    }

    #[test]
    fn age_deadline_does_not_restart_on_poll_or_normal_stop() {
        let (sender, mut inbox) = channel();
        let start = Instant::now();
        sender.offer_at(input(1), start).unwrap();
        drop(sender);
        assert_eq!(
            inbox
                .poll_at(start + MAX_AGE + Duration::from_nanos(1))
                .unwrap_err(),
            IngressFailure::TooOld
        );
        assert_eq!(inbox.poll_at(start).unwrap_err(), IngressFailure::TooOld);
    }

    #[test]
    fn foreign_epoch_and_dead_worker_fail_explicitly() {
        let (sender, _inbox) = channel();
        let mut wrong = input(1);
        wrong.access_unit.stream_epoch = 3;
        assert_eq!(sender.offer(wrong), Err(IngressFailure::InvalidInput));
        let (sender, inbox) = channel();
        drop(inbox);
        assert_eq!(sender.offer(input(1)), Err(IngressFailure::WorkerStopped));
    }
}
