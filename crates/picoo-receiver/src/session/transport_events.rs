//! Fair, age-bounded transport event consumption for the Receiver owner.

use std::time::{Duration, Instant};

use picoo_transport::TransportEvent;

use super::reducer::{ReceiverEffect, ReceiverEvent};
use super::ReceiverSession;
use crate::ReceiverError;

impl ReceiverSession {
    pub fn pump(&mut self) -> Result<(), ReceiverError> {
        const MAX_TRANSPORT_EVENTS_PER_PUMP: usize = 64;
        const TRANSPORT_BUDGET: Duration = Duration::from_millis(2);

        self.drain_decoder_events()?;
        self.expire_pending_pairing_if_needed();
        self.expire_reassembly_deadline()?;

        let transport_started = Instant::now();
        let mut transport_events = 0_usize;
        while transport_events < MAX_TRANSPORT_EVENTS_PER_PUMP
            && transport_started.elapsed() < TRANSPORT_BUDGET
        {
            let Some(event) = self.transport.poll_event() else {
                break;
            };
            transport_events += 1;
            match event {
                TransportEvent::Connected(session)
                    if self.transport.active_session() == Some(session) =>
                {
                    self.apply_receiver_event(ReceiverEvent::TransportConnected {
                        generation: session.0,
                    })?;
                }
                TransportEvent::Disconnected(session, _)
                    if self.transport.active_session().is_none() =>
                {
                    let retain_frame = self.lifecycle.runtime.stream().is_streaming()
                        && self.latest_frame_store.latest().is_some()
                        && !self.last_frame_hold.is_zero();
                    self.apply_receiver_event(ReceiverEvent::TransportDisconnected {
                        generation: session.0,
                        retain_frame,
                    })?;
                }
                TransportEvent::ControlMessage(session, msg) => {
                    let effects = self.apply_receiver_event(ReceiverEvent::ControlReceived {
                        generation: session.0,
                    })?;
                    if effects.contains(ReceiverEffect::AcceptControl) {
                        if let Err(error) = self.handle_control(session, msg) {
                            self.reject_control_session(session);
                            return Err(error);
                        }
                    }
                }
                TransportEvent::VideoPackets(session, packets) => {
                    let effects = self.apply_receiver_event(ReceiverEvent::VideoReceived {
                        generation: session.0,
                    })?;
                    if effects.contains(ReceiverEffect::AcceptVideo) {
                        let received_at = packets.received_at();
                        let queue_age = Instant::now().saturating_duration_since(received_at);
                        self.stats_reporter.record_receive_queue_age(queue_age);
                        if queue_age >= self.media_deadline() {
                            self.discard_stale_video_batch(packets)?;
                        } else {
                            for packet in packets {
                                self.ingest_video_packet(packet, received_at)?;
                            }
                        }
                        // The transport queue can remain continuously readable on
                        // a 1080p stream. Give every bounded ingress batch a
                        // playout opportunity before polling more media.
                        self.drain_jitter()?;
                    }
                }
                _ => {
                    // An event queued by an older connection generation must not
                    // mutate the currently active Receiver session.
                }
            }
        }
        if transport_events == MAX_TRANSPORT_EVENTS_PER_PUMP
            || transport_started.elapsed() >= TRANSPORT_BUDGET
        {
            // Preserve fairness without waiting for a new network event when
            // the current queue still has work after this bounded turn.
            self.runtime_wake.signal();
        }

        // QUIC Datagram may reorder fragments across access units. A newer AU
        // is therefore not proof that an older partial AU was lost; only the
        // bounded real-time deadline makes that decision.
        self.expire_reassembly_deadline()?;

        self.drain_jitter()?;
        self.drain_decoder_events()?;
        self.maybe_request_recovery_keyframe()?;
        self.maybe_finalize_disconnect_hold()?;
        self.maybe_send_receiver_stats()?;
        self.maybe_send_clock_sync()?;

        Ok(())
    }
}
