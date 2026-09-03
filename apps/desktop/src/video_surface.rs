//! VideoSurface — ARCH-PICOO-UI-001 / REQ-PICOO-UI-004.
//!
//! Renders a prepared platform surface; does not own pixel conversion, decoder, or network.

#[cfg(not(target_os = "macos"))]
use std::sync::Arc;

use gpui::*;
#[cfg(not(target_os = "macos"))]
use image::{Frame, ImageBuffer, Rgba};
#[cfg(not(target_os = "macos"))]
use smallvec::smallvec;

use crate::preview_pipeline::PreparedPreview;

#[derive(Default)]
pub struct VideoSurface {
    #[cfg(target_os = "macos")]
    pixel_buffer: Option<core_video::pixel_buffer::CVPixelBuffer>,
    #[cfg(not(target_os = "macos"))]
    render_image: Option<Arc<RenderImage>>,
    last_sequence: u64,
}

impl VideoSurface {
    /// Take ownership of a prepared frame and report whether rendering changed.
    pub fn present(&mut self, preview: PreparedPreview, cx: &mut App) -> bool {
        if !self.accepts_sequence(preview.sequence) {
            return false;
        }

        #[cfg(target_os = "macos")]
        {
            let _ = cx;
            self.pixel_buffer = Some(preview.pixel_buffer);
            true
        }

        #[cfg(not(target_os = "macos"))]
        {
            // GPUI's RenderImage stores `image::Frame`, but its raw upload contract is BGRA.
            if let Some(buffer) = ImageBuffer::<Rgba<u8>, Vec<u8>>::from_raw(
                preview.width,
                preview.height,
                preview.bgra,
            ) {
                let frame = Frame::new(buffer);
                let next = Arc::new(RenderImage::new(smallvec![frame]));
                if let Some(previous) = self.render_image.replace(next) {
                    // RenderImage is a static-asset API. Explicitly evict the previous
                    // frame so its atlas tile cannot accumulate during a long stream.
                    cx.drop_image(previous, None);
                }
                return true;
            }
            false
        }
    }

    #[cfg(target_os = "macos")]
    pub fn render_preview(&self) -> impl IntoElement {
        if let Some(pixel_buffer) = &self.pixel_buffer {
            surface(pixel_buffer.clone())
                .w_full()
                .h_full()
                .object_fit(ObjectFit::Contain)
                .into_any_element()
        } else {
            empty_preview()
        }
    }

    #[cfg(not(target_os = "macos"))]
    pub fn render_preview(&self) -> impl IntoElement {
        if let Some(image) = &self.render_image {
            img(ImageSource::Render(image.clone()))
                .w_full()
                .h_full()
                .object_fit(ObjectFit::Contain)
                .into_any_element()
        } else {
            empty_preview()
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

fn empty_preview() -> AnyElement {
    div().w_full().h_full().into_any_element()
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
