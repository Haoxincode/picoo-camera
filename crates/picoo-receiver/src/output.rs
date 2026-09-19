//! Dedicated CPU sink preparation from native source frames.
//! REQ-PICOO-NEXT-029/033/034: Receiver owner never maps or transforms pixels.

mod backend;
#[cfg(windows)]
pub(crate) use backend::{
    BackendCapabilities, BackendFailureReason, BackendState, OutputBackend, OutputPlan,
};

#[cfg(windows)]
use std::sync::{mpsc, Arc, Condvar, Mutex};
#[cfg(windows)]
use std::thread::{self, JoinHandle};
#[cfg(windows)]
use std::time::Duration;

#[cfg(windows)]
use picoo_frame_hub::{
    NativeVideoFrame, PlaceholderMode, RingContentFence, RingPublishOutcome, SharedFrameKind,
    SharedFrameRingProducer, SharedRingError, SharedRingSubmitOutcome,
};
#[cfg(windows)]
use picoo_gpu::CpuImage;

#[cfg(target_os = "macos")]
mod apple;
#[cfg(target_os = "macos")]
mod macos_cmio;
#[cfg(target_os = "macos")]
mod native_macos;
#[cfg(target_os = "macos")]
pub(crate) use native_macos::NativeOutput;
#[cfg(windows)]
mod windows;
#[cfg(windows)]
use windows::{prepare, Resources};
#[cfg(windows)]
mod native_windows;
#[cfg(windows)]
pub(crate) use native_windows::NativeOutput;

#[cfg(windows)]
enum Request {
    Frame(Arc<NativeVideoFrame>),
    Placeholder(PlaceholderMode, bool),
}

#[cfg(windows)]
#[derive(Default)]
struct State {
    pending: Option<Request>,
    generation: u64,
    kind: SharedFrameKind,
    stopped: bool,
    exports: u64,
    demand_waits: u64,
}

pub(crate) enum OutputEvent {
    Published,
    Failed(String),
}

#[cfg(windows)]
pub(crate) struct CpuOutput {
    shared: Arc<(Mutex<State>, Condvar)>,
    events: Arc<Mutex<Option<(u64, OutputEvent)>>>,
    generation: RingContentFence,
    worker: Option<JoinHandle<()>>,
    backend: BackendState,
}

#[cfg(windows)]
impl CpuOutput {
    pub(crate) fn start(
        factory: impl FnOnce() -> Result<SharedFrameRingProducer, SharedRingError> + Send + 'static,
    ) -> Result<Self, SharedRingError> {
        let shared = Arc::new((Mutex::new(State::default()), Condvar::new()));
        let worker_shared = Arc::clone(&shared);
        let events = Arc::new(Mutex::new(None));
        let worker_events = Arc::clone(&events);
        let (started, startup) = mpsc::sync_channel(1);
        let worker = thread::Builder::new()
            .name("picoo-cpu-output".into())
            .spawn(move || {
                let mut producer = match factory() {
                    Ok(producer) => producer,
                    Err(error) => {
                        let _ = started.send(Err(error));
                        return;
                    }
                };
                let worker_generation = producer.content_fence();
                worker_shared.0.lock().unwrap().generation = worker_generation.invalidate();
                let _ = started.send(Ok(worker_generation.clone()));
                let mut resources: Option<Resources> = None;
                let mut prepared_cache = None;
                let mut published_key = None;
                let mut served_request = 0;
                loop {
                    let (generation, request) = {
                        let (lock, ready) = &*worker_shared;
                        let mut state = lock.lock().unwrap();
                        while !state.stopped {
                            let admitted = match state.pending.as_ref() {
                                Some(Request::Placeholder(..)) => true,
                                Some(Request::Frame(_)) => {
                                    let request = producer
                                        .cpu_request_sequence()
                                        .filter(|sequence| *sequence != served_request);
                                    let active = request.is_some();
                                    if let Some(sequence) = request {
                                        // One latest request authorizes one preparation attempt.
                                        // Capture it before GPU work so a later request stays pending.
                                        served_request = sequence;
                                    }
                                    if !active {
                                        state.demand_waits = state.demand_waits.saturating_add(1);
                                    }
                                    active
                                }
                                None => false,
                            };
                            if admitted {
                                break;
                            }
                            // Consumers renew demand through shared atomics, not
                            // this process's Condvar. A bounded poll admits the
                            // latest retained source even if capture is paused.
                            state = if state.pending.is_none() {
                                ready.wait(state).unwrap()
                            } else {
                                ready
                                    .wait_timeout(state, Duration::from_millis(16))
                                    .unwrap()
                                    .0
                            };
                        }
                        if state.stopped {
                            return;
                        }
                        (state.generation, state.pending.take().unwrap())
                    };
                    let source_key = match &request {
                        Request::Frame(frame) => Some((
                            generation,
                            frame.identity(),
                            frame.description().config_revision,
                        )),
                        Request::Placeholder(..) => None,
                    };
                    if source_key.is_some() && source_key == published_key {
                        continue;
                    }
                    let prepared = match request {
                        Request::Frame(frame) => {
                            let cached = prepared_cache
                                .as_ref()
                                .filter(|(key, _)| Some(*key) == source_key)
                                .map(|(_, image)| Arc::clone(image));
                            let image = match cached {
                                Some(image) => Ok(image),
                                None => prepare_counted(&mut resources, &frame, &worker_shared),
                            };
                            image.map(|image| {
                                prepared_cache = Some((source_key.unwrap(), Arc::clone(&image)));
                                Prepared::Image(image)
                            })
                        }
                        Request::Placeholder(mode, reconnecting) => {
                            resources = None;
                            prepared_cache = None;
                            published_key = None;
                            Ok(Prepared::Placeholder(if reconnecting {
                                mode.reconnecting_frame()
                            } else {
                                mode.waiting_frame()
                            }))
                        }
                    };
                    if worker_generation.current() != generation {
                        continue;
                    }
                    let prepared = match prepared {
                        Ok(prepared) => prepared,
                        Err(error) => {
                            *worker_events.lock().unwrap() =
                                Some((generation, OutputEvent::Failed(error)));
                            continue;
                        }
                    };
                    loop {
                        let (lock, ready) = &*worker_shared;
                        let state = lock.lock().unwrap();
                        if state.stopped {
                            return;
                        }
                        if state.generation != generation {
                            break;
                        }
                        // No queue mutex is held across copies. The IPC slot keeps
                        // this request's generation; concurrent invalidation also
                        // prevents cross-process readers acquiring a stale result.
                        drop(state);
                        if worker_generation.current() != generation {
                            break;
                        }
                        let result = match &prepared {
                            Prepared::Image(image) => producer.publish_nv12_in_generation(
                                generation,
                                image.spec().width,
                                image.spec().height,
                                image.stride(),
                                0,
                                picoo_media_decode::now_timestamp_us(),
                                image.pixels(),
                            ),
                            Prepared::Placeholder(pixels) => producer
                                .publish_nv12_kind_in_generation(
                                    generation,
                                    SharedFrameKind::Placeholder,
                                    picoo_frame_hub::PLACEHOLDER_WIDTH,
                                    picoo_frame_hub::PLACEHOLDER_HEIGHT,
                                    picoo_frame_hub::PLACEHOLDER_WIDTH,
                                    0,
                                    0,
                                    pixels,
                                ),
                        };
                        match result {
                            Ok(RingPublishOutcome::Published { .. }) => {
                                published_key = source_key;
                                *worker_events.lock().unwrap() =
                                    Some((generation, OutputEvent::Published));
                                break;
                            }
                            Err(error) => {
                                *worker_events.lock().unwrap() =
                                    Some((generation, OutputEvent::Failed(error.to_string())));
                                break;
                            }
                            Ok(RingPublishOutcome::Busy) => {
                                let state = lock.lock().unwrap();
                                if state.stopped || state.generation != generation {
                                    break;
                                }
                                if state.pending.is_some() {
                                    break;
                                }
                                let _ =
                                    ready.wait_timeout(state, Duration::from_millis(2)).unwrap();
                            }
                        }
                    }
                }
            })
            .map_err(|error| SharedRingError::Shmem(error.to_string()))?;
        let generation = match startup.recv_timeout(Duration::from_secs(2)) {
            Ok(Ok(fence)) => fence,
            result => {
                shared.0.lock().unwrap().stopped = true;
                shared.1.notify_one();
                return Err(match result {
                    Ok(Err(error)) => error,
                    Err(error) => SharedRingError::Shmem(format!("CPU output startup: {error}")),
                    Ok(Ok(_)) => unreachable!(),
                });
            }
        };
        let backend = BackendState::auto(BackendCapabilities::cpu_bridge_only())
            .map_err(|error| SharedRingError::Shmem(format!("CPU bridge backend: {error:?}")))?;
        Ok(Self {
            shared,
            events,
            generation,
            worker: Some(worker),
            backend,
        })
    }

    pub(crate) const fn backend(&self) -> OutputBackend {
        self.backend.selection().backend
    }

    pub(crate) const fn backend_plan_for(
        &self,
        source_generation: u64,
        output_revision: u64,
    ) -> OutputPlan {
        self.backend.plan_for(source_generation, output_revision)
    }

    pub(crate) const fn backend_failure_reason(&self) -> Option<BackendFailureReason> {
        self.backend.selection().native_failure()
    }

    pub(crate) fn submit(&self, frame: Arc<NativeVideoFrame>) -> SharedRingSubmitOutcome {
        self.enqueue(Request::Frame(frame), false)
    }
    pub(crate) fn placeholder(&self, mode: PlaceholderMode, reconnecting: bool) {
        self.enqueue(Request::Placeholder(mode, reconnecting), true);
    }
    pub(crate) fn invalidate(&self) {
        let mut state = self.shared.0.lock().unwrap();
        self.advance_generation(&mut state, SharedFrameKind::Placeholder);
        state.pending = None;
        self.shared.1.notify_one();
    }
    fn enqueue(&self, request: Request, invalidate: bool) -> SharedRingSubmitOutcome {
        let mut state = self.shared.0.lock().unwrap();
        if state.stopped {
            return SharedRingSubmitOutcome::Stopped;
        }
        let kind = match &request {
            Request::Frame(_) => SharedFrameKind::Live,
            Request::Placeholder(..) => SharedFrameKind::Placeholder,
        };
        if invalidate || state.kind != kind {
            self.advance_generation(&mut state, kind);
            if state.stopped {
                return SharedRingSubmitOutcome::Stopped;
            }
        }
        let replaced = state.pending.replace(request).is_some();
        self.shared.1.notify_one();
        if replaced {
            SharedRingSubmitOutcome::ReplacedPending
        } else {
            SharedRingSubmitOutcome::Queued
        }
    }
    fn advance_generation(&self, state: &mut State, kind: SharedFrameKind) {
        state.kind = kind;
        state.generation = self.generation.invalidate_as(kind);
        if state.generation == 0 {
            state.stopped = true;
            state.pending = None;
        }
    }

    pub(crate) fn poll_event(&self) -> Option<OutputEvent> {
        let (generation, event) = self.events.lock().unwrap().take()?;
        (generation == self.generation.current()).then_some(event)
    }
}

#[cfg(windows)]
impl Drop for CpuOutput {
    fn drop(&mut self) {
        {
            let mut state = self.shared.0.lock().unwrap();
            state.stopped = true;
            state.pending = None;
            self.advance_generation(&mut state, SharedFrameKind::Placeholder);
        }
        self.shared.1.notify_one();
        // A stuck GPU task retains its own leases on its worker. Never join it
        // from the Receiver owner; bounded teardown supervision is separate.
        self.worker.take();
    }
}

#[cfg(windows)]
enum Prepared {
    Image(Arc<CpuImage>),
    Placeholder(Vec<u8>),
}

#[cfg(windows)]
fn prepare_counted(
    resources: &mut Option<Resources>,
    frame: &NativeVideoFrame,
    shared: &Arc<(Mutex<State>, Condvar)>,
) -> Result<Arc<CpuImage>, String> {
    let image = prepare(resources, frame)?;
    let mut state = shared.0.lock().unwrap();
    state.exports = state.exports.saturating_add(1);
    tracing::trace!(
        cpu_exports = state.exports,
        source_frame_id = frame.identity().frame_id,
        "CPU output materialized for active demand"
    );
    Ok(image)
}
