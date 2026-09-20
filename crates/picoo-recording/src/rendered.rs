//! FrameBus -> GPU -> hardware encoder -> MP4 owner — REQ-PICOO-MEDIA-082/085.
//!
//! The Receiver owns only the subscription endpoint and commands. Every GPU,
//! encoder, mux and bundle operation remains on this dedicated worker.

#[cfg(target_os = "macos")]
use crate::{apple::AppleSegment as NativeSegment, apple_encoder::AppleEncoder as NativeEncoder};
use crate::{
    bundle::{
        GapReason, RecordingBundle, RecordingMode, RecordingState, SegmentMetadata, SourceRange,
    },
    rendered_timeline::{RenderedSample, RenderedSampleDecision, RenderedTimeline},
    worker::progress::Progress,
    AppendOutcome, RecordingError, RecordingResult,
};
#[cfg(windows)]
use crate::{
    windows::WindowsSegment as NativeSegment, windows_encoder::WindowsEncoder as NativeEncoder,
};
use picoo_bitstream::Codec;
use picoo_frame_hub::{NativeFrameSubscription, NativeVideoFrame, Rotation, SubscriptionEnd};
#[cfg(target_os = "macos")]
use picoo_gpu::AppleRenderer as NativeRenderer;
#[cfg(windows)]
use picoo_gpu::WindowsRenderer as NativeRenderer;
use picoo_gpu::{OutputColor, OutputFormat, RenderSpec, RenderedImage};
use sha2::{Digest, Sha256};
use std::{
    path::{Path, PathBuf},
    sync::{
        atomic::{AtomicBool, AtomicU8, Ordering},
        Arc, OnceLock,
    },
    time::{Duration, Instant},
};

const WRITE_DEADLINE: Duration = Duration::from_millis(250);
static ACTIVE: AtomicBool = AtomicBool::new(false);

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct RenderedRecordingConfig {
    pub codec: Codec,
    pub width: u32,
    pub height: u32,
    pub fps: u32,
    pub bitrate: u32,
    /// Immutable scene/effect configuration selected for this recording.
    pub scene_revision: u64,
}

impl RenderedRecordingConfig {
    fn validate(self) -> Result<Self, RecordingError> {
        if !matches!((self.width, self.height), (1280, 720) | (1920, 1080))
            || !matches!(self.fps, 30 | 60)
            || self.bitrate == 0
            || self.bitrate > 100_000_000
        {
            return Err(RecordingError::InvalidInput(
                "unsupported rendered recording format",
            ));
        }
        Ok(self)
    }
}

struct Shared {
    progress: Progress,
    path: OnceLock<PathBuf>,
    state: AtomicU8,
    stop: AtomicBool,
    result: OnceLock<RecordingResult>,
}

/// Nonblocking command/result handle. Dropping it requests a normal finish and
/// never joins a native thread. Encoded and rendered recorders use independent
/// process slots, as the product permits one of each concurrently.
pub struct RenderedRecordingWorker {
    shared: Arc<Shared>,
    cutoff: picoo_frame_hub::NativeFrameSubscriptionCutoff,
}

struct WorkerSlot;

impl Drop for WorkerSlot {
    fn drop(&mut self) {
        ACTIVE.store(false, Ordering::Release);
    }
}

impl RenderedRecordingWorker {
    pub fn start(
        parent: PathBuf,
        subscription: NativeFrameSubscription,
        config: RenderedRecordingConfig,
    ) -> Result<Self, RecordingError> {
        let config = config.validate()?;
        let cutoff = subscription.cutoff();
        ACTIVE
            .compare_exchange(false, true, Ordering::AcqRel, Ordering::Acquire)
            .map_err(|_| {
                RecordingError::InvalidInput("rendered recording worker already active")
            })?;
        let slot = WorkerSlot;
        let shared = Arc::new(Shared {
            progress: Progress::new(),
            path: OnceLock::new(),
            state: AtomicU8::new(state_code(RecordingState::Arming)),
            stop: AtomicBool::new(false),
            result: OnceLock::new(),
        });
        let worker_shared = Arc::clone(&shared);
        std::thread::Builder::new()
            .name("picoo-rendered-recording".into())
            .spawn(move || {
                let slot = slot;
                let outcome = match std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
                    run(parent, subscription, config, &worker_shared)
                })) {
                    Ok(outcome) => outcome,
                    Err(_) => RecordingResult {
                        path: worker_shared.path.get().cloned(),
                        state: RecordingState::Failed,
                        error: Some("rendered recording worker panicked".into()),
                    },
                };
                worker_shared
                    .state
                    .store(state_code(outcome.state), Ordering::Release);
                // A published result means another rendered worker may start.
                drop(slot);
                let _ = worker_shared.result.set(outcome);
            })?;
        Ok(Self { shared, cutoff })
    }

    pub fn is_accepting(&self) -> bool {
        !self.shared.stop.load(Ordering::Acquire) && self.shared.result.get().is_none()
    }

    pub fn stop(&self) {
        self.cutoff.close();
        self.shared.stop.store(true, Ordering::Release);
    }

    pub fn stalled(&self) -> bool {
        self.shared.result.get().is_none() && self.shared.progress.stalled()
    }

    pub fn state(&self) -> RecordingState {
        match self.shared.state.load(Ordering::Acquire) {
            0 => RecordingState::Arming,
            1 => RecordingState::Recording,
            2 => RecordingState::Complete,
            3 => RecordingState::HasGaps,
            _ => RecordingState::Failed,
        }
    }

    pub fn result(&self) -> Option<RecordingResult> {
        self.shared.result.get().cloned()
    }
}

impl Drop for RenderedRecordingWorker {
    fn drop(&mut self) {
        self.stop();
    }
}

fn state_code(state: RecordingState) -> u8 {
    match state {
        RecordingState::Arming => 0,
        RecordingState::Recording => 1,
        RecordingState::Complete => 2,
        RecordingState::HasGaps => 3,
        RecordingState::Failed => 4,
    }
}

fn run(
    parent: PathBuf,
    mut subscription: NativeFrameSubscription,
    config: RenderedRecordingConfig,
    shared: &Shared,
) -> RecordingResult {
    let mut recorder = match RenderedRecorder::create(&parent, config) {
        Ok(recorder) => recorder,
        Err(error) => {
            return RecordingResult {
                path: None,
                state: RecordingState::Failed,
                error: Some(error.to_string()),
            };
        }
    };
    let _ = shared.path.set(recorder.path().to_owned());
    shared.progress.tick();
    let result = guarded(&mut recorder, |recorder| {
        pump(recorder, &mut subscription, shared)
    });
    shared.progress.tick();
    RecordingResult {
        path: Some(recorder.path().to_owned()),
        state: recorder.state(),
        error: result.err().map(|error| error.to_string()),
    }
}

fn guarded(
    recorder: &mut RenderedRecorder,
    operation: impl FnOnce(&mut RenderedRecorder) -> Result<(), RecordingError>,
) -> Result<(), RecordingError> {
    let result =
        match std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| operation(recorder))) {
            Ok(result) => result,
            Err(_) => Err(RecordingError::Platform(
                "rendered recording worker panicked".into(),
            )),
        };
    if let Err(error) = &result {
        recorder.abort(&error.to_string());
    }
    result
}

fn pump(
    recorder: &mut RenderedRecorder,
    subscription: &mut NativeFrameSubscription,
    shared: &Shared,
) -> Result<(), RecordingError> {
    loop {
        shared.progress.tick();
        if shared.stop.load(Ordering::Acquire) {
            return drain_and_finish(recorder, subscription, shared);
        }
        match subscription.try_next() {
            Ok(Some(frame)) => {
                recorder.push(frame)?;
                shared
                    .state
                    .store(state_code(recorder.state()), Ordering::Release);
            }
            Ok(None) => std::thread::sleep(Duration::from_millis(2)),
            Err(end) => return recorder.finish_after_subscription_end(end),
        }
    }
}

fn drain_and_finish(
    recorder: &mut RenderedRecorder,
    subscription: &mut NativeFrameSubscription,
    shared: &Shared,
) -> Result<(), RecordingError> {
    loop {
        shared.progress.tick();
        match subscription.try_next() {
            Ok(Some(frame)) => recorder.push(frame)?,
            Ok(None) => return recorder.finish(),
            Err(end) => return recorder.finish_after_subscription_end(end),
        }
    }
}

struct ActiveSegment {
    generation: u64,
    renderer: NativeRenderer,
    encoder: Option<NativeEncoder>,
    native: Option<NativeSegment>,
    configuration_record: Option<Vec<u8>>,
    metadata: SegmentMetadata,
}

struct RenderedRecorder {
    bundle: RecordingBundle,
    config: RenderedRecordingConfig,
    timeline: RenderedTimeline,
    active: Option<ActiveSegment>,
}

impl RenderedRecorder {
    fn create(parent: &Path, config: RenderedRecordingConfig) -> Result<Self, RecordingError> {
        Ok(Self {
            bundle: RecordingBundle::create(parent, RecordingMode::Rendered)?,
            timeline: RenderedTimeline::new(config.fps)
                .map_err(|_| RecordingError::InvalidInput("unsupported rendered frame rate"))?,
            config,
            active: None,
        })
    }

    fn path(&self) -> &Path {
        self.bundle.path()
    }

    fn state(&self) -> RecordingState {
        self.bundle.state()
    }

    fn push(&mut self, frame: Arc<NativeVideoFrame>) -> Result<(), RecordingError> {
        let decision = self
            .timeline
            .offer(
                frame.identity(),
                frame.description().config_revision,
                frame.source_pts_us(),
            )
            .map_err(|error| RecordingError::Platform(error.to_string()))?;
        let RenderedSampleDecision::Encode(sample) = decision else {
            return Ok(());
        };
        self.write_sample(&frame, sample)
    }

    fn write_sample(
        &mut self,
        frame: &NativeVideoFrame,
        sample: RenderedSample,
    ) -> Result<(), RecordingError> {
        if sample.source != frame.identity() || sample.source_pts_us != frame.source_pts_us() {
            return Err(RecordingError::InvalidInput(
                "rendered sample source mismatch",
            ));
        }
        if self
            .active
            .as_ref()
            .is_none_or(|active| active.generation != sample.segment_generation)
        {
            let previous = self
                .active
                .as_ref()
                .map(|active| active.metadata.source.clone());
            self.close_segment()?;
            if sample.missed_slots != 0 && self.bundle.state() == RecordingState::Recording {
                let gap = previous
                    .map(|previous| gap_range(previous, sample))
                    .transpose()?;
                self.bundle.record_gap(GapReason::TimeDiscontinuity, gap)?;
            }
            if !sample.force_idr || sample.segment_pts_us != 0 {
                return Err(RecordingError::InvalidInput(
                    "rendered segment must restart with an IDR at zero",
                ));
            }
            self.active = Some(self.start_segment(frame, sample)?);
        } else if sample.force_idr || sample.missed_slots != 0 {
            return Err(RecordingError::InvalidInput(
                "rendered segment boundary did not advance",
            ));
        }

        let active = self.active.as_mut().expect("rendered segment active");
        let image = render_frame(&mut active.renderer, frame)?;
        if active.encoder.is_none() {
            active.encoder = Some(create_encoder(&image, self.config)?);
        }
        let output = active
            .encoder
            .as_mut()
            .expect("first rendered image starts encoder")
            .encode(image, sample.segment_pts_us, sample.force_idr)?;
        if output.pts_us != sample.segment_pts_us {
            return Err(RecordingError::InvalidInput(
                "hardware encoder changed rendered PTS",
            ));
        }
        let record = output.configuration.record();
        if active
            .configuration_record
            .as_deref()
            .is_some_and(|expected| expected != record)
        {
            return Err(RecordingError::InvalidInput(
                "hardware encoder configuration changed within segment",
            ));
        }
        if active.native.is_none() {
            active.metadata.configuration_sha256 = format!("{:x}", Sha256::digest(record));
            active.configuration_record = Some(record.to_vec());
            active.native = Some(NativeSegment::new(
                &self.bundle.next_partial_path()?,
                output.configuration,
                self.config.fps,
            )?);
        }
        let native = active.native.as_mut().expect("first output starts mux");
        let deadline = Instant::now() + WRITE_DEADLINE;
        loop {
            match native.append(&output.data, output.pts_us)? {
                AppendOutcome::Written => break,
                AppendOutcome::Busy if Instant::now() < deadline => {
                    std::thread::sleep(Duration::from_millis(1));
                }
                AppendOutcome::Busy => {
                    return Err(RecordingError::Platform(
                        "rendered recording writer overrun".into(),
                    ));
                }
            }
        }
        active.metadata.source.last_au = sample.source.frame_id;
        active.metadata.source.last_pts_us = sample.source_pts_us;
        active.metadata.output_last_pts_us = sample.segment_pts_us;
        self.bundle.mark_started(sample.source_pts_us)?;
        Ok(())
    }

    fn start_segment(
        &self,
        frame: &NativeVideoFrame,
        sample: RenderedSample,
    ) -> Result<ActiveSegment, RecordingError> {
        let description = frame.description();
        let visible = description.visible_rect;
        if visible.x != 0
            || visible.y != 0
            || visible.width != frame.image().width()
            || visible.height != frame.image().height()
            || description.pixel_aspect_ratio.numerator
                != description.pixel_aspect_ratio.denominator
        {
            return Err(RecordingError::InvalidInput(
                "unsupported rendered source crop or pixel aspect",
            ));
        }
        let spec = RenderSpec {
            width: self.config.width,
            height: self.config.height,
            rotation: description.transform.rotation,
            mirror: description.transform.mirror,
            color: OutputColor::Bt709Limited,
            format: OutputFormat::Nv12,
        };
        let stream_epoch = u32::try_from(sample.source.stream_epoch)
            .map_err(|_| RecordingError::InvalidInput("source stream epoch overflow"))?;
        Ok(ActiveSegment {
            generation: sample.segment_generation,
            renderer: create_renderer(frame, spec)?,
            encoder: None,
            native: None,
            configuration_record: None,
            metadata: SegmentMetadata {
                source: SourceRange {
                    connection_generation: sample.source.connection_generation,
                    stream_epoch,
                    first_au: sample.source.frame_id,
                    last_au: sample.source.frame_id,
                    first_pts_us: sample.source_pts_us,
                    last_pts_us: sample.source_pts_us,
                },
                output_first_pts_us: 0,
                output_last_pts_us: 0,
                scene_revision: Some(self.config.scene_revision),
                source_rotation: Some(rotation_degrees(description.transform.rotation)),
                source_mirrored: Some(description.transform.mirror),
                codec: match self.config.codec {
                    Codec::Avc => "avc",
                    Codec::Hevc => "hevc",
                }
                .into(),
                width: self.config.width,
                height: self.config.height,
                fps: self.config.fps,
                // The renderer already baked the source transform into pixels.
                // No presentation transform remains for the output file.
                rotation: 0,
                mirrored: false,
                configuration_sha256: String::new(),
            },
        })
    }

    fn close_segment(&mut self) -> Result<(), RecordingError> {
        let Some(mut active) = self.active.take() else {
            return Ok(());
        };
        let Some(native) = active.native.take() else {
            return Ok(());
        };
        self.bundle
            .commit_segment(native.finish()?, active.metadata)
    }

    fn finish_after_subscription_end(
        &mut self,
        end: SubscriptionEnd,
    ) -> Result<(), RecordingError> {
        if self.bundle.state() == RecordingState::Recording {
            self.close_segment()?;
            self.bundle.record_gap(
                match end {
                    SubscriptionEnd::QueueFull(_) | SubscriptionEnd::TooOld(_) => {
                        GapReason::Overrun
                    }
                    SubscriptionEnd::Reset
                    | SubscriptionEnd::PublisherStopped
                    | SubscriptionEnd::Cancelled => GapReason::SourceStopped,
                },
                None,
            )?;
        }
        self.bundle.finish()
    }

    fn finish(&mut self) -> Result<(), RecordingError> {
        self.close_segment().and_then(|()| self.bundle.finish())
    }

    fn abort(&mut self, reason: &str) {
        let _ = self.close_segment();
        let _ = self.bundle.fail(reason);
        self.active = None;
    }
}

#[cfg(target_os = "macos")]
fn create_renderer(
    _frame: &NativeVideoFrame,
    spec: RenderSpec,
) -> Result<NativeRenderer, RecordingError> {
    NativeRenderer::new(spec).map_err(|error| RecordingError::Platform(error.to_string()))
}

#[cfg(windows)]
fn create_renderer(
    frame: &NativeVideoFrame,
    spec: RenderSpec,
) -> Result<NativeRenderer, RecordingError> {
    NativeRenderer::for_source(frame.image(), spec)
        .map_err(|error| RecordingError::Platform(error.to_string()))
}

#[cfg(target_os = "macos")]
fn render_frame(
    renderer: &mut NativeRenderer,
    frame: &NativeVideoFrame,
) -> Result<RenderedImage, RecordingError> {
    renderer
        .render(frame.image())
        .map_err(|error| RecordingError::Platform(error.to_string()))
}

#[cfg(windows)]
fn render_frame(
    renderer: &mut NativeRenderer,
    frame: &NativeVideoFrame,
) -> Result<RenderedImage, RecordingError> {
    renderer
        .render(frame)
        .map_err(|error| RecordingError::Platform(error.to_string()))
}

#[cfg(target_os = "macos")]
fn create_encoder(
    _image: &RenderedImage,
    config: RenderedRecordingConfig,
) -> Result<NativeEncoder, RecordingError> {
    NativeEncoder::new(
        config.codec,
        config.width,
        config.height,
        config.fps,
        config.bitrate,
    )
}

#[cfg(windows)]
fn create_encoder(
    image: &RenderedImage,
    config: RenderedRecordingConfig,
) -> Result<NativeEncoder, RecordingError> {
    NativeEncoder::new(
        image,
        config.codec,
        config.width,
        config.height,
        config.fps,
        config.bitrate,
    )
}

fn gap_range(
    previous: SourceRange,
    current: RenderedSample,
) -> Result<SourceRange, RecordingError> {
    Ok(SourceRange {
        connection_generation: current.source.connection_generation,
        stream_epoch: u32::try_from(current.source.stream_epoch)
            .map_err(|_| RecordingError::InvalidInput("source stream epoch overflow"))?,
        first_au: previous.last_au,
        last_au: current.source.frame_id,
        first_pts_us: previous.last_pts_us,
        last_pts_us: current.source_pts_us,
    })
}

fn rotation_degrees(rotation: Rotation) -> u32 {
    match rotation {
        Rotation::None => 0,
        Rotation::Clockwise90 => 90,
        Rotation::Clockwise180 => 180,
        Rotation::Clockwise270 => 270,
    }
}

#[cfg(all(test, target_os = "macos"))]
mod tests;
