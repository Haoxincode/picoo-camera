//! Apple native render/export adapter for the shared CPU output worker.
use picoo_frame_hub::NativeVideoFrame;
use picoo_gpu::{AppleRenderer, CpuExporter, CpuImage, OutputColor, RenderSpec, Rotation};
use std::sync::Arc;

pub(super) struct Resources {
    spec: RenderSpec,
    renderer: AppleRenderer,
    exporter: CpuExporter,
}
pub(super) fn prepare(
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
        format: picoo_gpu::OutputFormat::Nv12,
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
