//! Bounded desktop preview preparation — ARCH-PICOO-FRAME-001 / REQ-PICOO-UI-004.
//!
//! FrameHub remains the decoded-frame authority. This consumer keeps one pending
//! latest frame and performs SIMD color conversion and filtered scaling away
//! from the GPUI thread.

use std::sync::{Arc, Condvar, Mutex};
use std::thread::{self, JoinHandle};

use bytes::Bytes;
use fast_image_resize::images::{Image, ImageRef};
use fast_image_resize::{FilterType, PixelType, ResizeAlg, ResizeOptions, Resizer};
use picoo_frame_hub::FrameSlot;
use yuv::{yuv_nv12_to_bgra, YuvBiPlanarImage, YuvConversionMode, YuvRange, YuvStandardMatrix};

#[cfg(target_os = "macos")]
use core_foundation::{
    base::{CFType, TCFType},
    boolean::CFBoolean,
    dictionary::CFDictionary,
    number::CFNumber,
    string::CFString,
};
#[cfg(target_os = "macos")]
use core_video::{
    pixel_buffer::{
        kCVPixelBufferHeightKey, kCVPixelBufferMetalCompatibilityKey,
        kCVPixelBufferPixelFormatTypeKey, kCVPixelBufferWidthKey,
        kCVPixelFormatType_420YpCbCr8BiPlanarFullRange, CVPixelBuffer,
    },
    pixel_buffer_io_surface::kCVPixelBufferIOSurfaceCoreAnimationCompatibilityKey,
    pixel_buffer_pool::{
        kCVPixelBufferPoolAllocationThresholdKey, kCVPixelBufferPoolMinimumBufferCountKey,
        CVPixelBufferPool,
    },
    r#return::kCVReturnSuccess,
};
#[cfg(target_os = "macos")]
use yuv::{bgra_to_yuv_nv12, BufferStoreMut, YuvBiPlanarImageMut};

const PREVIEW_MIN_DETAIL_WIDTH: u32 = 1280;
const PREVIEW_MAX_DETAIL_WIDTH: u32 = 1920;

#[derive(Debug)]
struct PreviewRequest {
    sequence: u64,
    width: u32,
    height: u32,
    stride: u32,
    target_width: u32,
    pixels: Bytes,
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

#[cfg(target_os = "macos")]
const SURFACE_BUFFER_LIMIT: i32 = 3;

#[cfg(target_os = "macos")]
#[derive(Default)]
struct PlatformPreviewResources {
    pool: Option<SurfacePool>,
}

#[cfg(target_os = "macos")]
struct SurfacePool {
    width: u32,
    height: u32,
    pool: CVPixelBufferPool,
    allocation_attributes: CFDictionary<CFString, CFType>,
}

#[cfg(not(target_os = "macos"))]
#[derive(Default)]
struct PlatformPreviewResources;

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
            target_width: PREVIEW_MIN_DETAIL_WIDTH,
        }
    }

    /// Use the physical window width as a conservative upper bound for the
    /// preview surface. This avoids upscaling prepared pixels while keeping
    /// conversion work bounded to Full HD for the currently supported sources.
    pub(crate) fn set_viewport_physical_width(&mut self, width: f32) {
        self.target_width = target_width_for_viewport(width);
    }

    /// Submit a newer FrameHub slot without copying its ref-counted pixel buffer.
    /// A not-yet-started older request is replaced instead of queued.
    pub(crate) fn submit_latest(&mut self, slot: &FrameSlot) -> bool {
        if slot.sequence <= self.last_submitted_sequence {
            return false;
        }
        self.last_submitted_sequence = slot.sequence;
        let request = PreviewRequest {
            sequence: slot.sequence,
            width: slot.width,
            height: slot.height,
            stride: slot.stride,
            target_width: self.target_width,
            pixels: slot.pixel_data.clone(),
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
    let mut platform_resources = PlatformPreviewResources::default();
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
        return PREVIEW_MIN_DETAIL_WIDTH;
    }
    (viewport_physical_width.ceil() as u32)
        .clamp(PREVIEW_MIN_DETAIL_WIDTH, PREVIEW_MAX_DETAIL_WIDTH)
}

fn prepare_preview(
    request: PreviewRequest,
    platform_resources: &mut PlatformPreviewResources,
) -> Option<PreparedPreview> {
    let sequence = request.sequence;
    let prepared = prepare_bgra(request)?;

    #[cfg(target_os = "macos")]
    {
        let pixel_buffer = platform_resources.prepare_surface(&prepared)?;
        Some(PreparedPreview {
            sequence,
            pixel_buffer,
        })
    }

    #[cfg(not(target_os = "macos"))]
    {
        let _ = platform_resources;
        Some(PreparedPreview {
            sequence,
            width: prepared.width,
            height: prepared.height,
            bgra: prepared.bgra,
        })
    }
}

#[derive(Debug)]
struct PreparedBgra {
    width: u32,
    height: u32,
    bgra: Vec<u8>,
}

fn prepare_bgra(request: PreviewRequest) -> Option<PreparedBgra> {
    if request.width == 0
        || request.height == 0
        || !request.width.is_multiple_of(2)
        || !request.height.is_multiple_of(2)
        || request.stride < request.width
    {
        return None;
    }
    let y_len = (request.stride as usize).checked_mul(request.height as usize)?;
    let uv_len = (request.stride as usize).checked_mul(request.height as usize / 2)?;
    let required = y_len.checked_add(uv_len)?;
    if request.pixels.len() < required {
        return None;
    }

    let source = YuvBiPlanarImage {
        y_plane: &request.pixels[..y_len],
        y_stride: request.stride,
        uv_plane: &request.pixels[y_len..required],
        uv_stride: request.stride,
        width: request.width,
        height: request.height,
    };
    let bgra_stride = request.width.checked_mul(4)?;
    let bgra_len = (bgra_stride as usize).checked_mul(request.height as usize)?;
    let mut full_bgra = vec![0_u8; bgra_len];
    yuv_nv12_to_bgra(
        &source,
        &mut full_bgra,
        bgra_stride,
        YuvRange::Limited,
        YuvStandardMatrix::Bt709,
        YuvConversionMode::Balanced,
    )
    .ok()?;

    if request.width <= request.target_width {
        return Some(PreparedBgra {
            width: request.width,
            height: request.height,
            bgra: full_bgra,
        });
    }

    let output_width = request.target_width;
    let output_height = u32::try_from(
        (u64::from(request.height) * u64::from(output_width) + u64::from(request.width) / 2)
            / u64::from(request.width),
    )
    .ok()?
    .max(1);
    let source_image =
        ImageRef::new(request.width, request.height, &full_bgra, PixelType::U8x4).ok()?;
    let mut output_image = Image::new(output_width, output_height, PixelType::U8x4);
    let options = ResizeOptions::new().resize_alg(ResizeAlg::Convolution(FilterType::CatmullRom));
    Resizer::new()
        .resize(&source_image, &mut output_image, Some(&options))
        .ok()?;

    Some(PreparedBgra {
        width: output_width,
        height: output_height,
        bgra: output_image.into_vec(),
    })
}

#[cfg(target_os = "macos")]
impl PlatformPreviewResources {
    fn prepare_surface(&mut self, preview: &PreparedBgra) -> Option<CVPixelBuffer> {
        if self
            .pool
            .as_ref()
            .is_none_or(|pool| pool.width != preview.width || pool.height != preview.height)
        {
            self.pool = SurfacePool::new(preview.width, preview.height);
        }
        self.pool.as_ref()?.copy_bt709_bgra(preview)
    }
}

#[cfg(target_os = "macos")]
impl SurfacePool {
    fn new(width: u32, height: u32) -> Option<Self> {
        // SAFETY: these are process-lifetime CoreVideo constant keys.
        let (
            width_key,
            height_key,
            format_key,
            metal_key,
            animation_key,
            minimum_count_key,
            allocation_threshold_key,
        ) = unsafe {
            (
                cf_string(kCVPixelBufferWidthKey),
                cf_string(kCVPixelBufferHeightKey),
                cf_string(kCVPixelBufferPixelFormatTypeKey),
                cf_string(kCVPixelBufferMetalCompatibilityKey),
                cf_string(kCVPixelBufferIOSurfaceCoreAnimationCompatibilityKey),
                cf_string(kCVPixelBufferPoolMinimumBufferCountKey),
                cf_string(kCVPixelBufferPoolAllocationThresholdKey),
            )
        };

        let width_value = CFNumber::from(i64::from(width));
        let height_value = CFNumber::from(i64::from(height));
        let format_value =
            CFNumber::from(i64::from(kCVPixelFormatType_420YpCbCr8BiPlanarFullRange));
        let yes = CFBoolean::true_value();
        let minimum_count = CFNumber::from(SURFACE_BUFFER_LIMIT);
        let allocation_threshold = CFNumber::from(SURFACE_BUFFER_LIMIT);

        let buffer_attributes = CFDictionary::from_CFType_pairs(&[
            (width_key, width_value.into_CFType()),
            (height_key, height_value.into_CFType()),
            (format_key, format_value.into_CFType()),
            (metal_key, yes.clone().into_CFType()),
            (animation_key, yes.into_CFType()),
        ]);
        let pool_attributes =
            CFDictionary::from_CFType_pairs(&[(minimum_count_key, minimum_count.into_CFType())]);
        let allocation_attributes = CFDictionary::from_CFType_pairs(&[(
            allocation_threshold_key,
            allocation_threshold.into_CFType(),
        )]);
        let pool = CVPixelBufferPool::new(Some(&pool_attributes), Some(&buffer_attributes)).ok()?;

        Some(Self {
            width,
            height,
            pool,
            allocation_attributes,
        })
    }

    fn copy_bt709_bgra(&self, preview: &PreparedBgra) -> Option<CVPixelBuffer> {
        let pixel_buffer = self
            .pool
            .create_pixel_buffer_with_aux_attributes(Some(&self.allocation_attributes))
            .ok()?;
        if pixel_buffer.lock_base_address(0) != kCVReturnSuccess {
            return None;
        }

        let result = (|| unsafe {
            let y_stride = pixel_buffer.get_bytes_per_row_of_plane(0);
            let uv_stride = pixel_buffer.get_bytes_per_row_of_plane(1);
            let y_len = pixel_buffer.get_height_of_plane(0).checked_mul(y_stride)?;
            let uv_len = pixel_buffer.get_height_of_plane(1).checked_mul(uv_stride)?;
            let y_base = pixel_buffer.get_base_address_of_plane(0).cast::<u8>();
            let uv_base = pixel_buffer.get_base_address_of_plane(1).cast::<u8>();
            if y_base.is_null() || uv_base.is_null() {
                None
            } else {
                let y_plane = std::slice::from_raw_parts_mut(y_base, y_len);
                let uv_plane = std::slice::from_raw_parts_mut(uv_base, uv_len);
                let mut target = YuvBiPlanarImageMut {
                    y_plane: BufferStoreMut::Borrowed(y_plane),
                    y_stride: u32::try_from(y_stride).ok()?,
                    uv_plane: BufferStoreMut::Borrowed(uv_plane),
                    uv_stride: u32::try_from(uv_stride).ok()?,
                    width: preview.width,
                    height: preview.height,
                };
                bgra_to_yuv_nv12(
                    &mut target,
                    &preview.bgra,
                    preview.width.checked_mul(4)?,
                    YuvRange::Full,
                    // GPUI's current macOS surface shader consumes full-range BT.601.
                    YuvStandardMatrix::Bt601,
                    YuvConversionMode::Balanced,
                )
                .ok()
            }
        })();
        let unlock_status = pixel_buffer.unlock_base_address(0);
        if result.is_some() && unlock_status == kCVReturnSuccess {
            Some(pixel_buffer)
        } else {
            None
        }
    }
}

#[cfg(target_os = "macos")]
unsafe fn cf_string(value: core_foundation::string::CFStringRef) -> CFString {
    // SAFETY: caller guarantees a live Core Foundation string constant.
    unsafe { CFString::wrap_under_get_rule(value) }
}

#[cfg(test)]
mod tests {
    use super::*;
    use picoo_frame_hub::nv12_black;

    fn request(sequence: u64, width: u32, height: u32, target_width: u32) -> PreviewRequest {
        PreviewRequest {
            sequence,
            width,
            height,
            stride: width,
            target_width,
            pixels: nv12_black(width, height).into(),
        }
    }

    fn prepared(sequence: u64) -> PreparedPreview {
        let mut resources = PlatformPreviewResources::default();
        prepare_preview(
            request(sequence, 2, 2, PREVIEW_MIN_DETAIL_WIDTH),
            &mut resources,
        )
        .expect("prepare fixture")
    }

    #[test]
    fn pending_slot_keeps_only_the_latest_frame() {
        let mut state = WorkerState::default();
        state.enqueue_latest(request(1, 2, 2, PREVIEW_MIN_DETAIL_WIDTH));
        state.enqueue_latest(request(2, 2, 2, PREVIEW_MIN_DETAIL_WIDTH));
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
        assert_eq!(target_width_for_viewport(960.0), 1280);
        assert_eq!(target_width_for_viewport(1440.1), 1441);
        assert_eq!(target_width_for_viewport(2560.0), 1920);
    }

    #[test]
    fn keeps_native_720p_detail_and_bt709_black() {
        let preview = prepare_bgra(request(7, 1280, 720, 1280)).expect("prepare 720p");
        assert_eq!((preview.width, preview.height), (1280, 720));
        assert_eq!(preview.bgra.len(), 1280 * 720 * 4);
        assert!(preview.bgra[0] <= 16);
        assert!(preview.bgra[1] <= 16);
        assert!(preview.bgra[2] <= 16);
        assert_eq!(preview.bgra[3], 255);
    }

    #[test]
    fn conversion_uses_bt709_limited_bgra_channel_order() {
        let width = 2;
        let height = 2;
        let mut pixels = vec![81_u8; (width * height * 3 / 2) as usize];
        pixels[(width * height) as usize..].copy_from_slice(&[90, 240]);
        let preview = prepare_bgra(PreviewRequest {
            sequence: 8,
            width,
            height,
            stride: width,
            target_width: PREVIEW_MIN_DETAIL_WIDTH,
            pixels: pixels.into(),
        })
        .expect("prepare red fixture");

        assert!(preview.bgra[0] < 32, "blue channel should remain dark");
        assert!(preview.bgra[1] < 40, "green channel should remain dark");
        assert!(preview.bgra[2] > 240, "red channel should be dominant");
        assert_eq!(preview.bgra[3], 255);
    }

    #[test]
    fn filtered_1080p_preview_matches_a_smaller_physical_viewport() {
        let preview = prepare_bgra(request(9, 1920, 1080, 1280)).expect("prepare 1080p");
        assert_eq!((preview.width, preview.height), (1280, 720));
        assert_eq!(preview.bgra.len(), 1280 * 720 * 4);
    }

    #[test]
    fn full_hd_viewport_keeps_native_1080p_detail() {
        let preview = prepare_bgra(request(10, 1920, 1080, 1920)).expect("prepare 1080p");
        assert_eq!((preview.width, preview.height), (1920, 1080));
        assert_eq!(preview.bgra.len(), 1920 * 1080 * 4);
    }

    #[cfg(target_os = "macos")]
    #[test]
    fn macos_surface_pool_is_hard_bounded_and_recycles() {
        let preview = PreparedBgra {
            width: 2,
            height: 2,
            bgra: vec![0, 0, 0, 255, 0, 0, 0, 255, 0, 0, 0, 255, 0, 0, 0, 255],
        };
        let mut resources = PlatformPreviewResources::default();
        let first = resources.prepare_surface(&preview).expect("first surface");
        let second = resources.prepare_surface(&preview).expect("second surface");
        let third = resources.prepare_surface(&preview).expect("third surface");

        assert_eq!(first.get_width(), 2);
        assert_eq!(first.get_height(), 2);
        assert_eq!(
            first.get_pixel_format(),
            kCVPixelFormatType_420YpCbCr8BiPlanarFullRange
        );
        assert!(resources.prepare_surface(&preview).is_none());

        drop(first);
        assert!(resources.prepare_surface(&preview).is_some());
        drop((second, third));
    }
}
