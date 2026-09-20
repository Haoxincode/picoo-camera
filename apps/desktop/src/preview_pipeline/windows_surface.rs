//! Native Windows preview uses the Decoder device, then GPUI's shared read seam.
use gpui_kit::{size, DevicePixels, Direct3DSurface, Direct3DSurfaceSource, Size};
use picoo_frame_hub::{NativeVideoFrame, Rotation};
use picoo_gpu::{
    OutputColor, OutputFormat, RenderSpec, WindowsDisplayImage, WindowsDisplayReader,
    WindowsRenderer,
};
use std::sync::Arc;
use windows::Win32::Graphics::Direct3D11::{ID3D11Device, ID3D11ShaderResourceView};

#[derive(Default)]
pub(super) struct PlatformPreviewResources {
    prepared: Option<Resources>,
    reader: Arc<WindowsDisplayReader>,
}
struct Resources {
    owner: (u64, u64),
    spec: RenderSpec,
    renderer: WindowsRenderer,
}
impl PlatformPreviewResources {
    pub(super) fn prepare_surface(
        &mut self,
        frame: &NativeVideoFrame,
        target_width: u32,
    ) -> Option<Direct3DSurface> {
        self.prepare(frame, target_width)
            .map_err(|error| tracing::warn!(%error, "native Windows preview failed"))
            .ok()
    }
    fn prepare(
        &mut self,
        frame: &NativeVideoFrame,
        target_width: u32,
    ) -> Result<Direct3DSurface, picoo_gpu::RenderError> {
        let description = frame.description();
        let rect = description.visible_rect;
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
            format: OutputFormat::Bgra8,
            color: OutputColor::RgbFullG22Bt709,
        };
        let identity = frame.identity();
        let owner = (identity.connection_generation, identity.decoder_generation);
        if self
            .prepared
            .as_ref()
            .is_none_or(|current| current.spec != spec || current.owner != owner)
        {
            self.prepared = Some(Resources {
                owner,
                spec,
                renderer: WindowsRenderer::for_source(frame.image(), spec)?,
            });
        }
        let image = self.prepared.as_mut().unwrap().renderer.render(frame)?;
        Ok(Direct3DSurface::new(Surface(self.reader.image(image)?)))
    }
}

#[derive(Debug)]
struct Surface(WindowsDisplayImage);
// SAFETY: WindowsDisplayImage owns the original output image and keyed access
// through the existing bounded GPU completion mechanism, also on draw failure.
unsafe impl Direct3DSurfaceSource for Surface {
    fn size(&self) -> Size<DevicePixels> {
        let spec = self.0.spec();
        size(
            DevicePixels(spec.width as i32),
            DevicePixels(spec.height as i32),
        )
    }
    unsafe fn with_read(
        &self,
        device: &ID3D11Device,
        read: &mut dyn FnMut(&ID3D11ShaderResourceView) -> anyhow::Result<()>,
    ) -> anyhow::Result<bool> {
        self.0
            .with_read(device, |view| {
                read(view).map_err(|error| picoo_gpu::RenderError::Platform(error.to_string()))
            })
            .map_err(Into::into)
    }
}
