use crate::{
    App, Bounds, Element, ElementId, GlobalElementId, InspectorElementId, IntoElement, LayoutId,
    ObjectFit, Pixels, Style, StyleRefinement, Styled, Window,
};
#[cfg(target_os = "macos")]
use core_video::pixel_buffer::CVPixelBuffer;
use refineable::Refineable;
#[cfg(target_os = "windows")]
mod direct3d;
#[cfg(target_os = "windows")]
pub use direct3d::*;

/// A source of a surface's content.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum SurfaceSource {
    /// An immutable BGRA texture supplied with explicit GPU-read ownership.
    #[cfg(target_os = "windows")]
    Direct3D(Direct3DSurface),
    /// A macOS image buffer from CoreVideo
    #[cfg(target_os = "macos")]
    Surface(CVPixelBuffer),
}

#[cfg(target_os = "macos")]
impl From<CVPixelBuffer> for SurfaceSource {
    fn from(value: CVPixelBuffer) -> Self {
        SurfaceSource::Surface(value)
    }
}

#[cfg(target_os = "windows")]
impl From<Direct3DSurface> for SurfaceSource {
    fn from(value: Direct3DSurface) -> Self {
        Self::Direct3D(value)
    }
}

/// A surface element.
pub struct Surface {
    source: SurfaceSource,
    object_fit: ObjectFit,
    style: StyleRefinement,
}

/// Create a new surface element.
#[cfg(any(target_os = "macos", target_os = "windows"))]
pub fn surface(source: impl Into<SurfaceSource>) -> Surface {
    Surface {
        source: source.into(),
        object_fit: ObjectFit::Contain,
        style: Default::default(),
    }
}

impl Surface {
    /// Set the object fit for the image.
    pub fn object_fit(mut self, object_fit: ObjectFit) -> Self {
        self.object_fit = object_fit;
        self
    }
}

impl Element for Surface {
    type RequestLayoutState = ();
    type PrepaintState = ();

    fn id(&self) -> Option<ElementId> {
        None
    }

    fn source_location(&self) -> Option<&'static core::panic::Location<'static>> {
        None
    }

    fn request_layout(
        &mut self,
        _global_id: Option<&GlobalElementId>,
        _inspector_id: Option<&InspectorElementId>,
        window: &mut Window,
        cx: &mut App,
    ) -> (LayoutId, Self::RequestLayoutState) {
        let mut style = Style::default();
        style.refine(&self.style);
        let layout_id = window.request_layout(style, [], cx);
        (layout_id, ())
    }

    fn prepaint(
        &mut self,
        _global_id: Option<&GlobalElementId>,
        _inspector_id: Option<&InspectorElementId>,
        _bounds: Bounds<Pixels>,
        _request_layout: &mut Self::RequestLayoutState,
        _window: &mut Window,
        _cx: &mut App,
    ) -> Self::PrepaintState {
    }

    fn paint(
        &mut self,
        _global_id: Option<&GlobalElementId>,
        _inspector_id: Option<&InspectorElementId>,
        #[cfg_attr(
            not(any(target_os = "macos", target_os = "windows")),
            allow(unused_variables)
        )]
        bounds: Bounds<Pixels>,
        _: &mut Self::RequestLayoutState,
        _: &mut Self::PrepaintState,
        #[cfg_attr(
            not(any(target_os = "macos", target_os = "windows")),
            allow(unused_variables)
        )]
        window: &mut Window,
        _: &mut App,
    ) {
        match &self.source {
            #[cfg(target_os = "macos")]
            SurfaceSource::Surface(surface) => {
                let size = crate::size(surface.get_width().into(), surface.get_height().into());
                let new_bounds = self.object_fit.get_bounds(bounds, size);
                // TODO: Add support for corner_radii
                window.paint_surface(new_bounds, surface.clone());
            }
            #[cfg(target_os = "windows")]
            SurfaceSource::Direct3D(surface) => {
                let size = surface.size();
                if size.width <= crate::DevicePixels(0) || size.height <= crate::DevicePixels(0) {
                    return;
                }
                let new_bounds = self.object_fit.get_bounds(bounds, size);
                window.paint_surface(new_bounds, surface.clone());
            }
            #[allow(unreachable_patterns)]
            _ => {}
        }
    }
}

impl IntoElement for Surface {
    type Element = Self;

    fn into_element(self) -> Self::Element {
        self
    }
}

impl Styled for Surface {
    fn style(&mut self) -> &mut StyleRefinement {
        &mut self.style
    }
}
