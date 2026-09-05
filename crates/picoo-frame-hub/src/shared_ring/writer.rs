//! Latest-only asynchronous Shared Frame Ring output — REQ-PICOO-FRAME-011.

use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::mpsc::{self, Receiver};
use std::sync::{Arc, Condvar, Mutex};
use std::thread::{self, JoinHandle};
use std::time::{Duration, Instant};

use super::{RingPublishOutcome, SharedFrameRingProducer, SharedRingError};
use crate::VideoFrame;

const BUSY_RETRY_INTERVAL: Duration = Duration::from_millis(2);

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
    pub busy_retries: u64,
    pub failed: u64,
}

#[derive(Default)]
struct WriterCounters {
    submitted: AtomicU64,
    replaced_pending: AtomicU64,
    published: AtomicU64,
    busy_retries: AtomicU64,
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

    /// Retain a busy output as the capacity-one pending item. A newer submit
    /// replaces it and wakes immediately; otherwise the same latest frame is
    /// retried after a short non-spinning delay.
    fn retry_after(&self, frame: Arc<VideoFrame>, delay: Duration) -> Option<Arc<VideoFrame>> {
        let mut state = self
            .state
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        if state.stopped {
            return None;
        }
        if state.pending.is_some() {
            return state.pending.take();
        }
        state.pending = Some(frame);
        let deadline = Instant::now() + delay;
        loop {
            if state.stopped {
                state.pending = None;
                return None;
            }
            let remaining = deadline.saturating_duration_since(Instant::now());
            if remaining.is_zero() {
                return state.pending.take();
            }
            let (next_state, timeout) = self
                .ready
                .wait_timeout(state, remaining)
                .unwrap_or_else(std::sync::PoisonError::into_inner);
            state = next_state;
            if timeout.timed_out() {
                return state.pending.take();
            }
            if state.pending.is_some() {
                return state.pending.take();
            }
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
                    while let Some(mut frame) = worker_queue.take() {
                        loop {
                            let result = producer.publish_nv12(
                                frame.width,
                                frame.height,
                                frame.stride,
                                frame.rotation,
                                frame.timestamp_us,
                                &frame.pixel_data,
                            );
                            match result {
                                Ok(RingPublishOutcome::Published {
                                    sequence: ring_sequence,
                                }) => {
                                    worker_counters.published.fetch_add(1, Ordering::Relaxed);
                                    let _ = event_sender.send(SharedRingWriterEvent::Published {
                                        frame_sequence: frame.sequence,
                                        ring_sequence,
                                    });
                                    break;
                                }
                                Ok(RingPublishOutcome::Busy) => {
                                    worker_counters.busy_retries.fetch_add(1, Ordering::Relaxed);
                                    let Some(next) =
                                        worker_queue.retry_after(frame, BUSY_RETRY_INTERVAL)
                                    else {
                                        return;
                                    };
                                    frame = next;
                                }
                                Err(error) => {
                                    worker_counters.failed.fetch_add(1, Ordering::Relaxed);
                                    let _ = event_sender.send(SharedRingWriterEvent::Failed {
                                        frame_sequence: frame.sequence,
                                        error,
                                    });
                                    break;
                                }
                            }
                        }
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
            busy_retries: self.counters.busy_retries.load(Ordering::Relaxed),
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
    use std::time::{Duration, SystemTime, UNIX_EPOCH};

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
            sequence_marker,
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

    #[test]
    fn busy_output_is_retried_without_false_publish() {
        let suffix = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .expect("wall clock")
            .as_nanos();
        let name = format!("picoo-ring-writer-{}-{suffix}", std::process::id());
        let writer = SharedFrameRingWriter::start({
            let name = name.clone();
            move || SharedFrameRingProducer::create(&name, 6)
        })
        .expect("start writer");
        let consumer = super::super::SharedFrameRingConsumer::open(&name, 6).expect("consumer");

        writer.submit(frame(1));
        wait_for_publish(&writer, 1);
        let first = consumer.latest_frame().expect("first lease");
        writer.submit(frame(2));
        wait_for_publish(&writer, 2);
        let second = consumer.latest_frame().expect("second lease");
        writer.submit(frame(3));
        wait_for_publish(&writer, 3);
        let third = consumer.latest_frame().expect("third lease");

        writer.submit(frame(4));
        let deadline = Instant::now() + Duration::from_secs(1);
        while writer.stats().busy_retries == 0 && Instant::now() < deadline {
            std::thread::sleep(Duration::from_millis(1));
        }
        let busy_stats = writer.stats();
        assert!(busy_stats.busy_retries > 0, "writer never observed Busy");
        assert_eq!(busy_stats.published, 3, "Busy was counted as Published");
        assert!(
            writer.poll_event().is_none(),
            "Busy emitted a success event"
        );

        drop((first, second, third));
        wait_for_publish(&writer, 4);
        let latest = consumer.latest_frame().expect("retried frame");
        assert_eq!(latest.timestamp_us, 4);
        assert_eq!(latest.nv12, &[4; 6]);
    }

    fn wait_for_publish(writer: &SharedFrameRingWriter, expected_frame: u64) {
        let deadline = Instant::now() + Duration::from_secs(1);
        loop {
            match writer.poll_event() {
                Some(SharedRingWriterEvent::Published { frame_sequence, .. })
                    if frame_sequence == expected_frame =>
                {
                    return;
                }
                Some(SharedRingWriterEvent::Published { frame_sequence, .. }) => {
                    panic!("unexpected frame {frame_sequence}, expected {expected_frame}")
                }
                Some(SharedRingWriterEvent::Failed { error, .. }) => {
                    panic!("ring publish failed: {error}")
                }
                None if Instant::now() < deadline => {
                    std::thread::sleep(Duration::from_millis(1));
                }
                None => panic!("frame {expected_frame} was not published"),
            }
        }
    }
}
