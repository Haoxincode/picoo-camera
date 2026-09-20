//! Bounded asynchronous Decoder adapter — REQ-PICOO-MEDIA-018.

use std::collections::VecDeque;
use std::panic::{catch_unwind, AssertUnwindSafe};
use std::sync::mpsc::{self, Receiver, Sender};
use std::sync::{Arc, Condvar, Mutex};
use std::thread::{self, JoinHandle};
use std::time::Instant;

use bytes::Bytes;
use picoo_media_decode::{AccessUnitDecoder, DecodeError, DecodeOutcome};
use picoo_protocol::control::{Capabilities, StreamConfig};

use crate::media_scheduler::DecoderAdmission;

const MAX_PENDING_DECODE_JOBS: usize = 2;

pub(super) use picoo_media_decode::{AccessUnitTimeline, FrameKind};

#[derive(Debug)]
pub(super) struct EncodedAccessUnit {
    pub(super) connection_generation: u64,
    pub(super) stream_generation: u64,
    pub(super) frame_id: u64,
    pub(super) source_pts_us: u64,
    pub(super) encoded_at_us: u64,
    pub(super) received_at_us: u64,
    pub(super) decode_submitted_at_us: u64,
    pub(super) kind: FrameKind,
    pub(super) data: Bytes,
}

impl EncodedAccessUnit {
    pub(super) fn timeline(&self) -> AccessUnitTimeline {
        AccessUnitTimeline {
            connection_generation: self.connection_generation,
            stream_generation: self.stream_generation,
            frame_id: self.frame_id,
            source_pts_us: self.source_pts_us,
            encoded_at_us: self.encoded_at_us,
            received_at_us: self.received_at_us,
            decode_submitted_at_us: self.decode_submitted_at_us,
            kind: self.kind,
        }
    }
}

struct DecodeJob {
    config_revision: u64,
    access_unit: EncodedAccessUnit,
    stream_config: Option<Arc<StreamConfig>>,
    decoder_generation: u64,
}

enum WorkItem {
    Decode(DecodeJob),
    Reset,
    Shutdown,
}

#[derive(Default)]
struct QueueState {
    items: VecDeque<WorkItem>,
    stopped: bool,
    decoder_generation: u64,
}

struct WorkQueue {
    state: Mutex<QueueState>,
    ready: Condvar,
}

impl WorkQueue {
    fn new() -> Self {
        Self {
            state: Mutex::new(QueueState::default()),
            ready: Condvar::new(),
        }
    }

    fn submit(&self, job: DecodeJob) -> DecodeSubmitOutcome {
        let mut state = self
            .state
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner());
        if state.stopped {
            return DecodeSubmitOutcome::Dropped {
                requires_refresh: job.access_unit.kind.requires_refresh_when_dropped(),
            };
        }

        let pending = state
            .items
            .iter()
            .filter(|item| matches!(item, WorkItem::Decode(_)))
            .count();
        if job.access_unit.kind == FrameKind::Key {
            // A fresh IDR supersedes every not-yet-started AU. Preserve Reset
            // commands so the platform prediction state is cleared first.
            state
                .items
                .retain(|item| !matches!(item, WorkItem::Decode(_)));
        } else if pending >= MAX_PENDING_DECODE_JOBS {
            if job.access_unit.kind == FrameKind::ReferenceDelta {
                if let Some(index) = state.items.iter().position(|item| {
                    matches!(
                        item,
                        WorkItem::Decode(DecodeJob {
                            access_unit: EncodedAccessUnit {
                                kind: FrameKind::DiscardableDelta,
                                ..
                            },
                            ..
                        })
                    )
                }) {
                    state.items.remove(index);
                } else {
                    return DecodeSubmitOutcome::Dropped {
                        requires_refresh: true,
                    };
                }
            } else {
                return DecodeSubmitOutcome::Dropped {
                    requires_refresh: false,
                };
            }
        }

        let mut job = job;
        job.decoder_generation = state.decoder_generation;
        state.items.push_back(WorkItem::Decode(job));
        self.ready.notify_one();
        DecodeSubmitOutcome::Queued
    }

    fn admission(&self, kind: FrameKind) -> DecoderAdmission {
        let state = self
            .state
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner());
        if state.stopped || kind == FrameKind::Key {
            return DecoderAdmission::Ready;
        }
        let pending = state
            .items
            .iter()
            .filter(|item| matches!(item, WorkItem::Decode(_)))
            .count();
        if pending < MAX_PENDING_DECODE_JOBS {
            return DecoderAdmission::Ready;
        }
        if kind == FrameKind::ReferenceDelta
            && state.items.iter().any(|item| {
                matches!(
                    item,
                    WorkItem::Decode(DecodeJob {
                        access_unit: EncodedAccessUnit {
                            kind: FrameKind::DiscardableDelta,
                            ..
                        },
                        ..
                    })
                )
            })
        {
            DecoderAdmission::Ready
        } else if kind == FrameKind::DiscardableDelta {
            DecoderAdmission::DropDiscardable
        } else {
            DecoderAdmission::WaitForCapacity
        }
    }

    fn reset(&self) {
        let mut state = self
            .state
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner());
        if state.stopped {
            return;
        }
        state.decoder_generation = state.decoder_generation.wrapping_add(1);
        state
            .items
            .retain(|item| !matches!(item, WorkItem::Decode(_) | WorkItem::Reset));
        state.items.push_back(WorkItem::Reset);
        self.ready.notify_one();
    }

    fn shutdown(&self) {
        let mut state = self
            .state
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner());
        if state.stopped {
            return;
        }
        state.stopped = true;
        state.items.clear();
        state.items.push_back(WorkItem::Shutdown);
        self.ready.notify_one();
    }

    fn decoder_generation(&self) -> u64 {
        self.state
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner())
            .decoder_generation
    }

    fn pop(&self) -> WorkItem {
        let mut state = self
            .state
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner());
        loop {
            if let Some(item) = state.items.pop_front() {
                return item;
            }
            state = self
                .ready
                .wait(state)
                .unwrap_or_else(|poisoned| poisoned.into_inner());
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) enum DecodeSubmitOutcome {
    Queued,
    Dropped { requires_refresh: bool },
}

pub(super) enum DecoderEvent {
    Capabilities(Result<Capabilities, String>),
    Started,
    Completed {
        timeline: AccessUnitTimeline,
        decoder_generation: u64,
        decoded_at: Instant,
        decode_time_us: u64,
        result: Result<DecodeOutcome, DecodeError>,
    },
    Unavailable(String),
}

pub(super) struct DecoderWorker {
    queue: Arc<WorkQueue>,
    events: Receiver<DecoderEvent>,
    thread: Option<JoinHandle<()>>,
}

impl DecoderWorker {
    pub(super) fn with_event_wake(event_wake: picoo_transport::TransportEventWake) -> Self {
        #[cfg(all(
            test,
            not(target_vendor = "apple"),
            any(not(windows), feature = "windows-mf")
        ))]
        let factory =
            || picoo_media_decode::create_test_decoder().expect("explicit software test decoder");
        #[cfg(not(all(
            test,
            not(target_vendor = "apple"),
            any(not(windows), feature = "windows-mf")
        )))]
        let factory = picoo_media_decode::create_platform_decoder;
        #[cfg(test)]
        {
            Self::with_decoder_factory(factory, event_wake)
        }
        #[cfg(not(test))]
        {
            Self::with_preparation(factory, picoo_media_decode::probe_capabilities, event_wake)
        }
    }

    #[cfg(any(test, feature = "loopback-diagnostics"))]
    fn with_decoder_factory(
        factory: impl FnOnce() -> Box<dyn AccessUnitDecoder> + Send + 'static,
        event_wake: picoo_transport::TransportEventWake,
    ) -> Self {
        Self::with_preparation(
            factory,
            |_| Ok(super::decoder_capabilities::synthetic_capabilities()),
            event_wake,
        )
    }

    pub(super) fn with_preparation(
        factory: impl FnOnce() -> Box<dyn AccessUnitDecoder> + Send + 'static,
        prepare: impl FnOnce(&mut dyn AccessUnitDecoder) -> Result<Capabilities, DecodeError>
            + Send
            + 'static,
        event_wake: picoo_transport::TransportEventWake,
    ) -> Self {
        let queue = Arc::new(WorkQueue::new());
        let worker_queue = Arc::clone(&queue);
        let (event_sender, events) = mpsc::channel();
        let thread = thread::Builder::new()
            .name("picoo-decoder".into())
            .spawn(move || {
                let initialized = catch_unwind(AssertUnwindSafe(|| -> Result<_, DecodeError> {
                    let mut decoder = factory();
                    let prepare = || prepare(decoder.as_mut());
                    #[cfg(target_os = "macos")]
                    let prepare = || objc2::rc::autoreleasepool(|_| prepare());
                    let capabilities = prepare()?;
                    Ok((decoder, capabilities))
                }))
                .unwrap_or_else(|_| {
                    Err(DecodeError::Platform("decoder preparation panicked".into()))
                });
                let (decoder, capabilities) = match initialized {
                    Ok(ready) => ready,
                    Err(error) => {
                        worker_queue.shutdown();
                        let _ = send_decoder_event(
                            &event_sender,
                            &event_wake,
                            DecoderEvent::Capabilities(Err(error.to_string())),
                        );
                        return;
                    }
                };
                if send_decoder_event(
                    &event_sender,
                    &event_wake,
                    DecoderEvent::Capabilities(Ok(capabilities)),
                )
                .is_err()
                {
                    worker_queue.shutdown();
                    return;
                }
                run_worker(decoder, worker_queue, event_sender, event_wake)
            })
            .expect("start decoder worker");
        Self {
            queue,
            events,
            thread: Some(thread),
        }
    }

    #[cfg(any(test, feature = "loopback-diagnostics"))]
    pub(super) fn with_decoder(decoder: Box<dyn AccessUnitDecoder + Send>) -> Self {
        Self::with_decoder_factory(
            move || decoder,
            picoo_transport::TransportEventWake::default(),
        )
    }

    pub(super) fn submit(
        &self,
        access_unit: EncodedAccessUnit,
        stream_config: Option<Arc<StreamConfig>>,
        config_revision: u64,
    ) -> DecodeSubmitOutcome {
        self.queue.submit(DecodeJob {
            config_revision,
            access_unit,
            stream_config,
            decoder_generation: 0,
        })
    }

    /// Only the Receiver owner submits jobs. The Decoder worker can remove
    /// pending jobs between this check and submit, so capacity can only improve;
    /// no second producer can consume the observed admission.
    pub(super) fn admission(&self, kind: FrameKind) -> DecoderAdmission {
        self.queue.admission(kind)
    }

    pub(super) fn reset(&self) {
        self.queue.reset();
    }

    pub(super) fn poll_event(&self) -> Option<DecoderEvent> {
        self.events.try_recv().ok()
    }

    pub(super) fn is_current_generation(&self, generation: u64) -> bool {
        self.queue.decoder_generation() == generation
    }
}

impl Drop for DecoderWorker {
    fn drop(&mut self) {
        self.queue.shutdown();
        if let Some(thread) = self.thread.take() {
            // Platform decode calls are outside Picoo's control. Never let a
            // stuck codec call turn Receiver teardown into an unbounded wait.
            if thread.is_finished() {
                let _ = thread.join();
            }
        }
    }
}

fn run_worker(
    mut decoder: Box<dyn AccessUnitDecoder>,
    queue: Arc<WorkQueue>,
    events: Sender<DecoderEvent>,
    event_wake: picoo_transport::TransportEventWake,
) {
    loop {
        let item = queue.pop();
        #[cfg(target_os = "macos")]
        let should_stop = objc2::rc::autoreleasepool(|_| {
            process_work_item(item, &mut decoder, &events, &event_wake)
        });
        #[cfg(not(target_os = "macos"))]
        let should_stop = process_work_item(item, &mut decoder, &events, &event_wake);
        if should_stop {
            queue.shutdown();
            return;
        }
    }
}

fn process_work_item(
    item: WorkItem,
    decoder: &mut Box<dyn AccessUnitDecoder>,
    events: &Sender<DecoderEvent>,
    event_wake: &picoo_transport::TransportEventWake,
) -> bool {
    match item {
        WorkItem::Decode(job) => {
            let timeline = job.access_unit.timeline();
            if send_decoder_event(events, event_wake, DecoderEvent::Started).is_err() {
                return true;
            }
            let started = Instant::now();
            let decode = catch_unwind(AssertUnwindSafe(|| {
                decoder.submit(picoo_media_decode::DecodeSubmission {
                    access_unit: &job.access_unit.data,
                    token: Arc::new(picoo_media_decode::DecodeToken {
                        timeline,
                        config_revision: job.config_revision,
                        decoder_generation: job.decoder_generation,
                        stream_config: job.stream_config.clone(),
                    }),
                })
            }));
            let result = match decode {
                Ok(result) => result,
                Err(_) => {
                    let _ = send_decoder_event(
                        events,
                        event_wake,
                        DecoderEvent::Unavailable(
                            "platform decoder panicked on worker thread".into(),
                        ),
                    );
                    return true;
                }
            };
            let decoded_at = Instant::now();
            send_decoder_event(
                events,
                event_wake,
                DecoderEvent::Completed {
                    timeline,
                    decoder_generation: job.decoder_generation,
                    decoded_at,
                    decode_time_us: started.elapsed().as_micros() as u64,
                    result,
                },
            )
            .is_err()
        }
        WorkItem::Reset => {
            let reset = catch_unwind(AssertUnwindSafe(|| decoder.reset())).unwrap_or_else(|_| {
                Err(DecodeError::Platform(
                    "platform decoder panicked while resetting".into(),
                ))
            });
            if let Err(error) = reset {
                let _ = send_decoder_event(
                    events,
                    event_wake,
                    DecoderEvent::Unavailable(error.to_string()),
                );
                return true;
            }
            false
        }
        WorkItem::Shutdown => true,
    }
}

fn send_decoder_event(
    events: &Sender<DecoderEvent>,
    event_wake: &picoo_transport::TransportEventWake,
    event: DecoderEvent,
) -> Result<(), ()> {
    events.send(event).map_err(|_| ())?;
    event_wake.signal();
    Ok(())
}

#[cfg(test)]
#[path = "decoder_worker_tests.rs"]
mod tests;
