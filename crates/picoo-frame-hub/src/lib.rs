//! FrameHub and Shared Frame Ring — REQ-PICOO-FRAME-001..006.

mod hub;
mod nv12;
mod placeholder;
mod shared_ring;

pub use hub::{FrameHub, FrameHubError, FrameSlot, ReadyState, SLOT_COUNT};
pub use nv12::{
    normalize_rotation_degrees, nv12_mirror_horizontal, nv12_preview_bgra,
    nv12_preview_bgra_max_width, nv12_rotate_clockwise,
};
pub use placeholder::{
    color_bars_placeholder, nv12_black, nv12_byte_size, reconnecting_placeholder,
    waiting_placeholder, waiting_placeholder_for_size, PlaceholderMode, PLACEHOLDER_HEIGHT,
    PLACEHOLDER_WIDTH,
};
#[cfg(target_os = "macos")]
pub use shared_ring::{
    macos_app_group_identifier, macos_app_group_ring_path, MACOS_APP_GROUP_INFO_KEY,
    MACOS_UNSIGNED_BUILD_INFO_KEY,
};
#[cfg(target_os = "windows")]
pub use shared_ring::{windows_shared_ring_path, WINDOWS_SHARED_RING_DIRECTORY};
pub use shared_ring::{
    SharedFrameRingConsumer, SharedFrameRingProducer, SharedFrameView, SharedRingError,
    DEFAULT_MAX_FRAME_BYTES, PIXEL_FORMAT_NV12, RING_MAGIC, RING_META_SIZE, RING_READY_DONE,
    RING_SLOT_COUNT, RING_SLOT_META_SIZE, RING_VERSION,
};
