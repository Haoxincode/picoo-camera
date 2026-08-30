//! VideoSurface — ARCH-PICOO-UI-001 / REQ-PICOO-UI-004.
//!
//! Renders decoded FrameHub NV12 as GPUI texture; does not own decoder or network.

use std::sync::Arc;

use gpui::*;
use image::{Frame, ImageBuffer, Rgba};
use picoo_frame_hub::FrameSlot;
use smallvec::smallvec;

#[derive(Default)]
pub struct VideoSurface {
    render_image: Option<Arc<RenderImage>>,
    last_sequence: u64,
}

impl VideoSurface {
    /// Update the preview only for a newer frame and report whether rendering changed.
    pub fn update_from_slot(&mut self, slot: &FrameSlot) -> bool {
        if slot.sequence <= self.last_sequence {
            return false;
        }
        self.last_sequence = slot.sequence;
        if let Some((width, height, rgba)) = picoo_frame_hub::nv12_preview_rgba(
            slot.width,
            slot.height,
            slot.stride,
            slot.pixel_data.as_ref(),
        ) {
            if let Some(buffer) = ImageBuffer::<Rgba<u8>, Vec<u8>>::from_raw(width, height, rgba) {
                let frame = Frame::new(buffer);
                self.render_image = Some(Arc::new(RenderImage::new(smallvec![frame])));
                return true;
            }
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
    use picoo_frame_hub::{nv12_black, FrameSlot, ReadyState};

    #[test]
    fn repeated_frame_sequence_does_not_request_another_render() {
        let frame = FrameSlot {
            sequence: 1,
            timestamp_us: 1,
            width: 2,
            height: 2,
            stride: 2,
            rotation: 0,
            pixel_data: nv12_black(2, 2).into(),
            ready_state: ReadyState::Ready,
        };
        let mut surface = VideoSurface::default();

        assert!(surface.update_from_slot(&frame));
        assert!(!surface.update_from_slot(&frame));
    }
}
