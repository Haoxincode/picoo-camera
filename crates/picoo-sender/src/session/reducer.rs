//! Pure Sender connection lifecycle reducer.
//!
//! Pairing, encoder and ABR reducers retain their domain state, while
//! connection-generation gating and every destructive session boundary pass
//! through this reducer (REQ-PICOO-SESSION-012).

use picoo_session::{ConnectionState, SessionRuntimeState};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) struct SenderReducerState {
    pub(super) runtime: SessionRuntimeState,
    pub(super) active_generation: Option<u64>,
    pending_generation: Option<u64>,
    resources_active: bool,
    reconnect_enabled: bool,
    reconnect_pending: bool,
}

impl Default for SenderReducerState {
    fn default() -> Self {
        Self {
            runtime: SessionRuntimeState::default(),
            active_generation: None,
            pending_generation: None,
            resources_active: false,
            reconnect_enabled: true,
            reconnect_pending: false,
        }
    }
}

impl SenderReducerState {
    pub(super) fn active_session(self) -> Option<picoo_transport::SessionId> {
        self.active_generation.map(picoo_transport::SessionId)
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) enum SenderEvent {
    ConnectRequested,
    TransportConnectStarted {
        generation: u64,
    },
    ExplicitConnectFailed,
    TransportConnected {
        generation: u64,
    },
    ControlReceived {
        generation: u64,
    },
    TransportDisconnected {
        generation: u64,
        endpoint_available: bool,
    },
    AuthenticationRejected {
        generation: u64,
    },
    UserDisconnect {
        domain_resources_active: bool,
    },
    ReconnectPolicyChanged {
        enabled: bool,
    },
    ReconnectArmed {
        attempt: u32,
    },
    ReconnectDeadlineElapsed,
    ReconnectConnectFailed,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) enum SenderEffect {
    AcceptControl,
    ResetSessionResources,
    CloseTransport { generation: u64 },
    PrepareConnection { generation: u64 },
    ScheduleReconnect,
    StartReconnect,
    DisableReconnect,
    ClearConnectionIntent,
}

/// Sender lifecycle transitions emit at most three Effects. Keeping them
/// inline avoids allocating for every incoming control message.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) struct SenderEffects {
    items: [Option<SenderEffect>; 3],
}

impl SenderEffects {
    fn none() -> Self {
        Self { items: [None; 3] }
    }

    fn one(first: SenderEffect) -> Self {
        Self {
            items: [Some(first), None, None],
        }
    }

    fn two(first: SenderEffect, second: SenderEffect) -> Self {
        Self {
            items: [Some(first), Some(second), None],
        }
    }

    fn three(first: SenderEffect, second: SenderEffect, third: SenderEffect) -> Self {
        Self {
            items: [Some(first), Some(second), Some(third)],
        }
    }

    pub(super) fn contains(&self, expected: SenderEffect) -> bool {
        self.items.contains(&Some(expected))
    }

    pub(super) fn iter(&self) -> impl Iterator<Item = &SenderEffect> {
        self.items.iter().flatten()
    }

    #[cfg(test)]
    fn is_empty(&self) -> bool {
        self.items.iter().all(Option::is_none)
    }
}

pub(super) fn reduce(
    mut state: SenderReducerState,
    event: SenderEvent,
) -> (SenderReducerState, SenderEffects) {
    use SenderEffect as Effect;
    use SenderEvent as Event;

    let effects = match event {
        Event::ConnectRequested => {
            let previous_generation = state
                .active_generation
                .take()
                .or_else(|| state.pending_generation.take());
            state.resources_active = true;
            state.reconnect_enabled = true;
            state.reconnect_pending = false;
            state.runtime.reset_session(ConnectionState::Connecting);
            match previous_generation {
                Some(generation) => SenderEffects::two(
                    Effect::ResetSessionResources,
                    Effect::CloseTransport { generation },
                ),
                None => SenderEffects::none(),
            }
        }
        Event::TransportConnectStarted { generation }
            if state.runtime.connection() == ConnectionState::Connecting
                && state.active_generation.is_none() =>
        {
            state.pending_generation = Some(generation);
            SenderEffects::none()
        }
        Event::ExplicitConnectFailed
            if state.active_generation.is_none()
                && state.runtime.connection() == ConnectionState::Connecting =>
        {
            state.resources_active = false;
            state.reconnect_enabled = false;
            state.pending_generation = None;
            state.runtime = SessionRuntimeState::default();
            SenderEffects::one(Effect::ClearConnectionIntent)
        }
        Event::TransportConnected { generation }
            if state.runtime.connection() == ConnectionState::Connecting
                && state.pending_generation == Some(generation) =>
        {
            state.pending_generation = None;
            state.active_generation = Some(generation);
            state.resources_active = true;
            state.reconnect_pending = false;
            state
                .runtime
                .reset_session(ConnectionState::Connected { generation });
            SenderEffects::one(Effect::PrepareConnection { generation })
        }
        Event::ControlReceived { generation } if state.active_generation == Some(generation) => {
            SenderEffects::one(Effect::AcceptControl)
        }
        Event::TransportDisconnected {
            generation,
            endpoint_available,
        } if state.active_generation == Some(generation)
            || state.pending_generation == Some(generation) =>
        {
            state.active_generation = None;
            state.pending_generation = None;
            state.resources_active = false;
            state.runtime = SessionRuntimeState::default();
            if state.reconnect_enabled && endpoint_available {
                SenderEffects::two(Effect::ResetSessionResources, Effect::ScheduleReconnect)
            } else {
                SenderEffects::one(Effect::ResetSessionResources)
            }
        }
        Event::AuthenticationRejected { generation }
            if state.active_generation == Some(generation) =>
        {
            state.active_generation = None;
            state.pending_generation = None;
            state.resources_active = false;
            state.reconnect_enabled = false;
            state.reconnect_pending = false;
            state.runtime = SessionRuntimeState::default();
            SenderEffects::three(
                Effect::ResetSessionResources,
                Effect::CloseTransport { generation },
                Effect::DisableReconnect,
            )
        }
        Event::UserDisconnect {
            domain_resources_active,
        } if state.resources_active
            || state.active_generation.is_some()
            || state.reconnect_pending
            || domain_resources_active =>
        {
            let previous_generation = state
                .active_generation
                .take()
                .or_else(|| state.pending_generation.take());
            state.resources_active = false;
            state.reconnect_enabled = false;
            state.reconnect_pending = false;
            state.runtime = SessionRuntimeState::default();
            match previous_generation {
                Some(generation) => SenderEffects::three(
                    Effect::ResetSessionResources,
                    Effect::CloseTransport { generation },
                    Effect::ClearConnectionIntent,
                ),
                None => {
                    SenderEffects::two(Effect::ResetSessionResources, Effect::ClearConnectionIntent)
                }
            }
        }
        Event::ReconnectPolicyChanged { enabled } => {
            state.reconnect_enabled = enabled;
            if !enabled && state.reconnect_pending {
                state.reconnect_pending = false;
                state.runtime = SessionRuntimeState::default();
            }
            SenderEffects::none()
        }
        Event::ReconnectArmed { attempt }
            if state.reconnect_enabled && state.active_generation.is_none() =>
        {
            state.reconnect_pending = true;
            state
                .runtime
                .reset_session(ConnectionState::Reconnecting { attempt });
            SenderEffects::none()
        }
        Event::ReconnectDeadlineElapsed if state.reconnect_pending => {
            state.reconnect_pending = false;
            state.resources_active = true;
            state.runtime.reset_session(ConnectionState::Connecting);
            SenderEffects::one(Effect::StartReconnect)
        }
        Event::ReconnectConnectFailed
            if state.active_generation.is_none()
                && matches!(
                    state.runtime.connection(),
                    ConnectionState::Connecting | ConnectionState::Reconnecting { .. }
                ) =>
        {
            state.resources_active = false;
            state.pending_generation = None;
            state.runtime = SessionRuntimeState::default();
            if state.reconnect_enabled {
                SenderEffects::one(Effect::ScheduleReconnect)
            } else {
                SenderEffects::none()
            }
        }
        _ => SenderEffects::none(),
    };
    (state, effects)
}

#[cfg(test)]
mod tests {
    use super::*;
    use picoo_session::SenderStatus;

    #[test]
    fn stale_generation_cannot_route_control_or_disconnect_new_session() {
        let (state, _) = reduce(SenderReducerState::default(), SenderEvent::ConnectRequested);
        let (state, _) = reduce(
            state,
            SenderEvent::TransportConnectStarted { generation: 8 },
        );
        let (state, _) = reduce(state, SenderEvent::TransportConnected { generation: 8 });
        let before = state;
        let (state, control) = reduce(state, SenderEvent::ControlReceived { generation: 7 });
        assert_eq!(state, before);
        assert!(control.is_empty());
        let (state, disconnected) = reduce(
            state,
            SenderEvent::TransportDisconnected {
                generation: 7,
                endpoint_available: true,
            },
        );
        assert_eq!(state, before);
        assert!(disconnected.is_empty());
    }

    fn connected_state(generation: u64) -> SenderReducerState {
        let (state, _) = reduce(SenderReducerState::default(), SenderEvent::ConnectRequested);
        let (state, _) = reduce(state, SenderEvent::TransportConnectStarted { generation });
        let (state, _) = reduce(state, SenderEvent::TransportConnected { generation });
        state
    }

    #[test]
    fn late_connected_event_after_user_disconnect_is_ignored() {
        let (state, _) = reduce(SenderReducerState::default(), SenderEvent::ConnectRequested);
        let (state, _) = reduce(
            state,
            SenderEvent::TransportConnectStarted { generation: 8 },
        );
        let (state, _) = reduce(
            state,
            SenderEvent::UserDisconnect {
                domain_resources_active: false,
            },
        );
        let before = state;
        let (state, effects) = reduce(state, SenderEvent::TransportConnected { generation: 8 });
        assert_eq!(state, before);
        assert!(effects.is_empty());
    }

    #[test]
    fn peer_disconnect_resets_once_then_arms_reconnect() {
        let state = connected_state(3);
        let (state, effects) = reduce(
            state,
            SenderEvent::TransportDisconnected {
                generation: 3,
                endpoint_available: true,
            },
        );
        assert_eq!(
            effects,
            SenderEffects::two(
                SenderEffect::ResetSessionResources,
                SenderEffect::ScheduleReconnect,
            )
        );
        let (state, effects) = reduce(state, SenderEvent::ReconnectArmed { attempt: 1 });
        assert!(effects.is_empty());
        assert_eq!(state.runtime.sender_status(), SenderStatus::Reconnecting);
    }

    #[test]
    fn user_disconnect_is_idempotent_and_disables_recovery() {
        let state = connected_state(2);
        let (state, first) = reduce(
            state,
            SenderEvent::UserDisconnect {
                domain_resources_active: false,
            },
        );
        assert!(first.contains(SenderEffect::ResetSessionResources));
        let (same, repeated) = reduce(
            state,
            SenderEvent::UserDisconnect {
                domain_resources_active: false,
            },
        );
        assert_eq!(same, state);
        assert!(repeated.is_empty());
        let (_, stale) = reduce(
            state,
            SenderEvent::TransportDisconnected {
                generation: 2,
                endpoint_available: true,
            },
        );
        assert!(stale.is_empty());
    }

    #[test]
    fn reconnect_failure_rearms_instead_of_abandoning_endpoint() {
        let state = connected_state(1);
        let (state, _) = reduce(
            state,
            SenderEvent::TransportDisconnected {
                generation: 1,
                endpoint_available: true,
            },
        );
        let (state, _) = reduce(state, SenderEvent::ReconnectArmed { attempt: 1 });
        let (state, start) = reduce(state, SenderEvent::ReconnectDeadlineElapsed);
        assert_eq!(start, SenderEffects::one(SenderEffect::StartReconnect));
        let (_, failed) = reduce(state, SenderEvent::ReconnectConnectFailed);
        assert_eq!(failed, SenderEffects::one(SenderEffect::ScheduleReconnect));
    }
}
