//! Recording observation across the Receiver/UI boundary — MEDIA-075/076.
use picoo_receiver::ReceiverSession;
use picoo_recording::{bundle::RecordingState, RecordingResult};

#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct RecordingSnapshot {
    pub available: bool,
    pub state: Option<RecordingState>,
    pub stopping: bool,
    pub stalled: bool,
    pub result: Option<RecordingResult>,
}

#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct RecordingSnapshots {
    pub encoded: RecordingSnapshot,
    pub rendered: RecordingSnapshot,
    pub rendered_source_supported: bool,
}

impl RecordingSnapshots {
    pub(super) fn capture(receiver: &ReceiverSession) -> Self {
        #[cfg(any(target_os = "macos", windows))]
        let encoded = {
            // Read the immutable final result first. If publication occurs
            // during this capture, show finalizing until the next snapshot;
            // never infer successful persistence from a state atomic alone.
            let result = receiver.encoded_recording_result();
            let state = result
                .as_ref()
                .map(|result| result.state)
                .or_else(|| receiver.encoded_recording_state());
            let stopping = result.is_none()
                && (receiver.encoded_recording_stopping()
                    || matches!(
                        state,
                        Some(
                            RecordingState::Complete
                                | RecordingState::HasGaps
                                | RecordingState::Failed
                        )
                    ));
            RecordingSnapshot {
                available: true,
                state,
                stopping,
                stalled: result.is_none() && receiver.encoded_recording_stalled(),
                result,
            }
        };
        #[cfg(not(any(target_os = "macos", windows)))]
        let encoded = {
            let _ = receiver;
            RecordingSnapshot::default()
        };

        #[cfg(any(target_os = "macos", windows))]
        let rendered = {
            let result = receiver.rendered_recording_result();
            let state = result
                .as_ref()
                .map(|result| result.state)
                .or_else(|| receiver.rendered_recording_state());
            let stopping = result.is_none()
                && (receiver.rendered_recording_stopping()
                    || matches!(
                        state,
                        Some(
                            RecordingState::Complete
                                | RecordingState::HasGaps
                                | RecordingState::Failed
                        )
                    ));
            RecordingSnapshot {
                available: true,
                state,
                stopping,
                stalled: result.is_none() && receiver.rendered_recording_stalled(),
                result,
            }
        };
        #[cfg(not(any(target_os = "macos", windows)))]
        let rendered = RecordingSnapshot::default();

        #[cfg(any(target_os = "macos", windows))]
        let rendered_source_supported = receiver.rendered_recording_source_supported();
        #[cfg(not(any(target_os = "macos", windows)))]
        let rendered_source_supported = false;

        Self {
            encoded,
            rendered,
            rendered_source_supported,
        }
    }
}
