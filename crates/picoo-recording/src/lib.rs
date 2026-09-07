//! Native recording boundaries — ARCH-PICOO-MEDIA-002.
//! Segment adapters run on a dedicated recording worker, never the Receiver/UI owner.

#[cfg(target_os = "macos")]
pub mod apple;

#[derive(Debug, thiserror::Error)]
pub enum RecordingError {
    #[error("invalid recording input: {0}")]
    InvalidInput(&'static str),
    #[error("native recording failed: {0}")]
    Platform(String),
    #[error("recording finalization timed out")]
    FinalizationTimeout,
}
