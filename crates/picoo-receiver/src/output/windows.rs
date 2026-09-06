//! Windows rendering and CPU export stay on the output worker and source device.
use picoo_frame_hub::{NativeVideoFrame, Rotation};
use picoo_gpu::{CpuExporter, CpuImage, OutputColor, OutputFormat, RenderSpec, WindowsRenderer};
use std::sync::Arc;

pub(super) struct Resources {
    owner: (u64, u64),
    spec: RenderSpec,
    renderer: WindowsRenderer,
    exporter: Option<CpuExporter>,
}

pub(super) fn prepare(
    resources: &mut Option<Resources>,
    frame: &NativeVideoFrame,
) -> Result<Arc<CpuImage>, String> {
    let description = frame.description();
    let (mut width, mut height) = (
        description.visible_rect.width,
        description.visible_rect.height,
    );
    if matches!(
        description.transform.rotation,
        Rotation::Clockwise90 | Rotation::Clockwise270
    ) {
        std::mem::swap(&mut width, &mut height);
    }
    let identity = frame.identity();
    let owner = (identity.connection_generation, identity.decoder_generation);
    let spec = RenderSpec {
        width,
        height,
        rotation: description.transform.rotation,
        mirror: description.transform.mirror,
        format: OutputFormat::Nv12,
        color: OutputColor::Bt709Limited,
    };
    if resources
        .as_ref()
        .is_none_or(|current| current.owner != owner || current.spec != spec)
    {
        *resources = Some(Resources {
            owner,
            spec,
            renderer: WindowsRenderer::for_source(frame.image(), spec)
                .map_err(|e| e.to_string())?,
            exporter: None,
        });
    }
    let resources = resources.as_mut().unwrap();
    let image = resources
        .renderer
        .render(frame)
        .map_err(|e| e.to_string())?;
    if resources.exporter.is_none() {
        resources.exporter = Some(CpuExporter::for_image(&image).map_err(|e| e.to_string())?);
    }
    resources
        .exporter
        .as_mut()
        .unwrap()
        .export(&image)
        .map_err(|e| e.to_string())
}
