//! REQ-PICOO-GPU-006: native Video Processor output, without source CPU access.
mod geometry;
mod pipeline;
mod pool;

use crate::{OutputColor, RenderError, RenderSpec, Rotation, WindowsGpuContext};
use picoo_frame_hub::{ChromaSiting, ImageSize, NativeImage, NativeVideoFrame, SourceColor};
use pipeline::Pipeline;
use pool::{OutputPool, Surface};
use std::mem::ManuallyDrop;
use std::sync::Arc;
use windows::core::{IUnknown, Interface};
use windows::Win32::Foundation::RECT;
use windows::Win32::Graphics::Direct3D11::*;
use windows::Win32::Graphics::Dxgi::Common::DXGI_COLOR_SPACE_YCBCR_STUDIO_G22_LEFT_P709;

fn platform(error: windows::core::Error) -> RenderError {
    RenderError::Platform(error.to_string())
}
fn unsupported(capability: &str) -> RenderError {
    RenderError::Platform(format!("Video Processor does not support {capability}"))
}

/// Immutable completed target. GPU consumers retain a clone until their reads finish.
#[derive(Clone)]
pub struct RenderedImage {
    surface: Arc<Surface>,
    spec: RenderSpec,
}
impl RenderedImage {
    pub fn spec(&self) -> RenderSpec {
        self.spec
    }
    /// # Safety
    /// Never mutate this target or allow mutable aliases. Retain a clone until
    /// GPU reading completes. Only an output exporter/diagnostic may map it.
    pub unsafe fn texture(&self) -> &ID3D11Texture2D {
        &self.surface.0
    }
}

struct RenderOwners {
    _source: NativeImage,
    target: Arc<Surface>,
    input_view: ID3D11VideoProcessorInputView,
    output_view: ID3D11VideoProcessorOutputView,
}
// SAFETY: Source and target leases remain held until the GPU event. D3D11 view
// interfaces are free-threaded; callbacks only retain/release, never submit work.
unsafe impl Send for RenderOwners {}

/// One output worker owns this renderer and its lazy, hard three-surface pool.
/// The supplied context must be the source device; an extra sink cannot rebuild it.
pub struct WindowsRenderer {
    gpu: Arc<WindowsGpuContext>,
    device: ID3D11VideoDevice,
    context: ID3D11VideoContext1,
    spec: RenderSpec,
    pool: OutputPool,
    pipeline: Option<Pipeline>,
}
impl WindowsRenderer {
    pub fn new(gpu: Arc<WindowsGpuContext>, spec: RenderSpec) -> Result<Self, RenderError> {
        spec.validate()?;
        if spec.color != OutputColor::Bt709Limited {
            return Err(unsupported("requested output color"));
        }
        let device = gpu.device.cast().map_err(platform)?;
        let context = gpu.immediate.cast().map_err(platform)?;
        Ok(Self {
            gpu,
            device,
            context,
            spec,
            pool: OutputPool::new(spec),
            pipeline: None,
        })
    }

    /// REQ-PICOO-NEXT-009/016/029: waits only on this dedicated output worker.
    pub fn render(&mut self, frame: &NativeVideoFrame) -> Result<RenderedImage, RenderError> {
        let description = frame.description();
        if description.pixel_aspect_ratio.numerator != description.pixel_aspect_ratio.denominator {
            return Err(unsupported("non-square source pixels"));
        }
        if description.color
            != (SourceColor::Nv12Bt709Limited {
                chroma_siting: ChromaSiting::Left,
            })
        {
            return Err(RenderError::UnsupportedSourceColor);
        }
        let source = frame
            .image()
            .windows()
            .ok_or(RenderError::DeviceUnavailable)?;
        let size = ImageSize {
            width: source.width(),
            height: source.height(),
        };
        let (source_rect, dest_rect) =
            geometry::rectangles(size, description.visible_rect, self.spec)?;
        unsafe {
            let (texture, subresource) = source.texture();
            if texture
                .GetDevice()
                .map_err(platform)?
                .cast::<IUnknown>()
                .map_err(platform)?
                != self.gpu.device.cast::<IUnknown>().map_err(platform)?
            {
                return Err(unsupported("a different source D3D11 device"));
            }
            if self
                .pipeline
                .as_ref()
                .is_none_or(|pipeline| pipeline.size != size)
            {
                self.pipeline = Some(Pipeline::new(&self.device, size, self.spec)?);
            }
            let pipeline = self.pipeline.as_ref().expect("configured processor");
            let target = self.pool.acquire(&self.gpu.device)?;
            let mut input_view = None;
            self.device
                .CreateVideoProcessorInputView(
                    texture,
                    &pipeline.enumerator,
                    &D3D11_VIDEO_PROCESSOR_INPUT_VIEW_DESC {
                        FourCC: 0,
                        ViewDimension: D3D11_VPIV_DIMENSION_TEXTURE2D,
                        Anonymous: D3D11_VIDEO_PROCESSOR_INPUT_VIEW_DESC_0 {
                            Texture2D: D3D11_TEX2D_VPIV {
                                MipSlice: 0,
                                ArraySlice: subresource,
                            },
                        },
                    },
                    Some(&mut input_view),
                )
                .map_err(platform)?;
            let mut output_view = None;
            self.device
                .CreateVideoProcessorOutputView(
                    &target.0,
                    &pipeline.enumerator,
                    &D3D11_VIDEO_PROCESSOR_OUTPUT_VIEW_DESC {
                        ViewDimension: D3D11_VPOV_DIMENSION_TEXTURE2D,
                        Anonymous: D3D11_VIDEO_PROCESSOR_OUTPUT_VIEW_DESC_0 {
                            Texture2D: D3D11_TEX2D_VPOV { MipSlice: 0 },
                        },
                    },
                    Some(&mut output_view),
                )
                .map_err(platform)?;
            let owners = RenderOwners {
                _source: frame.image().clone(),
                target,
                input_view: input_view.ok_or(RenderError::DeviceUnavailable)?,
                output_view: output_view.ok_or(RenderError::DeviceUnavailable)?,
            };
            let owners = self
                .gpu
                .submit_owned(owners, |gpu, owners| {
                    gpu.with_immediate_context(|_| {
                        self.configure(&pipeline.processor, source_rect, dest_rect);
                        let mut stream = D3D11_VIDEO_PROCESSOR_STREAM {
                            Enable: true.into(),
                            pInputSurface: ManuallyDrop::new(Some(owners.input_view.clone())),
                            ..Default::default()
                        };
                        let result = self.context.VideoProcessorBlt(
                            &pipeline.processor,
                            &owners.output_view,
                            0,
                            std::slice::from_ref(&stream),
                        );
                        ManuallyDrop::drop(&mut stream.pInputSurface);
                        result.map_err(crate::WindowsCompletionError::from)
                    })
                })
                .and_then(|completion| completion.wait_on_worker())
                .map_err(|error| RenderError::Platform(error.to_string()))?;
            Ok(RenderedImage {
                surface: owners.target,
                spec: self.spec,
            })
        }
    }

    unsafe fn configure(&self, processor: &ID3D11VideoProcessor, source: RECT, dest: RECT) {
        self.context.VideoProcessorSetStreamFrameFormat(
            processor,
            0,
            D3D11_VIDEO_FRAME_FORMAT_PROGRESSIVE,
        );
        self.context
            .VideoProcessorSetStreamAutoProcessingMode(processor, 0, false);
        self.context.VideoProcessorSetStreamColorSpace1(
            processor,
            0,
            DXGI_COLOR_SPACE_YCBCR_STUDIO_G22_LEFT_P709,
        );
        self.context.VideoProcessorSetOutputColorSpace1(
            processor,
            DXGI_COLOR_SPACE_YCBCR_STUDIO_G22_LEFT_P709,
        );
        self.context.VideoProcessorSetStreamRotation(
            processor,
            0,
            self.spec.rotation != Rotation::None,
            match self.spec.rotation {
                Rotation::None => D3D11_VIDEO_PROCESSOR_ROTATION_IDENTITY,
                Rotation::Clockwise90 => D3D11_VIDEO_PROCESSOR_ROTATION_90,
                Rotation::Clockwise180 => D3D11_VIDEO_PROCESSOR_ROTATION_180,
                Rotation::Clockwise270 => D3D11_VIDEO_PROCESSOR_ROTATION_270,
            },
        );
        self.context.VideoProcessorSetStreamMirror(
            processor,
            0,
            self.spec.mirror,
            self.spec.mirror,
            false,
        );
        self.context
            .VideoProcessorSetStreamSourceRect(processor, 0, true, Some(&source));
        self.context
            .VideoProcessorSetStreamDestRect(processor, 0, true, Some(&dest));
        self.context.VideoProcessorSetOutputTargetRect(
            processor,
            true,
            Some(&RECT {
                left: 0,
                top: 0,
                right: self.spec.width as i32,
                bottom: self.spec.height as i32,
            }),
        );
        self.context.VideoProcessorSetOutputBackgroundColor(
            processor,
            false,
            &D3D11_VIDEO_COLOR {
                Anonymous: D3D11_VIDEO_COLOR_0 {
                    RGBA: D3D11_VIDEO_COLOR_RGBA {
                        R: 0.0,
                        G: 0.0,
                        B: 0.0,
                        A: 1.0,
                    },
                },
            },
        );
        self.context.VideoProcessorSetOutputAlphaFillMode(
            processor,
            D3D11_VIDEO_PROCESSOR_ALPHA_FILL_MODE_OPAQUE,
            0,
        );
    }
}

#[cfg(test)]
mod tests;
