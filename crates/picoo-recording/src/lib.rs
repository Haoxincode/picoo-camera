//! Native recording boundaries — ARCH-PICOO-MEDIA-002.
//! Segment adapters run on a dedicated recording worker, never the Receiver/UI owner.

#[cfg(target_os = "macos")]
pub mod apple;
pub mod bundle;
pub mod configuration_wait;
#[cfg(target_os = "macos")]
pub mod encoded;
pub mod ingress;
pub mod reorder;
#[cfg(target_os = "macos")]
pub mod worker;

#[derive(Debug, thiserror::Error)]
pub enum RecordingError {
    #[error("recording I/O failed: {0}")]
    Io(#[from] std::io::Error),
    #[error("recording manifest failed: {0}")]
    Manifest(#[from] serde_json::Error),
    #[error("invalid recording input: {0}")]
    InvalidInput(&'static str),
    #[error("native recording failed: {0}")]
    Platform(String),
    #[error("recording finalization timed out")]
    FinalizationTimeout,
}

/// Proof that a native adapter completed a segment. Only adapters in this crate
/// construct it; the bundle owns durable promotion and manifest publication.
#[derive(Debug)]
pub struct FinalizedSegment {
    pub(crate) path: std::path::PathBuf,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RecordingResult {
    pub path: Option<std::path::PathBuf>,
    pub state: bundle::RecordingState,
    pub error: Option<String>,
}
