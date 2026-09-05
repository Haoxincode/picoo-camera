//! Latest-only asynchronous Shared Frame Ring output — REQ-PICOO-FRAME-011.

use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::mpsc::{self, Receiver};
use std::sync::{Arc, Condvar, Mutex};
use std::thread::{self, JoinHandle};

use super::{SharedFrameRingProducer, SharedRingError};
use crate::VideoFrame;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SharedRingSubmitOutcome {
    Queued,
    ReplacedPending,
    Stopped,
}

#[derive(Debug)]
pub enum SharedRingWriterEvent {
    Published {
        frame_sequence: u64,
        ring_sequence: u64,
    },
    Failed {
        frame_sequence: u64,
        error: SharedRingError,
    },
}

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct SharedRingWriterStats {
    pub submitted: u64,
    pub replaced_pending: u64,
    pub published: u64,
    pub failed: u64,
}

#[derive(Default)]
struct WriterCounters {
    submitted: AtomicU64,
    replaced_pending: AtomicU64,
    published: AtomicU64,
    failed: AtomicU64,
}

#[derive(Default)]
struct QueueState {
    pending: Option<Arc<VideoFrame>>,
    stopped: bool,
}

#[derive(Default)]
struct LatestFrameQueue {
    state: Mutex<QueueState>,
    ready: Condvar,
}

impl LatestFrameQueue {
    fn submit(&self, frame: Arc<VideoFrame>) -> SharedRingSubmitOutcome {
        let mut state = self
            .state
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        if state.stopped {
            return SharedRingSubmitOutcome::Stopped;
        }
        let replaced = state.pending.replace(frame).is_some();
        self.ready.notify_one();
        if replaced {
            SharedRingSubmitOutcome::ReplacedPending
        } else {
            SharedRingSubmitOutcome::Queued
        }
    }

    fn take(&self) -> Option<Arc<VideoFrame>> {
        let mut state = self
            .state
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        loop {
            if state.stopped {
                return None;
            }
            if let Some(frame) = state.pending.take() {
                return Some(frame);
            }
            state = self
                .ready
                .wait(state)
                .unwrap_or_else(std::sync::PoisonError::into_inner);
        }
    }

    fn stop(&self) {
        let mut state = self
            .state
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        state.stopped = true;
        state.pending = None;
        self.ready.notify_one();
    }
}

/// Capacity-one output adapter behind [`crate::LatestFrameStore`].
///
/// The Producer is constructed and remains on its worker thread, so its mmap
/// pointer is never made `Send`. A slow or failing cross-process consumer can
/// only replace pending output work; it cannot block Receiver session state.
pub struct SharedFrameRingWriter {
    queue: Arc<LatestFrameQueue>,
    counters: Arc<WriterCounters>,
    events: Receiver<SharedRingWriterEvent>,
    worker: Option<JoinHandle<()>>,
}

impl SharedFrameRingWriter {
    pub fn start(
        producer_factory: impl FnOnce() -> Result<SharedFrameRingProducer, SharedRingError>
            + Send
            + 'static,
    ) -> Result<Self, SharedRingError> {
        let queue = Arc::new(LatestFrameQueue::default());
        let worker_queue = Arc::clone(&queue);
        let counters = Arc::new(WriterCounters::default());
        let worker_counters = Arc::clone(&counters);
        let (event_sender, events) = mpsc::channel();
        let (startup_sender, startup_receiver) = mpsc::sync_channel(1);
        let worker = thread::Builder::new()
            .name("picoo-shared-ring-writer".into())
            .spawn(move || match producer_factory() {
                Ok(mut producer) => {
                    let _ = startup_sender.send(Ok(()));
                    while let Some(frame) = worker_queue.take() {
                        let result = producer.publish_nv12(
                            frame.width,
                            frame.height,
                            frame.stride,
                            frame.rotation,
                            frame.timestamp_us,
                            &frame.pixel_data,
                        );
                        let event = match result {
                            Ok(ring_sequence) => {
                                worker_counters.published.fetch_add(1, Ordering::Relaxed);
                                SharedRingWriterEvent::Published {
                                    frame_sequence: frame.sequence,
                                    ring_sequence,
                                }
                            }
                            Err(error) => {
                                worker_counters.failed.fetch_add(1, Ordering::Relaxed);
                                SharedRingWriterEvent::Failed {
                                    frame_sequence: frame.sequence,
                                    error,
                                }
                            }
                        };
                        let _ = event_sender.send(event);
                    }
                }
                Err(error) => {
                    let _ = startup_sender.send(Err(error));
                }
            })
            .map_err(|error| SharedRingError::Shmem(format!("start ring writer: {error}")))?;
        startup_receiver
            .recv()
            .map_err(|_| SharedRingError::Shmem("ring writer exited during startup".into()))??;
        Ok(Self {
            queue,
            counters,
            events,
            worker: Some(worker),
        })
    }

    pub fn submit(&self, frame: Arc<VideoFrame>) -> SharedRingSubmitOutcome {
        let outcome = self.queue.submit(frame);
        if outcome != SharedRingSubmitOutcome::Stopped {
            self.counters.submitted.fetch_add(1, Ordering::Relaxed);
        }
        if outcome == SharedRingSubmitOutcome::ReplacedPending {
            self.counters
                .replaced_pending
                .fetch_add(1, Ordering::Relaxed);
        }
        outcome
    }

    pub fn poll_event(&self) -> Option<SharedRingWriterEvent> {
        self.events.try_recv().ok()
    }

    pub fn stats(&self) -> SharedRingWriterStats {
        SharedRingWriterStats {
            submitted: self.counters.submitted.load(Ordering::Relaxed),
            replaced_pending: self.counters.replaced_pending.load(Ordering::Relaxed),
            published: self.counters.published.load(Ordering::Relaxed),
            failed: self.counters.failed.load(Ordering::Relaxed),
        }
    }
}

impl Drop for SharedFrameRingWriter {
    fn drop(&mut self) {
        self.queue.stop();
        if let Some(worker) = self.worker.take() {
            let _ = thread::Builder::new()
                .name("picoo-shared-ring-shutdown".into())
                .spawn(move || {
                    let _ = worker.join();
                });
        }
    }
}

#[cfg(test)]
mod tests {
    use std::time::Instant;

    use bytes::Bytes;

    use super::*;

    fn frame(sequence_marker: u64) -> Arc<VideoFrame> {
        let mut frame = VideoFrame::new(
            1,
            sequence_marker,
            0,
            0,
            0,
            0,
            Instant::now(),
            0,
            2,
            2,
            2,
            0,
            Bytes::from(vec![sequence_marker as u8; 6]),
        );
        frame.sequence = sequence_marker;
        Arc::new(frame)
    }

    #[test]
    fn pending_output_is_capacity_one_and_latest_wins() {
        let queue = LatestFrameQueue::default();
        assert_eq!(queue.submit(frame(1)), SharedRingSubmitOutcome::Queued);
        assert_eq!(
            queue.submit(frame(2)),
            SharedRingSubmitOutcome::ReplacedPending
        );
        assert_eq!(queue.take().expect("latest frame").sequence, 2);
    }
}
