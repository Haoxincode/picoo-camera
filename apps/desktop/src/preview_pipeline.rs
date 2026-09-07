//! Bounded desktop preview preparation — ARCH-PICOO-FRAME-001 / REQ-PICOO-UI-004.
//!
//! The source bus remains the decoded-frame authority. A capacity-one worker
//! prepares the visible preview with platform-native GPU images.

use std::sync::{Arc, Condvar, Mutex};
use std::thread::{self, JoinHandle};
use std::time::{Duration, Instant};

use picoo_receiver::ReceiverFrame as VideoFrame;

#[cfg(target_os = "macos")]
mod macos_surface;
#[cfg(windows)]
mod windows_surface;
#[cfg(target_os = "macos")]
use macos_surface::PlatformPreviewResources;
#[cfg(windows)]
use windows_surface::PlatformPreviewResources;

const PREVIEW_MAX_DETAIL_WIDTH: u32 = 1920;
const PREVIEW_TARGET_FRAME_INTERVAL: Duration = Duration::from_nanos(16_666_667);

#[derive(Debug)]
struct PreviewRequest {
    sequence: u64,
    generation: u64,
    frame: Arc<VideoFrame>,
    target_width: u32,
}

#[derive(Debug)]
pub(crate) struct PreparedPreview {
    pub(crate) sequence: u64,
    pub(crate) surface: gpui_kit::SurfaceSource,
}

// CoreVideo pixel buffers are immutable while crossing this hand-off: the worker
// completes its GPU render before publishing it and GPUI only reads it. Core Foundation
// retain/release and CVPixelBuffer are documented for cross-thread ownership.
#[cfg(target_os = "macos")]
unsafe impl Send for PreparedPreview {}

fn new_platform_preview_resources() -> PlatformPreviewResources {
    PlatformPreviewResources::default()
}

#[derive(Debug, Clone)]
pub(crate) struct PreviewViewportTracker(Arc<Mutex<PreviewViewport>>);

#[derive(Debug)]
struct PreviewViewport {
    width: f32,
    height: f32,
    pending: bool,
}

impl Default for PreviewViewportTracker {
    fn default() -> Self {
        Self(Arc::new(Mutex::new(PreviewViewport {
            width: 0.0,
            height: 0.0,
            pending: false,
        })))
    }
}

impl PreviewViewportTracker {
    pub(crate) fn request_frame(&self, width: f32, height: f32) {
        *self.0.lock().unwrap() = PreviewViewport {
            width,
            height,
            pending: true,
        };
    }

    pub(crate) fn take_target_physical_width(&self) -> Option<f32> {
        let mut viewport = self.0.lock().unwrap();
        let pending = std::mem::take(&mut viewport.pending);
        if !pending || viewport.width <= 0.0 || viewport.height <= 0.0 {
            return None;
        }
        Some(viewport.width)
    }
}

#[derive(Default)]
struct WorkerState {
    generation: u64,
    pending: Option<PreviewRequest>,
    completed: Option<PreparedPreview>,
    stopped: bool,
}

impl WorkerState {
    fn enqueue_latest(&mut self, request: PreviewRequest) {
        self.pending = Some(request);
    }

    fn publish_latest(&mut self, preview: PreparedPreview) {
        if self
            .completed
            .as_ref()
            .is_none_or(|completed| preview.sequence > completed.sequence)
        {
            self.completed = Some(preview);
        }
    }
}

pub(crate) struct PreviewPipeline {
    shared: Arc<(Mutex<WorkerState>, Condvar)>,
    worker: Option<JoinHandle<()>>,
    last_submitted_sequence: u64,
    last_submitted_frame: Option<Arc<VideoFrame>>,
    last_submitted_width: u32,
    cadence: PreviewCadence,
    target_width: u32,
}

#[derive(Debug)]
struct PreviewCadence {
    interval: Duration,
    next_deadline: Option<Instant>,
}

impl PreviewCadence {
    fn new(interval: Duration) -> Self {
        Self {
            interval,
            next_deadline: None,
        }
    }

    fn take_due(&mut self, now: Instant) -> bool {
        let Some(deadline) = self.next_deadline else {
            self.next_deadline = Some(now + self.interval);
            return true;
        };
        if now < deadline {
            return false;
        }

        // Keep phase for ordinary polling jitter. After a missed period, start
        // a fresh interval so resuming visibility cannot submit a burst.
        self.next_deadline = if now.duration_since(deadline) >= self.interval {
            now.checked_add(self.interval)
        } else {
            deadline.checked_add(self.interval)
        };
        true
    }
}

impl Default for PreviewPipeline {
    fn default() -> Self {
        Self::new()
    }
}

impl PreviewPipeline {
    fn new() -> Self {
        let shared = Arc::new((Mutex::new(WorkerState::default()), Condvar::new()));
        let worker_shared = Arc::clone(&shared);
        let worker = thread::Builder::new()
            .name("picoo-preview".into())
            .spawn(move || preview_worker(worker_shared))
            .expect("start desktop preview worker");
        Self {
            shared,
            worker: Some(worker),
            last_submitted_sequence: 0,
            last_submitted_frame: None,
            last_submitted_width: 0,
            cadence: PreviewCadence::new(PREVIEW_TARGET_FRAME_INTERVAL),
            target_width: PREVIEW_MAX_DETAIL_WIDTH,
        }
    }

    /// Use the physical window width as a conservative upper bound for the
    /// preview surface. This avoids upscaling prepared pixels while keeping
    /// conversion work bounded to Full HD for the currently supported sources.
    pub(crate) fn set_viewport_physical_width(&mut self, width: f32) {
        self.target_width = target_width_for_viewport(width);
    }

    /// Submit a newer shared VideoFrame without copying pixels or timeline data.
    /// A not-yet-started older request is replaced instead of queued.
    pub(crate) fn submit_latest(&mut self, frame: &Arc<VideoFrame>) -> bool {
        if self
            .last_submitted_frame
            .as_ref()
            .is_some_and(|last| Arc::ptr_eq(last, frame))
            && self.last_submitted_width == self.target_width
        {
            return false;
        }
        if !self.cadence.take_due(Instant::now()) {
            return false;
        }
        self.last_submitted_sequence = match self.last_submitted_sequence.checked_add(1) {
            Some(sequence) => sequence,
            None => return false,
        };
        self.last_submitted_frame = Some(Arc::clone(frame));
        self.last_submitted_width = self.target_width;
        let generation = self.shared.0.lock().unwrap().generation;
        let request = PreviewRequest {
            sequence: self.last_submitted_sequence,
            generation,
            frame: Arc::clone(frame),
            target_width: self.target_width,
        };
        let (state, ready) = &*self.shared;
        state.lock().unwrap().enqueue_latest(request);
        ready.notify_one();
        true
    }

    pub(crate) fn clear(&mut self) -> bool {
        if self.last_submitted_frame.take().is_none() {
            return false;
        }
        let mut state = self.shared.0.lock().unwrap();
        match state.generation.checked_add(1) {
            Some(next) => state.generation = next,
            None => state.stopped = true,
        }
        state.pending = None;
        state.completed = None;
        true
    }

    pub(crate) fn take_prepared(&mut self) -> Option<PreparedPreview> {
        self.shared.0.lock().unwrap().completed.take()
    }
}

impl Drop for PreviewPipeline {
    fn drop(&mut self) {
        let (state, ready) = &*self.shared;
        state.lock().unwrap().stopped = true;
        ready.notify_one();
        // GPU work keeps its leases until completion; UI teardown never waits
        // for a platform task that may be stalled by device loss.
        self.worker.take();
    }
}

fn preview_worker(shared: Arc<(Mutex<WorkerState>, Condvar)>) {
    let mut platform_resources = new_platform_preview_resources();
    loop {
        let request = {
            let (state, ready) = &*shared;
            let mut state = state.lock().unwrap();
            while state.pending.is_none() && !state.stopped {
                state = ready.wait(state).unwrap();
            }
            if state.stopped {
                return;
            }
            state.pending.take().expect("pending request")
        };

        let generation = request.generation;
        let prepared = prepare_preview(request, &mut platform_resources);
        let mut state = shared.0.lock().unwrap();
        if state.stopped {
            return;
        }
        if state.generation != generation {
            continue;
        }
        if let Some(prepared) = prepared {
            // Publish the finished frame even when a newer request is pending.
            // The completed slot remains bounded and this avoids starvation if
            // conversion is briefly slower than the incoming cadence.
            state.publish_latest(prepared);
        }
    }
}

fn target_width_for_viewport(viewport_physical_width: f32) -> u32 {
    if !viewport_physical_width.is_finite() {
        return PREVIEW_MAX_DETAIL_WIDTH;
    }
    (viewport_physical_width.ceil() as u32).clamp(2, PREVIEW_MAX_DETAIL_WIDTH) & !1
}

fn prepare_preview(
    request: PreviewRequest,
    platform_resources: &mut PlatformPreviewResources,
) -> Option<PreparedPreview> {
    let sequence = request.sequence;

    let surface = platform_resources.prepare_surface(&request.frame, request.target_width)?;
    Some(PreparedPreview {
        sequence,
        surface: surface.into(),
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use picoo_frame_hub::nv12_black;
    use std::time::Instant;

    #[test]
    fn preview_demand_survives_a_source_gap_and_is_consumed_once() {
        let viewport = PreviewViewportTracker::default();
        viewport.request_frame(1280.0, 720.0);
        std::thread::sleep(Duration::from_millis(150));
        assert_eq!(viewport.take_target_physical_width(), Some(1280.0));
        assert_eq!(viewport.take_target_physical_width(), None);
        viewport.request_frame(1920.0, 1080.0);
        assert_eq!(viewport.take_target_physical_width(), Some(1920.0));
    }

    fn request(sequence: u64, width: u32, height: u32, target_width: u32) -> PreviewRequest {
        request_with_pixels(
            sequence,
            width,
            height,
            target_width,
            nv12_black(width, height).into(),
        )
    }

    fn request_with_pixels(
        sequence: u64,
        width: u32,
        height: u32,
        target_width: u32,
        pixels: bytes::Bytes,
    ) -> PreviewRequest {
        let frame = {
            use picoo_frame_hub::*;
            let image = picoo_media_decode::DecodedFrame::fixture_nv12(
                width,
                height,
                width,
                0,
                sequence * 1_000,
                pixels,
            )
            .unwrap()
            .into_native_image();
            NativeVideoFrame::new(
                FrameIdentity {
                    connection_generation: 1,
                    stream_epoch: 1,
                    decoder_generation: 1,
                    frame_id: sequence,
                },
                sequence * 1_000,
                FrameDescription {
                    coded_size: ImageSize { width, height },
                    visible_rect: VisibleRect {
                        x: 0,
                        y: 0,
                        width,
                        height,
                    },
                    pixel_aspect_ratio: PixelAspectRatio {
                        numerator: 1,
                        denominator: 1,
                    },
                    color: SourceColor::Nv12Bt709Limited {
                        chroma_siting: ChromaSiting::Left,
                    },
                    transform: PresentationTransform {
                        rotation: Rotation::None,
                        mirror: false,
                    },
                    config_revision: 1,
                },
                image,
                FrameTimeline {
                    encoded_at_us: 0,
                    received_at_us: 0,
                    decode_submitted_at_us: 0,
                    decoded_at: Instant::now(),
                },
            )
            .unwrap()
        };
        PreviewRequest {
            sequence,
            generation: 0,
            frame: Arc::new(frame),
            target_width,
        }
    }

    fn prepared(sequence: u64) -> PreparedPreview {
        #[cfg(windows)]
        {
            PreparedPreview {
                sequence,
                surface: gpui_kit::Direct3DSurface::new(NoDrawFixture).into(),
            }
        }
        #[cfg(not(windows))]
        {
            let mut resources = new_platform_preview_resources();
            prepare_preview(request(sequence, 2, 2, 1280), &mut resources).expect("prepare fixture")
        }
    }

    #[cfg(windows)]
    #[derive(Debug)]
    struct NoDrawFixture;
    // SAFETY: Queue tests never expose a native view or invoke the draw callback.
    #[cfg(windows)]
    unsafe impl gpui_kit::Direct3DSurfaceSource for NoDrawFixture {
        fn size(&self) -> gpui_kit::Size<gpui_kit::DevicePixels> {
            gpui_kit::size(gpui_kit::DevicePixels(2), gpui_kit::DevicePixels(2))
        }
        unsafe fn with_read(
            &self,
            _: &windows::Win32::Graphics::Direct3D11::ID3D11Device,
            _: &mut dyn FnMut(
                &windows::Win32::Graphics::Direct3D11::ID3D11ShaderResourceView,
            ) -> anyhow::Result<()>,
        ) -> anyhow::Result<bool> {
            Ok(false)
        }
    }

    #[test]
    fn pending_slot_keeps_only_the_latest_frame() {
        let mut state = WorkerState::default();
        state.enqueue_latest(request(1, 2, 2, 1280));
        state.enqueue_latest(request(2, 2, 2, 1280));
        assert_eq!(state.pending.expect("latest request").sequence, 2);
    }

    #[test]
    fn completed_slot_keeps_newest_result_without_starving_rendering() {
        let mut state = WorkerState::default();
        state.publish_latest(prepared(2));
        state.publish_latest(prepared(1));
        assert_eq!(state.completed.expect("latest result").sequence, 2);
    }

    #[test]
    fn target_width_tracks_physical_viewport_with_bounded_detail() {
        assert_eq!(target_width_for_viewport(960.0), 960);
        assert_eq!(target_width_for_viewport(1440.1), 1440);
        assert_eq!(target_width_for_viewport(2560.0), 1920);
    }

    #[test]
    fn cadence_stays_near_sixty_fps_across_common_ui_check_intervals() {
        for check_interval in [
            Duration::from_millis(16),
            Duration::from_micros(16_200),
            Duration::from_micros(16_667),
        ] {
            let start = Instant::now();
            let end = start + Duration::from_secs(100);
            let mut cadence = PreviewCadence::new(PREVIEW_TARGET_FRAME_INTERVAL);
            let mut now = start;
            let mut submitted = 0_u32;
            while now <= end {
                submitted += u32::from(cadence.take_due(now));
                now += check_interval;
            }
            let fps = f64::from(submitted) / 100.0;
            assert!(
                (59.9..=60.1).contains(&fps),
                "check interval {check_interval:?} produced {fps:.2} fps"
            );
        }
    }

    #[test]
    fn cadence_skips_hidden_periods_without_replaying_them() {
        let start = Instant::now();
        let mut cadence = PreviewCadence::new(PREVIEW_TARGET_FRAME_INTERVAL);
        assert!(cadence.take_due(start));
        assert!(cadence.take_due(start + Duration::from_secs(5)));
        assert!(!cadence.take_due(start + Duration::from_secs(5) + Duration::from_millis(1)));
    }
}
