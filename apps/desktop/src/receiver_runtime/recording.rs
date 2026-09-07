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

impl RecordingSnapshot {
    pub(super) fn capture(receiver: &ReceiverSession) -> Self {
        #[cfg(target_os = "macos")]
        {
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
            Self {
                available: true,
                state,
                stopping,
                stalled: result.is_none() && receiver.encoded_recording_stalled(),
                result,
            }
        }
        #[cfg(not(target_os = "macos"))]
        {
            let _ = receiver;
            Self::default()
        }
    }
}
