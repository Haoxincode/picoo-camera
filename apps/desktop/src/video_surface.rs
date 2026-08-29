//! VideoSurface — ARCH-PICOO-UI-001 / REQ-PICOO-UI-004.
//!
//! Renders decoded FrameHub NV12 as GPUI texture; does not own decoder or network.

use std::sync::Arc;

use gpui::*;
use image::{Frame, ImageBuffer, Rgba};
use picoo_frame_hub::FrameSlot;
use smallvec::smallvec;

pub struct VideoSurface {
    render_image: Option<Arc<RenderImage>>,
    last_sequence: u64,
}

impl Default for VideoSurface {
    fn default() -> Self {
        Self {
            render_image: None,
            last_sequence: 0,
        }
    }
}

impl VideoSurface {
    pub fn update_from_slot(&mut self, slot: &FrameSlot) {
        if slot.sequence <= self.last_sequence {
            return;
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
            }
        }
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
