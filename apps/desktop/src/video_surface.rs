//! Native preview surface — REQ-PICOO-NEXT-011 / REQ-PICOO-UI-004.
//! Owns the prepared platform surface, without pixel conversion or decoding.

use crate::preview_pipeline::PreparedPreview;
use gpui_kit::*;

#[derive(Default)]
pub struct VideoSurface {
    native_surface: Option<SurfaceSource>,
    last_sequence: u64,
}

impl VideoSurface {
    pub fn clear(&mut self, _cx: &mut App) {
        self.native_surface = None;
    }

    pub fn present(&mut self, preview: PreparedPreview, _cx: &mut App) -> bool {
        if !self.accepts_sequence(preview.sequence) {
            return false;
        }
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
}
