//! Bounded desktop preview preparation — ARCH-PICOO-FRAME-001 / REQ-PICOO-UI-004.
//!
//! LatestFrameStore remains the decoded-frame authority. This consumer keeps one pending
//! latest frame and performs SIMD color conversion and filtered scaling away
//! from the GPUI thread.

use std::sync::{Arc, Condvar, Mutex};
use std::thread::{self, JoinHandle};
use std::time::{Duration, Instant};

#[cfg(not(target_os = "macos"))]
use fast_image_resize::images::{Image, ImageRef};
#[cfg(not(target_os = "macos"))]
use fast_image_resize::{FilterType, PixelType, ResizeAlg, ResizeOptions, Resizer};
use picoo_frame_hub::VideoFrame;
#[cfg(not(target_os = "macos"))]
use yuv::{yuv_nv12_to_bgra, YuvBiPlanarImage, YuvConversionMode, YuvRange, YuvStandardMatrix};

#[cfg(target_os = "macos")]
use core_video::pixel_buffer::CVPixelBuffer;

#[cfg(target_os = "macos")]
mod macos_surface;
#[cfg(target_os = "macos")]
use macos_surface::PlatformPreviewResources;

const PREVIEW_MAX_DETAIL_WIDTH: u32 = 1920;
const PREVIEW_TARGET_FRAME_INTERVAL: Duration = Duration::from_millis(33);
const PREVIEW_PAINT_FRESHNESS: Duration = Duration::from_millis(100);

#[derive(Debug)]
struct PreviewRequest {
    frame: Arc<VideoFrame>,
    target_width: u32,
}

#[derive(Debug)]
pub(crate) struct PreparedPreview {
    pub(crate) sequence: u64,
    #[cfg(not(target_os = "macos"))]
    pub(crate) width: u32,
    #[cfg(not(target_os = "macos"))]
    pub(crate) height: u32,
    #[cfg(not(target_os = "macos"))]
    pub(crate) bgra: Vec<u8>,
    #[cfg(target_os = "macos")]
    pub(crate) pixel_buffer: CVPixelBuffer,
}

// CoreVideo pixel buffers are immutable while crossing this hand-off: the worker
// unlocks the buffer before publishing it and GPUI only reads it. Core Foundation
// retain/release and CVPixelBuffer are documented for cross-thread ownership.
#[cfg(target_os = "macos")]
unsafe impl Send for PreparedPreview {}

#[cfg(not(target_os = "macos"))]
struct PlatformPreviewResources {
    resizer: Resizer,
    tight_nv12: Vec<u8>,
    scaled_nv12: Vec<u8>,
}

#[cfg(target_os = "macos")]
fn new_platform_preview_resources() -> PlatformPreviewResources {
    PlatformPreviewResources::default()
}

#[cfg(not(target_os = "macos"))]
fn new_platform_preview_resources() -> PlatformPreviewResources {
    PlatformPreviewResources {
        resizer: Resizer::new(),
        tight_nv12: Vec::new(),
        scaled_nv12: Vec::new(),
    }
}

#[derive(Debug, Clone)]
pub(crate) struct PreviewViewportTracker(Arc<Mutex<PreviewViewport>>);

#[derive(Debug)]
struct PreviewViewport {
    width: f32,
    height: f32,
    painted_at: Option<Instant>,
}

impl Default for PreviewViewportTracker {
    fn default() -> Self {
        Self(Arc::new(Mutex::new(PreviewViewport {
            width: 0.0,
            height: 0.0,
            painted_at: None,
        })))
    }
}

impl PreviewViewportTracker {
    pub(crate) fn record_painted(&self, width: f32, height: f32) {
        *self.0.lock().unwrap() = PreviewViewport {
            width,
            height,
            painted_at: Some(Instant::now()),
        };
    }

    pub(crate) fn target_physical_width(&self) -> Option<f32> {
        let viewport = self.0.lock().unwrap();
        let recently_painted = viewport
            .painted_at
            .is_some_and(|painted_at| painted_at.elapsed() <= PREVIEW_PAINT_FRESHNESS);
        if !recently_painted || viewport.width <= 0.0 || viewport.height <= 0.0 {
            return None;
        }
        Some(viewport.width)
    }
}

#[derive(Default)]
struct WorkerState {
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
    last_submitted_at: Option<Instant>,
    target_width: u32,
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
            last_submitted_at: None,
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
        if frame.sequence <= self.last_submitted_sequence {
            return false;
        }
        if self
            .last_submitted_at
            .is_some_and(|submitted_at| submitted_at.elapsed() < PREVIEW_TARGET_FRAME_INTERVAL)
        {
            return false;
        }
        self.last_submitted_sequence = frame.sequence;
        self.last_submitted_at = Some(Instant::now());
        let request = PreviewRequest {
            frame: Arc::clone(frame),
            target_width: self.target_width,
        };
        let (state, ready) = &*self.shared;
        state.lock().unwrap().enqueue_latest(request);
        ready.notify_one();
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
        if let Some(worker) = self.worker.take() {
            let _ = worker.join();
        }
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

        let prepared = prepare_preview(request, &mut platform_resources);
        let mut state = shared.0.lock().unwrap();
        if state.stopped {
            return;
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
    let sequence = request.frame.sequence;

    #[cfg(target_os = "macos")]
    {
        let pixel_buffer =
            platform_resources.prepare_surface(&request.frame, request.target_width)?;
        Some(PreparedPreview {
            sequence,
            pixel_buffer,
        })
    }

    #[cfg(not(target_os = "macos"))]
    {
        let prepared = prepare_bgra(request, platform_resources)?;
        Some(PreparedPreview {
            sequence,
            width: prepared.width,
            height: prepared.height,
            bgra: prepared.bgra,
        })
    }
}

#[cfg(not(target_os = "macos"))]
#[derive(Debug)]
struct PreparedBgra {
    width: u32,
    height: u32,
    bgra: Vec<u8>,
}

#[cfg(not(target_os = "macos"))]
fn prepare_bgra(
    request: PreviewRequest,
    resources: &mut PlatformPreviewResources,
) -> Option<PreparedBgra> {
    let frame = request.frame;
    if frame.width == 0
        || frame.height == 0
        || !frame.width.is_multiple_of(2)
        || !frame.height.is_multiple_of(2)
        || frame.stride < frame.width
    {
        return None;
    }
    let y_len = (frame.stride as usize).checked_mul(frame.height as usize)?;
    let uv_len = (frame.stride as usize).checked_mul(frame.height as usize / 2)?;
    let required = y_len.checked_add(uv_len)?;
    if frame.pixel_data.len() < required {
        return None;
    }

    let output_width = request.target_width.min(frame.width) & !1;
    let output_height = u32::try_from(
        (u64::from(frame.height) * u64::from(output_width) + u64::from(frame.width) / 2)
            / u64::from(frame.width),
    )
    .ok()?
    .max(2)
        & !1;

    let (source_y, source_uv, source_stride) = if output_width < frame.width {
        resize_nv12_into(
            resources,
            &frame.pixel_data[..required],
            frame.width,
            frame.height,
            frame.stride,
            output_width,
            output_height,
        )?;
        let output_y_len = (output_width as usize).checked_mul(output_height as usize)?;
        let (y, uv) = resources.scaled_nv12.split_at(output_y_len);
        (y, uv, output_width)
    } else {
        (
            &frame.pixel_data[..y_len],
            &frame.pixel_data[y_len..required],
            frame.stride,
        )
    };
    let source = YuvBiPlanarImage {
        y_plane: source_y,
        y_stride: source_stride,
        uv_plane: source_uv,
        uv_stride: source_stride,
        width: output_width,
        height: output_height,
    };
    let bgra_stride = output_width.checked_mul(4)?;
    let bgra_len = (bgra_stride as usize).checked_mul(output_height as usize)?;
    let mut bgra = vec![0_u8; bgra_len];
    yuv_nv12_to_bgra(
        &source,
        &mut bgra,
        bgra_stride,
        YuvRange::Limited,
        YuvStandardMatrix::Bt709,
        YuvConversionMode::Balanced,
    )
    .ok()?;

    Some(PreparedBgra {
        width: output_width,
        height: output_height,
        bgra,
    })
}

#[cfg(not(target_os = "macos"))]
fn resize_nv12_into(
    resources: &mut PlatformPreviewResources,
    source: &[u8],
    source_width: u32,
    source_height: u32,
    source_stride: u32,
    output_width: u32,
    output_height: u32,
) -> Option<()> {
    let source_y_len = (source_stride as usize).checked_mul(source_height as usize)?;
    let tight_source_y_len = (source_width as usize).checked_mul(source_height as usize)?;
    let tight_source_len = tight_source_y_len.checked_mul(3)?.checked_div(2)?;
    let source = if source_stride == source_width {
        &source[..tight_source_len]
    } else {
        resources.tight_nv12.resize(tight_source_len, 0);
        for row in 0..source_height as usize {
            let source_offset = row * source_stride as usize;
            let target_offset = row * source_width as usize;
            resources.tight_nv12[target_offset..target_offset + source_width as usize]
                .copy_from_slice(&source[source_offset..source_offset + source_width as usize]);
        }
        for row in 0..source_height as usize / 2 {
            let source_offset = source_y_len + row * source_stride as usize;
            let target_offset = tight_source_y_len + row * source_width as usize;
            resources.tight_nv12[target_offset..target_offset + source_width as usize]
                .copy_from_slice(&source[source_offset..source_offset + source_width as usize]);
        }
        resources.tight_nv12.as_slice()
    };
    let output_y_len = (output_width as usize).checked_mul(output_height as usize)?;
    let output_len = output_y_len.checked_mul(3)?.checked_div(2)?;
    resources.scaled_nv12.resize(output_len, 0);
    let (output_y, output_uv) = resources.scaled_nv12.split_at_mut(output_y_len);
    let source_y = ImageRef::new(
        source_width,
        source_height,
        &source[..tight_source_y_len],
        PixelType::U8,
    )
    .ok()?;
    let mut destination_y =
        Image::from_slice_u8(output_width, output_height, output_y, PixelType::U8).ok()?;
    let options = ResizeOptions::new().resize_alg(ResizeAlg::Convolution(FilterType::CatmullRom));
    resources
        .resizer
        .resize(&source_y, &mut destination_y, Some(&options))
        .ok()?;

    let source_uv = ImageRef::new(
        source_width / 2,
        source_height / 2,
        &source[tight_source_y_len..],
        PixelType::U8x2,
    )
    .ok()?;
    let mut destination_uv = Image::from_slice_u8(
        output_width / 2,
        output_height / 2,
        output_uv,
        PixelType::U8x2,
    )
    .ok()?;
    resources
        .resizer
        .resize(&source_uv, &mut destination_uv, Some(&options))
        .ok()
}

#[cfg(test)]
mod tests {
    use super::*;
    use picoo_frame_hub::nv12_black;
    use std::time::Instant;

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
        let mut frame = VideoFrame::new(
            1,
            sequence,
            sequence * 1_000,
            sequence * 1_000,
            sequence * 1_000,
            sequence * 1_000,
            Instant::now(),
            sequence * 1_000,
            width,
            height,
            width,
            0,
            pixels,
        );
        frame.sequence = sequence;
        PreviewRequest {
            frame: Arc::new(frame),
            target_width,
        }
    }

    fn prepared(sequence: u64) -> PreparedPreview {
        let mut resources = new_platform_preview_resources();
        prepare_preview(request(sequence, 2, 2, 1280), &mut resources).expect("prepare fixture")
    }

    #[cfg(not(target_os = "macos"))]
    fn prepare_bgra_for_test(request: PreviewRequest) -> Option<PreparedBgra> {
        prepare_bgra(request, &mut new_platform_preview_resources())
    }

    #[test]
    fn pending_slot_keeps_only_the_latest_frame() {
        let mut state = WorkerState::default();
        state.enqueue_latest(request(1, 2, 2, 1280));
        state.enqueue_latest(request(2, 2, 2, 1280));
        assert_eq!(state.pending.expect("latest request").frame.sequence, 2);
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
    #[cfg(not(target_os = "macos"))]
    fn keeps_native_720p_detail_and_bt709_black() {
        let preview = prepare_bgra_for_test(request(7, 1280, 720, 1280)).expect("prepare 720p");
        assert_eq!((preview.width, preview.height), (1280, 720));
        assert_eq!(preview.bgra.len(), 1280 * 720 * 4);
        assert!(preview.bgra[0] <= 16);
        assert!(preview.bgra[1] <= 16);
        assert!(preview.bgra[2] <= 16);
        assert_eq!(preview.bgra[3], 255);
    }

    #[test]
    #[cfg(not(target_os = "macos"))]
    fn conversion_uses_bt709_limited_bgra_channel_order() {
        let width = 2;
        let height = 2;
        let mut pixels = vec![81_u8; (width * height * 3 / 2) as usize];
        pixels[(width * height) as usize..].copy_from_slice(&[90, 240]);
        let preview =
            prepare_bgra_for_test(request_with_pixels(8, width, height, 1280, pixels.into()))
                .expect("prepare red fixture");

        assert!(preview.bgra[0] < 32, "blue channel should remain dark");
        assert!(preview.bgra[1] < 40, "green channel should remain dark");
        assert!(preview.bgra[2] > 240, "red channel should be dominant");
        assert_eq!(preview.bgra[3], 255);
    }

    #[test]
    #[cfg(not(target_os = "macos"))]
    fn filtered_1080p_preview_matches_a_smaller_physical_viewport() {
        let preview = prepare_bgra_for_test(request(9, 1920, 1080, 1280)).expect("prepare 1080p");
        assert_eq!((preview.width, preview.height), (1280, 720));
        assert_eq!(preview.bgra.len(), 1280 * 720 * 4);
    }

    #[test]
    #[cfg(not(target_os = "macos"))]
    fn full_hd_viewport_keeps_native_1080p_detail() {
        let preview = prepare_bgra_for_test(request(10, 1920, 1080, 1920)).expect("prepare 1080p");
        assert_eq!((preview.width, preview.height), (1920, 1080));
        assert_eq!(preview.bgra.len(), 1920 * 1080 * 4);
    }
}
