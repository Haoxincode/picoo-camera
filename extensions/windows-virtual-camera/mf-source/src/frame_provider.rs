//! Shared Frame Ring consumer and disconnect placeholder policy.

use std::io;
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::mpsc::{self, TrySendError};
use std::sync::{Arc, Condvar, Mutex, RwLock};
use std::thread::{self, JoinHandle};
use std::time::{Duration, Instant};

#[cfg(test)]
use picoo_frame_hub::{waiting_placeholder, PLACEHOLDER_HEIGHT, PLACEHOLDER_WIDTH};
use picoo_frame_hub::{
    waiting_placeholder_for_size, SharedFrameRingConsumer, DEFAULT_MAX_FRAME_BYTES,
};

use crate::{format::nv12_len, DEFAULT_RING_NAME};

const LAST_FRAME_HOLD: Duration = Duration::from_millis(500);
const GENERATION_PROBE_INTERVAL: Duration = Duration::from_millis(250);
const RING_POLL_INTERVAL: Duration = Duration::from_millis(5);

#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) struct OwnedNv12Frame {
    pub width: u32,
    pub height: u32,
    pub stride: u32,
    pub pixels: Arc<[u8]>,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum FrameOrigin {
    Fresh,
    Cached,
    Placeholder,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) struct AcquiredNv12Frame {
    pub frame: OwnedNv12Frame,
    pub origin: FrameOrigin,
}

struct RingFrameReader {
    ring_name: String,
    consumer: Option<SharedFrameRingConsumer>,
    last_sequence: u64,
    live_revision: u64,
    last_live: Option<OwnedNv12Frame>,
    last_live_at: Option<Instant>,
    next_generation_probe: Instant,
    producer_alive: bool,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum SourceKey {
    Live(u64),
    Placeholder,
}

struct SourceSnapshot {
    key: SourceKey,
    frame: Option<OwnedNv12Frame>,
}

impl RingFrameReader {
    #[cfg_attr(not(windows), allow(dead_code))]
    fn new() -> Self {
        Self {
            ring_name: DEFAULT_RING_NAME.to_owned(),
            consumer: None,
            last_sequence: 0,
            live_revision: 0,
            last_live: None,
            last_live_at: None,
            next_generation_probe: Instant::now(),
            producer_alive: false,
        }
    }

    #[cfg(test)]
    pub fn acquire(&mut self) -> AcquiredNv12Frame {
        let origin = self.refresh_source();
        let frame = match origin {
            FrameOrigin::Fresh | FrameOrigin::Cached => self
                .last_live
                .as_ref()
                .expect("live origin requires a complete frame")
                .clone(),
            FrameOrigin::Placeholder => OwnedNv12Frame {
                width: PLACEHOLDER_WIDTH,
                height: PLACEHOLDER_HEIGHT,
                stride: PLACEHOLDER_WIDTH,
                pixels: waiting_placeholder().into(),
            },
        };
        AcquiredNv12Frame { frame, origin }
    }

    fn refresh_source(&mut self) -> FrameOrigin {
        let now = Instant::now();
        if now >= self.next_generation_probe {
            self.next_generation_probe = now + GENERATION_PROBE_INTERVAL;
            if self
                .consumer
                .as_ref()
                .is_some_and(|consumer| !consumer.is_current_generation())
            {
                // REQ-PICOO-FRAME-007: a new Receiver mapping starts its
                // sequence at one, so detach must reset deduplication.
                self.consumer = None;
                self.last_sequence = 0;
                self.producer_alive = false;
            }
            if self.consumer.is_none() {
                self.consumer = self.open_consumer().ok();
            }
            #[cfg(windows)]
            {
                self.producer_alive = self
                    .consumer
                    .as_ref()
                    .is_some_and(SharedFrameRingConsumer::has_live_producer);
            }
            #[cfg(not(windows))]
            {
                self.producer_alive = self.consumer.is_some();
            }
        }

        if let Some(consumer) = &self.consumer {
            if let Some(view) = consumer.latest_frame() {
                let expected = nv12_len(view.width, view.height);
                let valid = view.sequence != self.last_sequence
                    && view.stride == view.width
                    && expected == Some(view.nv12.len());
                if valid {
                    let frame = OwnedNv12Frame {
                        width: view.width,
                        height: view.height,
                        stride: view.stride,
                        pixels: view.nv12.to_vec().into(),
                    };
                    self.last_sequence = view.sequence;
                    self.live_revision = self.live_revision.wrapping_add(1).max(1);
                    self.last_live = Some(frame);
                    self.last_live_at = Some(Instant::now());
                    return FrameOrigin::Fresh;
                }
            }
        }

        if self.last_live.is_some() {
            // REQ-PICOO-FRAME-005: Receiver owns connection semantics and
            // publishes an explicit reconnect/idle frame into the ring. A
            // live Producer with a temporarily unchanged sequence is not a
            // disconnect and must retain the last complete frame.
            if self.producer_alive
                || self
                    .last_live_at
                    .is_some_and(|at| at.elapsed() < LAST_FRAME_HOLD)
            {
                return FrameOrigin::Cached;
            }
        }
        FrameOrigin::Placeholder
    }

    fn snapshot(&mut self) -> SourceSnapshot {
        match self.refresh_source() {
            FrameOrigin::Fresh | FrameOrigin::Cached => SourceSnapshot {
                key: SourceKey::Live(self.live_revision),
                frame: self.last_live.clone(),
            },
            FrameOrigin::Placeholder => SourceSnapshot {
                key: SourceKey::Placeholder,
                frame: None,
            },
        }
    }

    fn open_consumer(&self) -> Result<SharedFrameRingConsumer, picoo_frame_hub::SharedRingError> {
        #[cfg(windows)]
        if self.ring_name == DEFAULT_RING_NAME {
            return SharedFrameRingConsumer::open_file(
                picoo_frame_hub::windows_shared_ring_path(&self.ring_name),
                DEFAULT_MAX_FRAME_BYTES,
            );
        }
        SharedFrameRingConsumer::open(&self.ring_name, DEFAULT_MAX_FRAME_BYTES)
    }

    #[cfg(test)]
    fn with_ring_name(ring_name: String) -> Self {
        Self {
            ring_name,
            consumer: None,
            last_sequence: 0,
            live_revision: 0,
            last_live: None,
            last_live_at: None,
            next_generation_probe: Instant::now(),
            producer_alive: false,
        }
    }
}

struct PreparedFrameSet {
    key: SourceKey,
    output_480: OwnedNv12Frame,
    output_720: OwnedNv12Frame,
    output_1080: OwnedNv12Frame,
}

impl PreparedFrameSet {
    fn placeholder() -> Self {
        Self {
            key: SourceKey::Placeholder,
            output_480: placeholder_for_size(854, 480),
            output_720: placeholder_for_size(1280, 720),
            output_1080: placeholder_for_size(1920, 1080),
        }
    }

    fn from_live(key: SourceKey, source: &OwnedNv12Frame) -> Self {
        debug_assert!(matches!(key, SourceKey::Live(_)));
        Self {
            key,
            output_480: prepare_for_size(source, 854, 480),
            output_720: prepare_for_size(source, 1280, 720),
            output_1080: prepare_for_size(source, 1920, 1080),
        }
    }

    fn output(&self, width: u32, height: u32) -> Option<OwnedNv12Frame> {
        match (width, height) {
            (854, 480) => Some(self.output_480.clone()),
            (1280, 720) => Some(self.output_720.clone()),
            (1920, 1080) => Some(self.output_1080.clone()),
            _ => None,
        }
    }
}

fn placeholder_for_size(width: u32, height: u32) -> OwnedNv12Frame {
    OwnedNv12Frame {
        width,
        height,
        stride: width,
        pixels: waiting_placeholder_for_size(width, height).into(),
    }
}

fn prepare_for_size(source: &OwnedNv12Frame, width: u32, height: u32) -> OwnedNv12Frame {
    fit_nv12(source, width, height).unwrap_or_else(|| OwnedNv12Frame {
        width,
        height,
        stride: width,
        pixels: picoo_frame_hub::nv12_black(width, height).into(),
    })
}

#[derive(Default)]
struct WorkerControl {
    stopped: Mutex<bool>,
    wake: Condvar,
}

impl WorkerControl {
    fn wait_for_poll(&self) -> bool {
        let stopped = self
            .stopped
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        let (stopped, _) = self
            .wake
            .wait_timeout_while(stopped, RING_POLL_INTERVAL, |stopped| !*stopped)
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        *stopped
    }

    fn stop(&self) {
        *self
            .stopped
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner) = true;
        self.wake.notify_all();
    }
}

struct WorkerHandles {
    ring_reader: JoinHandle<()>,
    output_preparation: JoinHandle<()>,
}

/// RequestSample-facing cache. Ring access and pixel preparation are owned by
/// two background workers; acquiring a frame only clones already prepared Arc
/// storage under a short pointer lock (REQ-PICOO-VCAM-010).
pub(crate) struct FrameProvider {
    prepared: Arc<RwLock<Arc<PreparedFrameSet>>>,
    last_delivered_live_revision: AtomicU64,
    control: Arc<WorkerControl>,
    workers: Mutex<Option<WorkerHandles>>,
}

impl FrameProvider {
    #[cfg_attr(not(windows), allow(dead_code))]
    pub(crate) fn new() -> io::Result<Self> {
        Self::with_reader(RingFrameReader::new())
    }

    fn with_reader(reader: RingFrameReader) -> io::Result<Self> {
        let placeholder = Arc::new(PreparedFrameSet::placeholder());
        let prepared = Arc::new(RwLock::new(Arc::clone(&placeholder)));
        let control = Arc::new(WorkerControl::default());
        let (source_tx, source_rx) = mpsc::sync_channel::<SourceSnapshot>(0);

        let prepared_for_worker = Arc::clone(&prepared);
        let placeholder_for_worker = Arc::clone(&placeholder);
        let output_preparation = thread::Builder::new()
            .name("picoo-vcam-output-preparation".into())
            .spawn(move || {
                while let Ok(snapshot) = source_rx.recv() {
                    let next = match (snapshot.key, snapshot.frame.as_ref()) {
                        (SourceKey::Live(_), Some(frame)) => {
                            Arc::new(PreparedFrameSet::from_live(snapshot.key, frame))
                        }
                        _ => Arc::clone(&placeholder_for_worker),
                    };
                    *prepared_for_worker
                        .write()
                        .unwrap_or_else(std::sync::PoisonError::into_inner) = next;
                }
            })?;

        let control_for_worker = Arc::clone(&control);
        let ring_reader = match thread::Builder::new()
            .name("picoo-vcam-ring-reader".into())
            .spawn(move || run_ring_reader(reader, source_tx, &control_for_worker))
        {
            Ok(worker) => worker,
            Err(error) => {
                control.stop();
                let _ = output_preparation.join();
                return Err(error);
            }
        };

        Ok(Self {
            prepared,
            last_delivered_live_revision: AtomicU64::new(0),
            control,
            workers: Mutex::new(Some(WorkerHandles {
                ring_reader,
                output_preparation,
            })),
        })
    }

    pub(crate) fn acquire_for_output(&self, width: u32, height: u32) -> Option<AcquiredNv12Frame> {
        let prepared = Arc::clone(
            &self
                .prepared
                .read()
                .unwrap_or_else(std::sync::PoisonError::into_inner),
        );
        let frame = prepared.output(width, height)?;
        let origin = match prepared.key {
            SourceKey::Placeholder => {
                self.last_delivered_live_revision
                    .store(0, Ordering::Release);
                FrameOrigin::Placeholder
            }
            SourceKey::Live(revision) => {
                let previous = self
                    .last_delivered_live_revision
                    .swap(revision, Ordering::AcqRel);
                if previous == revision {
                    FrameOrigin::Cached
                } else {
                    FrameOrigin::Fresh
                }
            }
        };
        Some(AcquiredNv12Frame { frame, origin })
    }

    pub(crate) fn shutdown(&self) {
        self.control.stop();
        let workers = self
            .workers
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
            .take();
        if let Some(workers) = workers {
            let _ = workers.ring_reader.join();
            let _ = workers.output_preparation.join();
        }
    }

    #[cfg(test)]
    pub(crate) fn with_ring_name(ring_name: String) -> io::Result<Self> {
        Self::with_reader(RingFrameReader::with_ring_name(ring_name))
    }
}

impl Drop for FrameProvider {
    fn drop(&mut self) {
        self.shutdown();
    }
}

fn run_ring_reader(
    mut reader: RingFrameReader,
    source_tx: mpsc::SyncSender<SourceSnapshot>,
    control: &WorkerControl,
) {
    let mut last_sent = SourceKey::Placeholder;
    loop {
        let snapshot = reader.snapshot();
        if snapshot.key != last_sent {
            let key = snapshot.key;
            match source_tx.try_send(snapshot) {
                Ok(()) => last_sent = key,
                Err(TrySendError::Full(_)) => {}
                Err(TrySendError::Disconnected(_)) => break,
            }
        }
        if control.wait_for_poll() {
            break;
        }
    }
}

fn fit_nv12(
    frame: &OwnedNv12Frame,
    output_width: u32,
    output_height: u32,
) -> Option<OwnedNv12Frame> {
    if frame.width == output_width && frame.height == output_height && frame.stride == frame.width {
        return Some(frame.clone());
    }
    if frame.width < 2
        || frame.height < 2
        || output_width < 2
        || output_height < 2
        || !frame.width.is_multiple_of(2)
        || !frame.height.is_multiple_of(2)
        || !output_width.is_multiple_of(2)
        || !output_height.is_multiple_of(2)
        || frame.stride < frame.width
        || frame.pixels.len() < (frame.stride as usize * frame.height as usize * 3 / 2)
    {
        return None;
    }

    let source_wider = u64::from(frame.width) * u64::from(output_height)
        > u64::from(output_width) * u64::from(frame.height);
    let (fit_width, fit_height) = if source_wider {
        let height =
            (u64::from(output_width) * u64::from(frame.height) / u64::from(frame.width)) as u32;
        (output_width, height.max(2) & !1)
    } else {
        let width =
            (u64::from(output_height) * u64::from(frame.width) / u64::from(frame.height)) as u32;
        (width.max(2) & !1, output_height)
    };
    let offset_x = ((output_width - fit_width) / 2) & !1;
    let offset_y = ((output_height - fit_height) / 2) & !1;
    let mut pixels = picoo_frame_hub::nv12_black(output_width, output_height);
    let src_stride = frame.stride as usize;
    let dst_stride = output_width as usize;
    let src_y_len = src_stride * frame.height as usize;
    let dst_y_len = dst_stride * output_height as usize;

    for dy in 0..fit_height {
        let sy = (u64::from(dy) * u64::from(frame.height) / u64::from(fit_height)) as usize;
        for dx in 0..fit_width {
            let sx = (u64::from(dx) * u64::from(frame.width) / u64::from(fit_width)) as usize;
            let destination = (offset_y + dy) as usize * dst_stride + (offset_x + dx) as usize;
            pixels[destination] = frame.pixels[sy * src_stride + sx];
        }
    }

    for dy in (0..fit_height).step_by(2) {
        let sy = (u64::from(dy) * u64::from(frame.height) / u64::from(fit_height)) as usize & !1;
        for dx in (0..fit_width).step_by(2) {
            let sx = (u64::from(dx) * u64::from(frame.width) / u64::from(fit_width)) as usize & !1;
            let source = src_y_len + (sy / 2) * src_stride + sx;
            let destination =
                dst_y_len + ((offset_y + dy) / 2) as usize * dst_stride + (offset_x + dx) as usize;
            pixels[destination] = frame.pixels[source];
            pixels[destination + 1] = frame.pixels[source + 1];
        }
    }

    Some(OwnedNv12Frame {
        width: output_width,
        height: output_height,
        stride: output_width,
        pixels: pixels.into(),
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use picoo_frame_hub::SharedFrameRingProducer;

    fn test_ring_name() -> String {
        format!(
            "frame-provider-{}",
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .expect("clock")
                .as_nanos()
        )
    }

    #[test]
    fn starts_with_shared_branded_placeholder() {
        let provider =
            FrameProvider::with_ring_name(test_ring_name()).expect("isolated frame provider");
        let acquired = provider
            .acquire_for_output(1280, 720)
            .expect("supported output");
        let frame = acquired.frame;
        assert_eq!(acquired.origin, FrameOrigin::Placeholder);
        assert_eq!((frame.width, frame.height), (1280, 720));
        assert_eq!(
            frame.pixels.as_ref(),
            waiting_placeholder_for_size(1280, 720).as_slice()
        );

        let negotiated = provider
            .acquire_for_output(1920, 1080)
            .expect("supported output");
        assert_eq!(negotiated.origin, FrameOrigin::Placeholder);
        assert_eq!(
            (negotiated.frame.width, negotiated.frame.height),
            (1920, 1080)
        );
        assert_eq!(
            negotiated.frame.pixels.as_ref(),
            waiting_placeholder_for_size(1920, 1080).as_slice()
        );
        let cached_pixels = negotiated.frame.pixels.as_ptr();
        let repeated = provider
            .acquire_for_output(1920, 1080)
            .expect("supported output");
        assert_eq!(repeated.origin, FrameOrigin::Placeholder);
        assert_eq!(
            repeated.frame.pixels.as_ptr(),
            cached_pixels,
            "cache hits must share immutable pixels instead of cloning the frame"
        );
        assert!(provider.acquire_for_output(640, 480).is_none());
    }

    #[test]
    fn reconnects_to_new_mapping_generation_even_when_sequence_restarts() {
        let ring_name = test_ring_name();
        let frame_len = nv12_len(PLACEHOLDER_WIDTH, PLACEHOLDER_HEIGHT).expect("NV12 size");
        let first_pixels = vec![1; frame_len];
        let second_pixels = vec![2; frame_len];
        let mut first_producer =
            SharedFrameRingProducer::create(&ring_name, DEFAULT_MAX_FRAME_BYTES)
                .expect("first producer");
        first_producer
            .publish_nv12(
                PLACEHOLDER_WIDTH,
                PLACEHOLDER_HEIGHT,
                PLACEHOLDER_WIDTH,
                0,
                1,
                &first_pixels,
            )
            .expect("first frame");
        let mut provider = RingFrameReader::with_ring_name(ring_name.clone());
        let acquired = provider.acquire();
        assert_eq!(acquired.origin, FrameOrigin::Fresh);
        assert_eq!(acquired.frame.pixels.as_ref(), first_pixels.as_slice());

        provider.last_live_at = Some(Instant::now() - LAST_FRAME_HOLD);
        let acquired = provider.acquire();
        assert_eq!(acquired.origin, FrameOrigin::Cached);
        assert_eq!(
            acquired.frame.pixels.as_ref(),
            first_pixels.as_slice(),
            "a live producer with an unchanged sequence is not disconnected"
        );
        provider.last_live_at = Some(Instant::now());

        drop(first_producer);
        provider.next_generation_probe = Instant::now();
        assert_eq!(
            provider.acquire().frame.pixels.as_ref(),
            first_pixels.as_slice(),
            "brief generation gap keeps the last complete frame"
        );
        provider.last_live_at = Some(Instant::now() - LAST_FRAME_HOLD);
        assert_eq!(
            provider.acquire().frame.pixels.as_ref(),
            waiting_placeholder().as_slice(),
            "an extended generation gap falls back to the placeholder"
        );

        let mut second_producer =
            SharedFrameRingProducer::create(&ring_name, DEFAULT_MAX_FRAME_BYTES)
                .expect("second producer");
        second_producer
            .publish_nv12(
                PLACEHOLDER_WIDTH,
                PLACEHOLDER_HEIGHT,
                PLACEHOLDER_WIDTH,
                0,
                2,
                &second_pixels,
            )
            .expect("second generation frame");

        provider.next_generation_probe = Instant::now();
        let acquired = provider.acquire();
        assert_eq!(acquired.origin, FrameOrigin::Fresh);
        assert_eq!(acquired.frame.pixels.as_ref(), second_pixels.as_slice());

        drop((provider, second_producer));
        let _ = std::fs::remove_file(SharedFrameRingProducer::flink_path(&ring_name));
    }

    #[test]
    fn background_workers_publish_latest_prepared_frames() {
        let ring_name = test_ring_name();
        let frame_len = nv12_len(1280, 720).expect("NV12 size");
        let pixels = vec![73; frame_len];
        let mut producer =
            SharedFrameRingProducer::create(&ring_name, DEFAULT_MAX_FRAME_BYTES).expect("producer");
        producer
            .publish_nv12(1280, 720, 1280, 0, 1, &pixels)
            .expect("publish");
        let provider = FrameProvider::with_ring_name(ring_name.clone()).expect("provider");

        let deadline = Instant::now() + Duration::from_secs(2);
        let prepared = loop {
            let acquired = provider
                .acquire_for_output(1280, 720)
                .expect("supported output");
            if acquired.frame.pixels.as_ref() == pixels.as_slice() {
                assert_eq!(acquired.origin, FrameOrigin::Fresh);
                break acquired.frame;
            }
            assert_eq!(acquired.origin, FrameOrigin::Placeholder);
            assert!(
                Instant::now() < deadline,
                "workers did not prepare live frame"
            );
            thread::sleep(Duration::from_millis(5));
        };

        let repeated = provider
            .acquire_for_output(1280, 720)
            .expect("supported output");
        assert_eq!(repeated.origin, FrameOrigin::Cached);
        assert_eq!(
            repeated.frame.pixels.as_ptr(),
            prepared.pixels.as_ptr(),
            "RequestSample cache hits must only clone the prepared Arc"
        );
        assert_eq!(
            provider
                .acquire_for_output(854, 480)
                .expect("480p")
                .frame
                .pixels
                .len(),
            nv12_len(854, 480).expect("480p size")
        );
        assert_eq!(
            provider
                .acquire_for_output(1920, 1080)
                .expect("1080p")
                .frame
                .pixels
                .len(),
            nv12_len(1920, 1080).expect("1080p size")
        );

        provider.shutdown();
        drop(producer);
        let _ = std::fs::remove_file(SharedFrameRingProducer::flink_path(&ring_name));
    }

    #[test]
    fn worker_shutdown_is_idempotent_and_keeps_last_prepared_frame_readable() {
        let provider =
            FrameProvider::with_ring_name(test_ring_name()).expect("isolated frame provider");
        let before = provider
            .acquire_for_output(1280, 720)
            .expect("supported output");

        provider.shutdown();
        provider.shutdown();

        let after = provider
            .acquire_for_output(1280, 720)
            .expect("prepared cache remains valid");
        assert_eq!(after.origin, FrameOrigin::Placeholder);
        assert_eq!(after.frame.pixels.as_ptr(), before.frame.pixels.as_ptr());
    }

    #[test]
    fn negotiated_output_shape_is_stable_and_letterboxes_input() {
        let source = OwnedNv12Frame {
            width: 4,
            height: 4,
            stride: 4,
            pixels: {
                let mut pixels = vec![80; nv12_len(4, 4).expect("source size")];
                pixels[16..].fill(128);
                pixels.into()
            },
        };
        let fitted = fit_nv12(&source, 8, 4).expect("fit");
        assert_eq!((fitted.width, fitted.height, fitted.stride), (8, 4, 8));
        assert_eq!(fitted.pixels[0], 0, "left pillar is black");
        assert_eq!(fitted.pixels[2], 80, "source is centered");
        assert_eq!(fitted.pixels[7], 0, "right pillar is black");
    }
}
