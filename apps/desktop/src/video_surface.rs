//! Native preview surface — REQ-PICOO-NEXT-011 / REQ-PICOO-UI-004.
//! Owns the prepared platform surface, without pixel conversion or decoding.

use crate::preview_pipeline::PreparedPreview;
use gpui_kit::*;
use picoo_diagnostics::PreviewStage;

#[derive(Default)]
pub struct VideoSurface {
    native_surface: Option<SurfaceSource>,
    last_sequence: u64,
}

impl VideoSurface {
    pub fn clear(&mut self) {
        self.native_surface = None;
        self.last_sequence = 0;
    }

    pub fn present(&mut self, preview: PreparedPreview) -> bool {
        if !self.accepts_sequence(preview.sequence) {
            preview
                .diagnostics
                .record(PreviewStage::SurfaceRejected, preview.generation);
            return false;
        }
        preview
            .diagnostics
            .record(PreviewStage::SurfacePresented, preview.generation);
        self.native_surface = Some(preview.surface);
        true
    }

    pub fn render_preview(&self) -> impl IntoElement {
        if let Some(source) = &self.native_surface {
            surface(source.clone())
                .w_full()
                .h_full()
                .object_fit(ObjectFit::Contain)
                .into_any_element()
        } else {
            div().w_full().h_full().into_any_element()
        }
    }

    fn accepts_sequence(&mut self, sequence: u64) -> bool {
        if sequence <= self.last_sequence {
            false
        } else {
            self.last_sequence = sequence;
            true
        }
    }
}

#[cfg(test)]
mod tests {
    use super::VideoSurface;

    #[test]
    fn repeated_frame_sequence_does_not_request_another_render() {
        let mut surface = VideoSurface::default();

        assert!(surface.accepts_sequence(1));
        assert!(!surface.accepts_sequence(1));
    }

    #[test]
    fn clear_allows_a_new_source_to_present_from_sequence_one() {
        let mut surface = VideoSurface::default();
        assert!(surface.accepts_sequence(8));
        surface.clear();
        assert!(surface.accepts_sequence(1));
    }
}
