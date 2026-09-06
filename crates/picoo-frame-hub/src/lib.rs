//! LatestFrameStore and Shared Frame Ring — REQ-PICOO-FRAME-001..011.

mod frame_buffer_pool;
mod frame_bus;
mod latest_frame_store;
mod native_frame;
mod native_image;
mod nv12;
mod placeholder;
mod shared_ring;

pub use frame_buffer_pool::{
    FrameBuffer, FrameBufferPool, FrameBufferPoolStats, DEFAULT_FRAME_BUFFER_POOL_BUFFERS,
    DEFAULT_FRAME_BUFFER_POOL_BYTES,
};
pub use frame_bus::{
    FrameBus, NativeFrameSubscription, SubscriptionAlreadyActive, SubscriptionEnd,
};
pub use latest_frame_store::{LatestFrameStore, VideoFrame};
pub use native_frame::{
    ChromaSiting, FrameDescription, FrameIdentity, FrameTimeline, ImageSize,
    InvalidFrameDescription, NativeVideoFrame, PixelAspectRatio, PresentationTransform, Rotation,
    SourceColor, VisibleRect,
};
pub use native_image::NativeImage;
#[cfg(target_os = "macos")]
pub use native_image::{ApplePixelBufferLease, NativeImageError};
#[cfg(target_os = "windows")]
pub use native_image::{D3D11ImageLease, NativeImageError};
pub use nv12::{
    normalize_rotation_degrees, transform_nv12, transform_nv12_with_pool, Nv12TransformError,
    TransformedNv12,
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
    RingContentFence, RingPublishOutcome, SharedFrameRingConsumer, SharedFrameRingProducer,
    SharedFrameRingWriter, SharedFrameView, SharedRingError, SharedRingSubmitOutcome,
    SharedRingWriterEvent, SharedRingWriterStats, DEFAULT_MAX_FRAME_BYTES, PIXEL_FORMAT_NV12,
    RING_MAGIC, RING_META_SIZE, RING_READY_DONE, RING_SLOT_COUNT, RING_SLOT_META_SIZE,
};
