//! Native GPU preview preparation. No CPU image map, scaling or conversion.

use core_foundation::base::TCFType;
use core_video::pixel_buffer::CVPixelBuffer;
use picoo_frame_hub::NativeVideoFrame;
use picoo_gpu::{AppleRenderer, OutputColor, RenderSpec, Rotation};

#[derive(Default)]
pub(super) struct PlatformPreviewResources {
    renderer: Option<AppleRenderer>,
    spec: Option<RenderSpec>,
}

impl PlatformPreviewResources {
    pub(super) fn prepare_surface(
        &mut self,
        frame: &NativeVideoFrame,
        target_width: u32,
    ) -> Option<CVPixelBuffer> {
        let description = frame.description();
        let rect = description.visible_rect;
        if rect.width == 0
            || rect.height == 0
            || description.pixel_aspect_ratio.numerator
                != description.pixel_aspect_ratio.denominator
        {
            tracing::warn!(
                visible = ?rect,
                par = ?description.pixel_aspect_ratio,
                "native preview rejected remaining crop or non-square pixels"
            );
            return None;
        }
        let (mut width, mut height) = (rect.width, rect.height);
        if matches!(
            description.transform.rotation,
            Rotation::Clockwise90 | Rotation::Clockwise270
        ) {
            std::mem::swap(&mut width, &mut height);
        }
        let output_width = target_width.min(width).max(2) & !1;
        let output_height =
            ((u64::from(height) * u64::from(output_width) / u64::from(width)) as u32).max(2) & !1;
        let spec = RenderSpec {
            width: output_width,
            height: output_height,
            rotation: description.transform.rotation,
            mirror: description.transform.mirror,
            color: OutputColor::Bt601Full,
            format: picoo_gpu::OutputFormat::Nv12,
        };
        if self.spec != Some(spec) {
            self.renderer = Some(
                AppleRenderer::new(spec)
                    .map_err(|error| tracing::warn!(%error, "native preview context failed"))
                    .ok()?,
            );
            self.spec = Some(spec);
        }
        let output = match self.renderer.as_mut()?.render_frame(frame) {
            Ok(output) => output,
            Err(picoo_gpu::RenderError::PoolFull) => {
                // Reconnect and GPUI can keep the previous pool's three
                // outputs alive. A new renderer owns a fresh allocation set.
                tracing::warn!("native preview pool full; recreating renderer");
                self.renderer = Some(
                    AppleRenderer::new(spec)
                        .map_err(|error| tracing::warn!(%error, "native preview context failed"))
                        .ok()?,
                );
                self.spec = Some(spec);
                self.renderer
                    .as_mut()?
                    .render_frame(frame)
                    .map_err(|error| tracing::warn!(%error, "native preview render failed"))
                    .ok()?
            }
            Err(error) => {
                tracing::warn!(%error, "native preview render failed");
                return None;
            }
        };
        // SAFETY: The completed immutable output remains retained during this
        // bridge. GPUI's binding receives its own +1 reference to the same CF
        // object, with no pixel mapping or intermediate image allocation.
        Some(unsafe {
            CVPixelBuffer::wrap_under_get_rule(
                (output.pixel_buffer() as *const objc2_core_video::CVPixelBuffer)
                    .cast_mut()
                    .cast(),
            )
        })
    }
}
