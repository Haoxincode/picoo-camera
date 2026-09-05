//! Shared Frame Ring consumer and disconnect placeholder policy.

use std::io;
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::mpsc::{self, TrySendError};
use std::sync::{Arc, Condvar, Mutex, RwLock};
use std::thread::{self, JoinHandle};
use std::time::{Duration, Instant};

#[cfg(test)]
use picoo_frame_hub::{waiting_placeholder, PLACEHOLDER_HEIGHT, PLACEHOLDER_WIDTH};
use picoo_frame_hub::{SharedFrameRingConsumer, DEFAULT_MAX_FRAME_BYTES};

use crate::{format::nv12_len, DEFAULT_RING_NAME};

mod preparation;
use preparation::{
    PlaceholderFrames, PreparationCounters, PreparationResources, PreparedFrameSet, PreparedFrames,
};

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
    output: OutputSize,
    demand_revision: u64,
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

    fn snapshot(&mut self, output: OutputSize, demand_revision: u64) -> SourceSnapshot {
        match self.refresh_source() {
            FrameOrigin::Fresh | FrameOrigin::Cached => SourceSnapshot {
                key: SourceKey::Live(self.live_revision),
                frame: self.last_live.clone(),
                output,
                demand_revision,
            },
            FrameOrigin::Placeholder => SourceSnapshot {
                key: SourceKey::Placeholder,
                frame: None,
                output,
                demand_revision,
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

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
struct OutputSize {
    width: u32,
    height: u32,
}

impl OutputSize {
    fn new(width: u32, height: u32) -> Option<Self> {
        matches!((width, height), (854, 480) | (1280, 720) | (1920, 1080))
            .then_some(Self { width, height })
    }

    const fn bit(self) -> u8 {
        1 << self.slot()
    }

    const fn slot(self) -> usize {
        match (self.width, self.height) {
            (854, 480) => 0,
            (1280, 720) => 1,
            (1920, 1080) => 2,
            _ => 3,
        }
    }
}

const OUTPUT_SIZES: [OutputSize; 3] = [
    OutputSize {
        width: 854,
        height: 480,
    },
    OutputSize {
        width: 1280,
        height: 720,
    },
    OutputSize {
        width: 1920,
        height: 1080,
    },
];

#[derive(Default)]
struct WorkerState {
    stopped: bool,
    active_outputs: u8,
    demand_revision: u64,
}

#[derive(Default)]
struct WorkerControl {
    state: Mutex<WorkerState>,
    wake: Condvar,
}

impl WorkerControl {
    fn wait_until_active(&self) -> Option<(u8, u64)> {
        let mut state = self
            .state
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        while !state.stopped && state.active_outputs == 0 {
            state = self
                .wake
                .wait(state)
                .unwrap_or_else(std::sync::PoisonError::into_inner);
        }
        (!state.stopped).then_some((state.active_outputs, state.demand_revision))
    }

    fn wait_for_poll(&self, demand_revision: u64) -> bool {
        let state = self
            .state
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        let (state, _) = self
            .wake
            .wait_timeout_while(state, RING_POLL_INTERVAL, |state| {
                !state.stopped
                    && state.active_outputs != 0
                    && state.demand_revision == demand_revision
            })
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        state.stopped
    }

    fn set_output_active(&self, output: OutputSize, active: bool) -> bool {
        let mut state = self
            .state
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        if state.stopped {
            return false;
        }
        let previous = state.active_outputs;
        if active {
            state.active_outputs |= output.bit();
        } else {
            state.active_outputs &= !output.bit();
        }
        if state.active_outputs == previous {
            return false;
        }
        state.demand_revision = state.demand_revision.wrapping_add(1).max(1);
        self.wake.notify_all();
        true
    }

    fn is_current_demand(&self, output: OutputSize, demand_revision: u64) -> bool {
        let state = self
            .state
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        !state.stopped
            && state.active_outputs & output.bit() != 0
            && state.demand_revision == demand_revision
    }

    fn stop(&self) {
        *self
            .state
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner) = WorkerState {
            stopped: true,
            active_outputs: 0,
            demand_revision: 0,
        };
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
    prepared: Arc<RwLock<PreparedFrames>>,
    placeholders: Arc<PlaceholderFrames>,
    last_delivered_live_revisions: [AtomicU64; 3],
    control: Arc<WorkerControl>,
    #[cfg(test)]
    preparation_counters: Arc<PreparationCounters>,
    workers: Mutex<Option<WorkerHandles>>,
}

impl FrameProvider {
    #[cfg_attr(not(windows), allow(dead_code))]
    pub(crate) fn new() -> io::Result<Self> {
        Self::with_reader(RingFrameReader::new())
    }

    fn with_reader(reader: RingFrameReader) -> io::Result<Self> {
        let placeholders = Arc::new(PlaceholderFrames::new());
        let prepared = Arc::new(RwLock::new(PreparedFrames::new(&placeholders)));
        let control = Arc::new(WorkerControl::default());
        let preparation_counters = Arc::new(PreparationCounters::default());
        let (source_tx, source_rx) = mpsc::sync_channel::<SourceSnapshot>(0);

        let prepared_for_worker = Arc::clone(&prepared);
        let placeholders_for_worker = Arc::clone(&placeholders);
        let control_for_preparation = Arc::clone(&control);
        let counters_for_worker = Arc::clone(&preparation_counters);
        let output_preparation = thread::Builder::new()
            .name("picoo-vcam-output-preparation".into())
            .spawn(move || {
                let mut resources = PreparationResources::default();
                while let Ok(snapshot) = source_rx.recv() {
                    if !control_for_preparation
                        .is_current_demand(snapshot.output, snapshot.demand_revision)
                    {
                        continue;
                    }
                    if prepared_for_worker
                        .read()
                        .unwrap_or_else(std::sync::PoisonError::into_inner)
                        .get(snapshot.output)
                        .key
                        == snapshot.key
                    {
                        continue;
                    }
                    let next = match (snapshot.key, snapshot.frame.as_ref()) {
                        (SourceKey::Live(_), Some(frame)) => {
                            counters_for_worker.record(snapshot.output);
                            Arc::new(PreparedFrameSet::from_live(
                                snapshot.key,
                                frame,
                                snapshot.output,
                                &mut resources,
                            ))
                        }
                        _ => placeholders_for_worker.get(snapshot.output),
                    };
                    if !control_for_preparation
                        .is_current_demand(snapshot.output, snapshot.demand_revision)
                    {
                        continue;
                    }
                    prepared_for_worker
                        .write()
                        .unwrap_or_else(std::sync::PoisonError::into_inner)
                        .set(snapshot.output, next);
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
            placeholders,
            last_delivered_live_revisions: std::array::from_fn(|_| AtomicU64::new(0)),
            control,
            #[cfg(test)]
            preparation_counters,
            workers: Mutex::new(Some(WorkerHandles {
                ring_reader,
                output_preparation,
            })),
        })
    }

    pub(crate) fn set_output_active(&self, width: u32, height: u32, active: bool) {
        let Some(output) = OutputSize::new(width, height) else {
            return;
        };
        if self.control.set_output_active(output, active) {
            self.last_delivered_live_revisions[output.slot()].store(0, Ordering::Release);
            if active {
                self.prepared
                    .write()
                    .unwrap_or_else(std::sync::PoisonError::into_inner)
                    .set(output, self.placeholders.get(output));
            }
        }
    }

    pub(crate) fn acquire_for_output(&self, width: u32, height: u32) -> Option<AcquiredNv12Frame> {
        let requested = OutputSize::new(width, height)?;
        let prepared = self
            .prepared
            .read()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
            .get(requested);
        let frame = prepared.output(width, height)?;
        let origin = match prepared.key {
            SourceKey::Placeholder => {
                self.last_delivered_live_revisions[requested.slot()].store(0, Ordering::Release);
                FrameOrigin::Placeholder
            }
            SourceKey::Live(revision) => {
                let previous = self.last_delivered_live_revisions[requested.slot()]
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
    fn preparation_counts(&self) -> (u64, u64, u64) {
        (
            self.preparation_counters.output_480.load(Ordering::Relaxed),
            self.preparation_counters.output_720.load(Ordering::Relaxed),
            self.preparation_counters
                .output_1080
                .load(Ordering::Relaxed),
        )
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
    let mut last_sent: [Option<(SourceKey, u64)>; 3] = [None; 3];
    while let Some((active_outputs, demand_revision)) = control.wait_until_active() {
        for output in OUTPUT_SIZES {
            if active_outputs & output.bit() == 0 {
                continue;
            }
            let snapshot = reader.snapshot(output, demand_revision);
            let request_key = (snapshot.key, demand_revision);
            let slot = output.slot();
            if Some(request_key) != last_sent[slot] {
                match source_tx.try_send(snapshot) {
                    Ok(()) => last_sent[slot] = Some(request_key),
                    Err(TrySendError::Full(_)) => {}
                    Err(TrySendError::Disconnected(_)) => return,
                }
            }
        }
        if control.wait_for_poll(demand_revision) {
            break;
        }
    }
}

#[cfg(test)]
mod tests;
