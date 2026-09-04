//! Pure Receiver lifecycle reducer and adapter effects.
//!
//! Detailed pairing/media reducers keep their own domain data, while every
//! connection-generation gate and destructive session boundary passes through
//! this state machine (REQ-PICOO-SESSION-012).

use picoo_session::{ConnectionState, OutputState, SessionRuntimeState};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) struct ReceiverReducerState {
    pub(super) runtime: SessionRuntimeState,
    pub(super) active_generation: Option<u64>,
    pub(super) resources_active: bool,
    listening: bool,
    disconnect_hold_pending: bool,
}

impl Default for ReceiverReducerState {
    fn default() -> Self {
        Self {
            runtime: SessionRuntimeState::default(),
            active_generation: None,
            // Decoder, timing and frame adapters exist immediately. The first
            // teardown must reset them even before a transport connects;
            // subsequent teardowns become effect-free until reactivated.
            resources_active: true,
            listening: false,
            disconnect_hold_pending: false,
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) enum ReceiverEvent {
    ListenerStarted,
    TransportConnected {
        generation: u64,
    },
    TransportDisconnected {
        generation: u64,
        retain_frame: bool,
    },
    ControlReceived {
        generation: u64,
    },
    VideoReceived {
        generation: u64,
    },
    StopStream {
        generation: u64,
    },
    AbortConnection {
        generation: u64,
        reason: ReceiverCloseReason,
    },
    UserClose,
    DisconnectHoldElapsed,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) enum ReceiverEffect {
    PrepareConnection,
    AcceptControl,
    AcceptVideo,
    ResetSessionResources,
    CloseActiveTransport(ReceiverCloseReason),
    ScheduleDisconnectHold,
    PublishWaitingPlaceholder,
    PublishReconnectingPlaceholder,
}

/// Lifecycle transitions have at most three Effects. A fixed inline set keeps
/// control/video routing allocation-free on the session pump hot path.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) struct ReceiverEffects {
    items: [Option<ReceiverEffect>; 3],
}

impl ReceiverEffects {
    fn none() -> Self {
        Self { items: [None; 3] }
    }

    fn one(first: ReceiverEffect) -> Self {
        Self {
            items: [Some(first), None, None],
        }
    }

    fn two(first: ReceiverEffect, second: ReceiverEffect) -> Self {
        Self {
            items: [Some(first), Some(second), None],
        }
    }

    fn three(first: ReceiverEffect, second: ReceiverEffect, third: ReceiverEffect) -> Self {
        Self {
            items: [Some(first), Some(second), Some(third)],
        }
    }

    pub(super) fn contains(&self, expected: ReceiverEffect) -> bool {
        self.items.contains(&Some(expected))
    }

    pub(super) fn iter(&self) -> impl Iterator<Item = &ReceiverEffect> {
        self.items.iter().flatten()
    }

    #[cfg(test)]
    fn is_empty(&self) -> bool {
        self.items.iter().all(Option::is_none)
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) enum ReceiverCloseReason {
    Local,
    InvalidControl,
    PairingExpired,
    PairingRejected,
    PublicKeyChanged,
}

pub(super) fn reduce(
    mut state: ReceiverReducerState,
    event: ReceiverEvent,
) -> (ReceiverReducerState, ReceiverEffects) {
    use ReceiverEffect as Effect;
    use ReceiverEvent as Event;

    let effects = match event {
        Event::ListenerStarted => {
            state.listening = true;
            state.runtime.set_output(OutputState::Ready);
            if state.active_generation.is_none() {
                state.runtime.set_connection(ConnectionState::Listening);
            }
            ReceiverEffects::none()
        }
        Event::TransportConnected { generation } => {
            state.active_generation = Some(generation);
            state.resources_active = true;
            state.disconnect_hold_pending = false;
            state
                .runtime
                .reset_session(ConnectionState::Connected { generation });
            ReceiverEffects::one(Effect::PrepareConnection)
        }
        Event::ControlReceived { generation } if state.active_generation == Some(generation) => {
            ReceiverEffects::one(Effect::AcceptControl)
        }
        Event::VideoReceived { generation } if state.active_generation == Some(generation) => {
            ReceiverEffects::one(Effect::AcceptVideo)
        }
        Event::TransportDisconnected {
            generation,
            retain_frame,
        } if state.active_generation == Some(generation) => {
            state.active_generation = None;
            state.resources_active = false;
            if retain_frame {
                state.disconnect_hold_pending = true;
                state
                    .runtime
                    .reset_session(ConnectionState::Reconnecting { attempt: 1 });
                ReceiverEffects::two(
                    Effect::ResetSessionResources,
                    Effect::ScheduleDisconnectHold,
                )
            } else {
                state.disconnect_hold_pending = false;
                reset_to_available_idle(&mut state);
                ReceiverEffects::two(
                    Effect::ResetSessionResources,
                    Effect::PublishWaitingPlaceholder,
                )
            }
        }
        Event::StopStream { generation } if state.active_generation == Some(generation) => {
            state.active_generation = None;
            state.resources_active = false;
            state.disconnect_hold_pending = false;
            reset_to_available_idle(&mut state);
            ReceiverEffects::three(
                Effect::ResetSessionResources,
                Effect::CloseActiveTransport(ReceiverCloseReason::Local),
                Effect::PublishWaitingPlaceholder,
            )
        }
        Event::AbortConnection { generation, reason }
            if state.active_generation == Some(generation) =>
        {
            state.active_generation = None;
            state.resources_active = false;
            state.disconnect_hold_pending = false;
            reset_to_available_idle(&mut state);
            ReceiverEffects::three(
                Effect::ResetSessionResources,
                Effect::CloseActiveTransport(reason),
                Effect::PublishWaitingPlaceholder,
            )
        }
        Event::UserClose
            if state.resources_active
                || state.active_generation.is_some()
                || state.disconnect_hold_pending =>
        {
            state.active_generation = None;
            state.resources_active = false;
            state.listening = false;
            state.disconnect_hold_pending = false;
            state.runtime = SessionRuntimeState::default();
            ReceiverEffects::three(
                Effect::ResetSessionResources,
                Effect::CloseActiveTransport(ReceiverCloseReason::Local),
                Effect::PublishWaitingPlaceholder,
            )
        }
        Event::DisconnectHoldElapsed if state.disconnect_hold_pending => {
            state.disconnect_hold_pending = false;
            reset_to_available_idle(&mut state);
            ReceiverEffects::one(Effect::PublishReconnectingPlaceholder)
        }
        _ => ReceiverEffects::none(),
    };
    (state, effects)
}

fn reset_to_available_idle(state: &mut ReceiverReducerState) {
    let connection = if state.listening {
        ConnectionState::Listening
    } else {
        ConnectionState::Idle
    };
    state.runtime.reset_session(connection);
}

#[cfg(test)]
mod tests {
    use super::*;
    use picoo_session::{ReceiverStatus, StreamState};

    #[test]
    fn stale_generation_events_have_no_state_or_effects() {
        let (state, _) = reduce(
            ReceiverReducerState::default(),
            ReceiverEvent::TransportConnected { generation: 7 },
        );
        let before = state;
        let (state, effects) = reduce(
            state,
            ReceiverEvent::TransportDisconnected {
                generation: 6,
                retain_frame: false,
            },
        );
        assert_eq!(state, before);
        assert!(effects.is_empty());
    }

    #[test]
    fn disconnect_hold_keeps_reconnecting_until_timer_effect() {
        let (state, _) = reduce(
            ReceiverReducerState::default(),
            ReceiverEvent::ListenerStarted,
        );
        let (mut state, _) = reduce(state, ReceiverEvent::TransportConnected { generation: 2 });
        state
            .runtime
            .set_stream(StreamState::Streaming { generation: 9 });
        let (state, effects) = reduce(
            state,
            ReceiverEvent::TransportDisconnected {
                generation: 2,
                retain_frame: true,
            },
        );
        assert_eq!(
            state.runtime.receiver_status(),
            ReceiverStatus::Reconnecting
        );
        assert_eq!(
            effects,
            ReceiverEffects::two(
                ReceiverEffect::ResetSessionResources,
                ReceiverEffect::ScheduleDisconnectHold,
            )
        );
        let (state, effects) = reduce(state, ReceiverEvent::DisconnectHoldElapsed);
        assert_eq!(state.runtime.receiver_status(), ReceiverStatus::Discovering);
        assert_eq!(
            effects,
            ReceiverEffects::one(ReceiverEffect::PublishReconnectingPlaceholder)
        );
    }

    #[test]
    fn repeated_user_teardown_is_effect_free() {
        let (state, _) = reduce(
            ReceiverReducerState::default(),
            ReceiverEvent::TransportConnected { generation: 1 },
        );
        let (state, first) = reduce(state, ReceiverEvent::UserClose);
        assert!(first.contains(ReceiverEffect::ResetSessionResources));
        let (same, repeated) = reduce(state, ReceiverEvent::UserClose);
        assert_eq!(same, state);
        assert!(repeated.is_empty());
    }

    #[test]
    fn control_and_video_are_effects_only_for_active_generation() {
        let (state, _) = reduce(
            ReceiverReducerState::default(),
            ReceiverEvent::TransportConnected { generation: 4 },
        );
        let (_, control) = reduce(state, ReceiverEvent::ControlReceived { generation: 4 });
        let (_, video) = reduce(state, ReceiverEvent::VideoReceived { generation: 4 });
        let (_, stale) = reduce(state, ReceiverEvent::VideoReceived { generation: 3 });
        assert_eq!(control, ReceiverEffects::one(ReceiverEffect::AcceptControl));
        assert_eq!(video, ReceiverEffects::one(ReceiverEffect::AcceptVideo));
        assert!(stale.is_empty());
    }
}
