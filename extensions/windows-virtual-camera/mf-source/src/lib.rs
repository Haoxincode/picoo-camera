//! Pure-Rust Windows Media Foundation virtual-camera source.
//!
//! REQ-PICOO-STACK-006 / REQ-PICOO-VCAM-002 / REQ-PICOO-VCAM-003.

mod format;
#[cfg(any(windows, test))]
mod frame_provider;
#[cfg(any(windows, test))]
mod metrics;
#[cfg(any(windows, test))]
mod sample_clock;
mod sample_copy;

#[cfg(test)]
mod contract_harness;

#[cfg(windows)]
mod windows_source;

pub use format::{
    is_supported_output_size, nv12_len, DEFAULT_HEIGHT, DEFAULT_WIDTH, FRAME_RATE_DEN,
    FRAME_RATE_NUM, SAMPLE_DURATION_100NS,
};
pub use sample_copy::{copy_prepared_frame, SampleCopyError};

pub const DEFAULT_RING_NAME: &str = "picoo-camera-v1";
pub const FRIENDLY_NAME: &str = "Picoo Camera";

#[cfg(windows)]
pub use windows_source::*;
