//! Apple VCam preparation on a fixed CMIO sink layout.

use picoo_frame_hub::NativeVideoFrame;
use picoo_gpu::{
    AppleCpuBridge, AppleRenderer, CpuBridgedImage, OutputColor, RenderSpec, RenderedImage,
    Rotation,
};

use super::backend::OutputBackend;
use super::macos_cmio::SinkLayout;

pub(super) struct Resources {
    spec: RenderSpec,
    renderer: AppleRenderer,
    cpu_bridge: Option<AppleCpuBridge>,
}

pub(super) enum PreparedImage {
    Native(RenderedImage),
    Cpu(CpuBridgedImage),
}

pub(super) enum PrepareError {
    Backpressure,
    Failed(String),
}

impl PreparedImage {
    pub(super) unsafe fn pixel_buffer(&self) -> &objc2_core_video::CVPixelBuffer {
        match self {
            Self::Native(image) => unsafe { image.pixel_buffer() },
            Self::Cpu(image) => unsafe { image.pixel_buffer() },
        }
    }
}

pub(super) fn prepare(
    resources: &mut Option<Resources>,
    frame: &NativeVideoFrame,
    layout: SinkLayout,
    backend: OutputBackend,
) -> Result<PreparedImage, PrepareError> {
    let description = frame.description();
    if description.pixel_aspect_ratio.numerator != description.pixel_aspect_ratio.denominator {
        return Err(PrepareError::Failed(
            "unsupported native source pixel aspect".into(),
        ));
    }
    let spec = RenderSpec {
        width: layout.width,
        height: layout.height,
        rotation: description.transform.rotation,
        mirror: description.transform.mirror,
        color: OutputColor::Bt709Limited,
        format: picoo_gpu::OutputFormat::Nv12,
    };
    if resources
        .as_ref()
        .is_none_or(|current| current.spec != spec)
    {
        *resources = Some(Resources {
            spec,
            renderer: AppleRenderer::new(spec).map_err(|error| match error {
                picoo_gpu::RenderError::PoolFull => PrepareError::Backpressure,
                error => PrepareError::Failed(error.to_string()),
            })?,
            cpu_bridge: None,
        });
    }
    let resources = resources.as_mut().expect("matching Apple resources");
    let rendered = if backend == OutputBackend::CpuBridge {
        if resources.cpu_bridge.is_none() {
            resources.cpu_bridge =
                Some(AppleCpuBridge::new(spec).map_err(|error| match error {
                    picoo_gpu::RenderError::PoolFull => PrepareError::Backpressure,
                    error => PrepareError::Failed(error.to_string()),
                })?);
        }
        match resources
            .cpu_bridge
            .as_mut()
            .expect("CPU bridge initialized")
            .has_capacity()
        {
            Ok(true) => {}
            Ok(false) => return Err(PrepareError::Backpressure),
            Err(error) => return Err(PrepareError::Failed(error.to_string())),
        }
        resources
            .renderer
            .render_frame(frame)
            .map_err(|error| match error {
                picoo_gpu::RenderError::PoolFull => PrepareError::Backpressure,
                error => PrepareError::Failed(error.to_string()),
            })?
    } else {
        resources
            .renderer
            .render_frame(frame)
            .map_err(|error| match error {
                picoo_gpu::RenderError::PoolFull => PrepareError::Backpressure,
                error => PrepareError::Failed(error.to_string()),
            })?
    };
    match backend {
        OutputBackend::GpuNative => Ok(PreparedImage::Native(rendered)),
        OutputBackend::CpuBridge => {
            let bridged = resources
                .cpu_bridge
                .as_mut()
                .expect("CPU bridge initialized")
                .export(&rendered)
                .map_err(|error| match error {
                    picoo_gpu::RenderError::PoolFull => PrepareError::Backpressure,
                    error => PrepareError::Failed(error.to_string()),
                })?;
            Ok(PreparedImage::Cpu(bridged))
        }
    }
}

pub(super) fn placeholder_frame(
    mode: picoo_frame_hub::PlaceholderMode,
    reconnecting: bool,
) -> Result<NativeVideoFrame, String> {
    use objc2_core_foundation::{CFDictionary, CFRetained, CFString, CFType};
    use objc2_core_video::*;
    use picoo_frame_hub::{
        ApplePixelBufferLease, ChromaSiting, FrameDescription, FrameIdentity, ImageSize,
        NativeImage, PixelAspectRatio, PresentationTransform, SourceColor, VisibleRect,
        PLACEHOLDER_HEIGHT, PLACEHOLDER_WIDTH,
    };
    use std::ptr::{self, NonNull};

    let surface = CFDictionary::<CFString, CFType>::empty();
    let attributes = unsafe {
        CFDictionary::<CFString, CFType>::from_slices(
            &[kCVPixelBufferIOSurfacePropertiesKey],
            &[surface.as_ref()],
        )
    };
    let mut raw = ptr::null_mut();
    let status = unsafe {
        CVPixelBufferCreate(
            None,
            PLACEHOLDER_WIDTH as usize,
            PLACEHOLDER_HEIGHT as usize,
            kCVPixelFormatType_420YpCbCr8BiPlanarVideoRange,
            Some(attributes.as_opaque()),
            NonNull::from(&mut raw),
        )
    };
    if status != 0 {
        return Err(format!("placeholder CVPixelBufferCreate: {status}"));
    }
    let raw = NonNull::new(raw).ok_or_else(|| "null placeholder pixel buffer".to_owned())?;
    let buffer = unsafe { CFRetained::from_raw(raw) };
    let pixels = if reconnecting {
        mode.reconnecting_frame()
    } else {
        mode.waiting_frame()
    };
    let width = PLACEHOLDER_WIDTH as usize;
    let height = PLACEHOLDER_HEIGHT as usize;
    let status = unsafe { CVPixelBufferLockBaseAddress(&buffer, CVPixelBufferLockFlags::empty()) };
    if status != 0 {
        return Err(format!("placeholder write lock: {status}"));
    }
    let copied = (|| {
        for plane in 0..2 {
            let rows = if plane == 0 { height } else { height / 2 };
            let offset = if plane == 0 { 0 } else { width * height };
            let stride = CVPixelBufferGetBytesPerRowOfPlane(&buffer, plane);
            let base = CVPixelBufferGetBaseAddressOfPlane(&buffer, plane).cast::<u8>();
            if base.is_null() || stride < width {
                return false;
            }
            for row in 0..rows {
                let source = &pixels[offset + row * width..offset + (row + 1) * width];
                let destination =
                    unsafe { std::slice::from_raw_parts_mut(base.add(row * stride), width) };
                destination.copy_from_slice(source);
            }
        }
        true
    })();
    let unlock =
        unsafe { CVPixelBufferUnlockBaseAddress(&buffer, CVPixelBufferLockFlags::empty()) };
    if !copied || unlock != 0 {
        return Err(format!("placeholder copy failed (unlock {unlock})"));
    }
    unsafe {
        for (key, value) in [
            (
                kCVImageBufferYCbCrMatrixKey,
                kCVImageBufferYCbCrMatrix_ITU_R_709_2,
            ),
            (
                kCVImageBufferColorPrimariesKey,
                kCVImageBufferColorPrimaries_ITU_R_709_2,
            ),
            (
                kCVImageBufferTransferFunctionKey,
                kCVImageBufferTransferFunction_ITU_R_709_2,
            ),
        ] {
            buffer.set_attachment(key, value.as_ref(), CVAttachmentMode::ShouldPropagate);
        }
    }
    let image = NativeImage::Apple(
        unsafe { ApplePixelBufferLease::retain_completed(&buffer) }
            .map_err(|error| error.to_string())?,
    );
    NativeVideoFrame::new(
        FrameIdentity {
            connection_generation: 0,
            stream_epoch: 0,
            decoder_generation: 0,
            frame_id: u64::from(reconnecting) + 1,
        },
        0,
        FrameDescription {
            coded_size: ImageSize {
                width: PLACEHOLDER_WIDTH,
                height: PLACEHOLDER_HEIGHT,
            },
            visible_rect: VisibleRect {
                x: 0,
                y: 0,
                width: PLACEHOLDER_WIDTH,
                height: PLACEHOLDER_HEIGHT,
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
            config_revision: 0,
        },
        image,
        picoo_frame_hub::FrameTimeline {
            encoded_at_us: 0,
            received_at_us: 0,
            decode_submitted_at_us: 0,
            decoded_at: std::time::Instant::now(),
        },
    )
    .map_err(|error| error.to_string())
}
