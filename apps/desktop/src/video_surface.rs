//! VideoSurface — ARCH-PICOO-UI-001 / REQ-PICOO-UI-004.
//!
//! Renders a prepared GPUI texture; does not own pixel conversion, decoder, or network.

use std::sync::Arc;

use gpui::*;
use image::{Frame, ImageBuffer, Rgba};
use smallvec::smallvec;

use crate::preview_pipeline::PreparedPreview;

#[derive(Default)]
pub struct VideoSurface {
    render_image: Option<Arc<RenderImage>>,
    last_sequence: u64,
}

impl VideoSurface {
    /// Take ownership of a prepared frame and report whether rendering changed.
    pub fn present(&mut self, preview: PreparedPreview) -> bool {
        if preview.sequence <= self.last_sequence {
            return false;
        }
        self.last_sequence = preview.sequence;
        // GPUI's RenderImage stores `image::Frame`, but its raw upload contract is BGRA.
        if let Some(buffer) =
            ImageBuffer::<Rgba<u8>, Vec<u8>>::from_raw(preview.width, preview.height, preview.bgra)
        {
            let frame = Frame::new(buffer);
            self.render_image = Some(Arc::new(RenderImage::new(smallvec![frame])));
            return true;
        }
        false
    }

    pub fn render_preview(&self) -> impl IntoElement {
        if let Some(image) = &self.render_image {
            img(ImageSource::Render(image.clone()))
                .w_full()
                .h_full()
                .object_fit(ObjectFit::Contain)
                .into_any_element()
        } else {
            div()
                .w_full()
                .h_full()
                .flex()
                .items_center()
                .justify_center()
                .child("等待视频帧…")
                .into_any_element()
        }
    }
}

#[cfg(test)]
mod tests {
    use super::VideoSurface;
    use crate::preview_pipeline::PreparedPreview;

    #[test]
    fn repeated_frame_sequence_does_not_request_another_render() {
        let frame = PreparedPreview {
            sequence: 1,
            width: 2,
            height: 2,
            bgra: vec![0; 16],
        };
        let mut surface = VideoSurface::default();

        assert!(surface.present(frame));
        assert!(!surface.present(PreparedPreview {
            sequence: 1,
            width: 2,
            height: 2,
            bgra: vec![0; 16],
        }));
    }
}
