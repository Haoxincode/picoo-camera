//! Dedicated CPU sink preparation from native source frames.
//! REQ-PICOO-NEXT-029/033/034: Receiver owner never maps or transforms pixels.

use std::sync::{mpsc, Arc, Condvar, Mutex};
use std::thread::{self, JoinHandle};
use std::time::Duration;

use picoo_frame_hub::{
    NativeVideoFrame, PlaceholderMode, RingContentFence, RingPublishOutcome,
    SharedFrameRingProducer, SharedRingError, SharedRingSubmitOutcome,
};
use picoo_gpu::{AppleRenderer, CpuExporter, CpuImage, OutputColor, RenderSpec, Rotation};

enum Request {
    Frame(Arc<NativeVideoFrame>),
    Placeholder(PlaceholderMode, bool),
}

#[derive(Default)]
struct State {
    pending: Option<Request>,
    generation: u64,
    stopped: bool,
    exports: u64,
    demand_waits: u64,
}

pub(crate) enum OutputEvent {
    Published,
    Failed(String),
}

pub(crate) struct MacCpuOutput {
    shared: Arc<(Mutex<State>, Condvar)>,
    events: Arc<Mutex<Option<(u64, OutputEvent)>>>,
    generation: RingContentFence,
    worker: Option<JoinHandle<()>>,
}

impl MacCpuOutput {
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
                loop {
                    let (generation, request) = {
                        let (lock, ready) = &*worker_shared;
                        let mut state = lock.lock().unwrap();
                        while !state.stopped {
                            let admitted = match state.pending.as_ref() {
                                Some(Request::Placeholder(..)) => true,
                                Some(Request::Frame(_)) => {
                                    let active = producer.has_cpu_demand();
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
                            Prepared::Placeholder(pixels) => producer.publish_nv12_in_generation(
                                generation,
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
        Ok(Self {
            shared,
            events,
            generation,
            worker: Some(worker),
        })
    }

    pub(crate) fn submit(&self, frame: Arc<NativeVideoFrame>) -> SharedRingSubmitOutcome {
        self.enqueue(Request::Frame(frame), false)
    }
    pub(crate) fn placeholder(&self, mode: PlaceholderMode, reconnecting: bool) {
        self.enqueue(Request::Placeholder(mode, reconnecting), true);
    }
    pub(crate) fn invalidate(&self) {
        let mut state = self.shared.0.lock().unwrap();
        self.advance_generation(&mut state);
        state.pending = None;
        self.shared.1.notify_one();
    }
    fn enqueue(&self, request: Request, invalidate: bool) -> SharedRingSubmitOutcome {
        let mut state = self.shared.0.lock().unwrap();
        if state.stopped {
            return SharedRingSubmitOutcome::Stopped;
        }
        if invalidate {
            self.advance_generation(&mut state);
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
    fn advance_generation(&self, state: &mut State) {
        state.generation = self.generation.invalidate();
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

impl Drop for MacCpuOutput {
    fn drop(&mut self) {
        {
            let mut state = self.shared.0.lock().unwrap();
            state.stopped = true;
            state.pending = None;
            self.advance_generation(&mut state);
        }
        self.shared.1.notify_one();
        // A stuck GPU task retains its own leases on its worker. Never join it
        // from the Receiver owner; bounded teardown supervision is separate.
        self.worker.take();
    }
}

struct Resources {
    spec: RenderSpec,
    renderer: AppleRenderer,
    exporter: CpuExporter,
}
enum Prepared {
    Image(Arc<CpuImage>),
    Placeholder(Vec<u8>),
}

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

fn prepare(
    resources: &mut Option<Resources>,
    frame: &NativeVideoFrame,
) -> Result<Arc<CpuImage>, String> {
    let description = frame.description();
    let crop = description.visible_rect;
    if crop.x != 0
        || crop.y != 0
        || crop.width != frame.image().width()
        || crop.height != frame.image().height()
        || description.pixel_aspect_ratio.numerator != description.pixel_aspect_ratio.denominator
    {
        return Err("unsupported native source crop or pixel aspect".into());
    }
    let (mut width, mut height) = (crop.width, crop.height);
    if matches!(
        description.transform.rotation,
        Rotation::Clockwise90 | Rotation::Clockwise270
    ) {
        std::mem::swap(&mut width, &mut height);
    }
    let spec = RenderSpec {
        width,
        height,
        rotation: description.transform.rotation,
        mirror: description.transform.mirror,
        color: OutputColor::Bt709Limited,
    };
    if resources
        .as_ref()
        .is_none_or(|current| current.spec != spec)
    {
        *resources = Some(Resources {
            spec,
            renderer: AppleRenderer::new(spec).map_err(|e| e.to_string())?,
            exporter: CpuExporter::new(spec).map_err(|e| e.to_string())?,
        });
    }
    let resources = resources.as_mut().unwrap();
    let image = resources
        .renderer
        .render(frame.image())
        .map_err(|e| e.to_string())?;
    resources.exporter.export(&image).map_err(|e| e.to_string())
}

#[cfg(test)]
mod tests {
    use super::*;
    use picoo_frame_hub::{
        FrameBus, FrameDescription, FrameIdentity, FrameTimeline, NativeVideoFrame,
        PresentationTransform, SharedFrameRingConsumer, SourceColor, DEFAULT_MAX_FRAME_BYTES,
    };
    use std::time::Instant;

    fn wait_until(mut predicate: impl FnMut() -> bool) {
        let deadline = Instant::now() + Duration::from_secs(3);
        while !predicate() {
            assert!(
                Instant::now() < deadline,
                "CPU demand worker did not progress"
            );
            thread::sleep(Duration::from_millis(1));
        }
    }

    #[test]
    fn hardware_decode_bus_gpu_and_cpu_sink_preserve_bt709_pixels() {
        // REQ-PICOO-NEXT-011/016/029: source stays native up to the output exporter.
        let decoded = picoo_media_decode::create_platform_decoder()
            .decode_access_unit(
                &crate::tests::wire_avc(picoo_testkit::AVC_64X64_BT709_IDR),
                None,
            )
            .unwrap()
            .frame
            .unwrap();
        let description = decoded.description().native_format;
        let mut bus = FrameBus::new();
        bus.publish(
            NativeVideoFrame::new(
                FrameIdentity {
                    connection_generation: 1,
                    stream_epoch: 1,
                    decoder_generation: 1,
                    frame_id: 7,
                },
                42_000,
                FrameDescription {
                    coded_size: description.coded_size,
                    visible_rect: description.visible_rect,
                    pixel_aspect_ratio: description.pixel_aspect_ratio,
                    color: SourceColor::Nv12Bt709Limited {
                        chroma_siting: description.chroma_siting,
                    },
                    transform: PresentationTransform {
                        rotation: Rotation::None,
                        mirror: false,
                    },
                    config_revision: 3,
                },
                decoded.into_native_image(),
                FrameTimeline {
                    encoded_at_us: 43_000,
                    received_at_us: 45_000,
                    decode_submitted_at_us: 46_000,
                    decoded_at: Instant::now(),
                },
            )
            .unwrap(),
        );
        let name = format!(
            "picoo-native-output-{}",
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap()
                .as_nanos()
        );
        let producer_name = name.clone();
        let output = MacCpuOutput::start(move || {
            SharedFrameRingProducer::create(&producer_name, DEFAULT_MAX_FRAME_BYTES)
        })
        .unwrap();
        let consumer = SharedFrameRingConsumer::open(&name, DEFAULT_MAX_FRAME_BYTES).unwrap();
        let source = bus.latest().unwrap().clone();
        output.submit(Arc::clone(&source));
        // Opening the mapping is not demand; wait until the worker actually
        // observes the queued frame before checking that no export occurred.
        wait_until(|| output.shared.0.lock().unwrap().demand_waits > 0);
        assert_eq!(output.shared.0.lock().unwrap().exports, 0);
        // Releasing the source bus must not invalidate the worker's native lease.
        bus.clear();
        let deadline = Instant::now() + Duration::from_secs(3);
        loop {
            if let Some(frame) = consumer.latest_frame() {
                assert_eq!((frame.width, frame.height), (64, 64));
                assert!(frame.timestamp_us > 0);
                let y = frame.nv12[0];
                let u = frame.nv12[64 * 64];
                let v = frame.nv12[64 * 64 + 1];
                assert!(
                    y.abs_diff(63) <= 3 && u.abs_diff(102) <= 3 && v.abs_diff(240) <= 3,
                    "hardware BT.709 red changed through GPU/CPU output: {y}/{u}/{v}"
                );
                break;
            }
            if let Some(OutputEvent::Failed(error)) = output.poll_event() {
                panic!("output failed: {error}");
            }
            assert!(
                Instant::now() < deadline,
                "native GPU output did not publish"
            );
            thread::sleep(Duration::from_millis(1));
        }
        assert_eq!(output.shared.0.lock().unwrap().exports, 1);
        let first_sequence = consumer.latest_frame().unwrap().sequence;
        for _ in 0..8 {
            output.submit(Arc::clone(&source));
        }
        wait_until(|| output.shared.0.lock().unwrap().pending.is_none());
        assert_eq!(
            output.shared.0.lock().unwrap().exports,
            1,
            "same source is exported once"
        );
        assert_eq!(consumer.latest_frame().unwrap().sequence, first_sequence);
        drop(consumer);
        thread::sleep(Duration::from_millis(300)); // crashed reader's 250ms lease expires
        let waited = output.shared.0.lock().unwrap().demand_waits;
        let next_source = Arc::new(
            NativeVideoFrame::new(
                FrameIdentity {
                    frame_id: source.identity().frame_id + 1,
                    ..source.identity()
                },
                source.source_pts_us() + 33_333,
                source.description(),
                source.image().clone(),
                source.timeline(),
            )
            .unwrap(),
        );
        output.submit(next_source);
        wait_until(|| output.shared.0.lock().unwrap().demand_waits > waited);
        assert_eq!(
            output.shared.0.lock().unwrap().exports,
            1,
            "expired demand cannot export new source"
        );
        let consumer = SharedFrameRingConsumer::open(&name, DEFAULT_MAX_FRAME_BYTES).unwrap();
        // No new source submission: an actual read request wakes retained work.
        wait_until(|| {
            consumer
                .latest_frame()
                .is_some_and(|frame| frame.sequence > first_sequence)
        });
        assert_eq!(output.shared.0.lock().unwrap().exports, 2);
        output.invalidate();
        assert!(
            consumer.latest_frame().is_none(),
            "owner invalidation fences IPC immediately"
        );
    }
}
