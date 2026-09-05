//! Pure Receiver media scheduling decisions — REQ-PICOO-SESSION-016.
//!
//! Reassembly, jitter buffering, and decoder admission keep their specialized
//! data structures. This module owns the one decision that coordinates them:
//! whether the oldest complete AU may advance, must expire, or should wait.

use std::time::Duration;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum MediaWaitReason {
    OlderAccessUnit,
    PlayoutTarget,
    HardExpiration,
    MediaEvent,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum MediaScheduleDecision {
    DecodeReadyFrame,
    DiscardExpired,
    WaitUntil {
        delay: Duration,
        reason: MediaWaitReason,
    },
    WaitForEvent(MediaWaitReason),
    Idle,
}

impl MediaScheduleDecision {
    /// Timer contribution for the Receiver owner loop.
    pub fn wake_delay(self) -> Option<Duration> {
        match self {
            Self::DecodeReadyFrame | Self::DiscardExpired => Some(Duration::ZERO),
            Self::WaitUntil { delay, .. } => Some(delay),
            Self::WaitForEvent(_) | Self::Idle => None,
        }
    }
}

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct MediaScheduleInput {
    pub front_frame_id: Option<u64>,
    pub oldest_unresolved_frame_id: Option<u64>,
    pub release_delay: Option<Duration>,
    pub expiration_delay: Option<Duration>,
}

/// Coordinate complete-AU playout with older incomplete reassembly.
///
/// Delays are Receiver-local and already relative to the same `now`. The hard
/// expiration wins over normal playout. While an older AU is unresolved, its
/// outcome (or the complete AU's own hard age bound) is the only valid timer;
/// an already-reached playout target must not cause a zero-delay retry loop.
pub fn schedule_media(input: MediaScheduleInput) -> MediaScheduleDecision {
    let Some(candidate_frame_id) = input.front_frame_id else {
        return MediaScheduleDecision::Idle;
    };

    if input.expiration_delay == Some(Duration::ZERO) {
        return MediaScheduleDecision::DiscardExpired;
    }

    let blocked_by_older = input
        .oldest_unresolved_frame_id
        .is_some_and(|unresolved_frame_id| unresolved_frame_id < candidate_frame_id);
    if blocked_by_older {
        return input.expiration_delay.map_or(
            MediaScheduleDecision::WaitForEvent(MediaWaitReason::OlderAccessUnit),
            |delay| MediaScheduleDecision::WaitUntil {
                delay,
                reason: MediaWaitReason::OlderAccessUnit,
            },
        );
    }

    match (input.release_delay, input.expiration_delay) {
        (Some(release), Some(expiration)) if expiration < release => {
            MediaScheduleDecision::WaitUntil {
                delay: expiration,
                reason: MediaWaitReason::HardExpiration,
            }
        }
        (Some(delay), _) if delay.is_zero() => MediaScheduleDecision::DecodeReadyFrame,
        (Some(delay), _) => MediaScheduleDecision::WaitUntil {
            delay,
            reason: MediaWaitReason::PlayoutTarget,
        },
        (None, Some(delay)) => MediaScheduleDecision::WaitUntil {
            delay,
            reason: MediaWaitReason::HardExpiration,
        },
        (None, None) => MediaScheduleDecision::WaitForEvent(MediaWaitReason::MediaEvent),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn older_reassembly_blocks_an_already_ready_newer_access_unit_without_spinning() {
        assert_eq!(
            schedule_media(MediaScheduleInput {
                front_frame_id: Some(200),
                oldest_unresolved_frame_id: Some(100),
                release_delay: Some(Duration::ZERO),
                expiration_delay: Some(Duration::from_millis(170)),
            }),
            MediaScheduleDecision::WaitUntil {
                delay: Duration::from_millis(170),
                reason: MediaWaitReason::OlderAccessUnit,
            }
        );
    }

    #[test]
    fn fixed_timing_waits_for_an_event_when_older_reassembly_has_no_expiry_timer() {
        assert_eq!(
            schedule_media(MediaScheduleInput {
                front_frame_id: Some(200),
                oldest_unresolved_frame_id: Some(100),
                release_delay: Some(Duration::ZERO),
                expiration_delay: None,
            }),
            MediaScheduleDecision::WaitForEvent(MediaWaitReason::OlderAccessUnit)
        );
    }

    #[test]
    fn hard_deadline_is_handled_before_playout() {
        assert_eq!(
            schedule_media(MediaScheduleInput {
                front_frame_id: Some(9),
                oldest_unresolved_frame_id: None,
                release_delay: Some(Duration::ZERO),
                expiration_delay: Some(Duration::ZERO),
            }),
            MediaScheduleDecision::DiscardExpired
        );
    }

    #[test]
    fn ready_unblocked_access_unit_advances_once() {
        assert_eq!(
            schedule_media(MediaScheduleInput {
                front_frame_id: Some(9),
                oldest_unresolved_frame_id: Some(9),
                release_delay: Some(Duration::ZERO),
                expiration_delay: Some(Duration::from_millis(200)),
            }),
            MediaScheduleDecision::DecodeReadyFrame
        );
    }

    #[test]
    fn hard_expiration_remains_a_deadline_without_a_playout_timer() {
        assert_eq!(
            schedule_media(MediaScheduleInput {
                front_frame_id: Some(9),
                oldest_unresolved_frame_id: None,
                release_delay: None,
                expiration_delay: Some(Duration::from_millis(125)),
            }),
            MediaScheduleDecision::WaitUntil {
                delay: Duration::from_millis(125),
                reason: MediaWaitReason::HardExpiration,
            }
        );
    }
}
