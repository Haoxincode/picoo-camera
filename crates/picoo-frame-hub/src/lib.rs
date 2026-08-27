//! FrameHub and Shared Frame Ring — REQ-PICOO-FRAME-001..005.

mod hub;
mod nv12;
mod placeholder;
mod shared_ring;

pub use hub::{FrameHub, FrameHubError, FrameSlot, ReadyState, SLOT_COUNT};
pub use nv12::{
    normalize_rotation_degrees, nv12_mirror_horizontal, nv12_preview_rgba,
    nv12_preview_rgba_max_width, nv12_rotate_clockwise,
};
pub use placeholder::{
    nv12_black, nv12_byte_size, reconnecting_placeholder, waiting_placeholder, PLACEHOLDER_HEIGHT,
    PLACEHOLDER_WIDTH,
};
pub use shared_ring::{
    SharedFrameRingConsumer, SharedFrameRingProducer, SharedFrameView, SharedRingError,
    DEFAULT_MAX_FRAME_BYTES, PIXEL_FORMAT_NV12, RING_MAGIC, RING_VERSION,
};
