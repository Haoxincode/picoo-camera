//! Receiver lifecycle Effect adapter and single idempotent teardown boundary.

use std::time::Instant;

use picoo_packet::ReassemblyMap;
use picoo_protocol::MAX_VIDEO_FRAGMENTS_PER_ACCESS_UNIT;
use picoo_transport::CloseReason;

use super::reducer::{reduce, ReceiverCloseReason, ReceiverEffect, ReceiverEffects, ReceiverEvent};
use super::{ReceiverSession, StatsReporter};
use crate::ReceiverError;

impl ReceiverSession {
    fn reset_session_resources(&mut self) {
        self.decoder_worker.reset();
        self.frame_buffer_pool.clear();
        self.active_sender = None;
        self.pending_pairing = None;
        self.reassembly = ReassemblyMap::new(8, MAX_VIDEO_FRAGMENTS_PER_ACCESS_UNIT);
        self.stats_reporter = StatsReporter::new();
        self.jitter.clear();
        self.interarrival_jitter.reset();
        self.reset_network_health();
        self.last_stats = None;
        self.last_sender_stats = None;
        self.last_decoded_fps = 0;
        self.last_media_error = None;
        self.current_stream_config = None;
        self.waiting_for_stream_config_epoch = None;
        self.pending_stream_config_idr = None;
        self.receiver_capabilities_sent = None;
        self.decoder_recovery.reset_session();
        self.control_generation = None;
        self.next_control_message_id = 1;
        self.last_received_control_message_id = 0;
    }

    pub(super) fn apply_receiver_event(
        &mut self,
        event: ReceiverEvent,
    ) -> Result<ReceiverEffects, ReceiverError> {
        let (state, effects) = reduce(self.lifecycle, event);
        self.lifecycle = state;
        for effect in effects.iter() {
            match effect {
                ReceiverEffect::PrepareConnection => {
                    self.placeholder_after = None;
                    self.control_generation = None;
                    self.next_control_message_id = 1;
                    self.last_received_control_message_id = 0;
                }
                ReceiverEffect::ResetSessionResources => self.reset_session_resources(),
                ReceiverEffect::CloseActiveTransport(reason) => {
                    let reason = match reason {
                        ReceiverCloseReason::Local => CloseReason::LocalClose,
                        ReceiverCloseReason::InvalidControl => {
                            CloseReason::Error("invalid PCP control message".into())
                        }
                        ReceiverCloseReason::PairingExpired => {
                            CloseReason::Error("pairing challenge expired".into())
                        }
                        ReceiverCloseReason::PairingRejected
                        | ReceiverCloseReason::PublicKeyChanged => CloseReason::LocalClose,
                    };
                    self.transport.close_active(reason);
                }
                ReceiverEffect::ScheduleDisconnectHold => {
                    self.placeholder_after = Some(Instant::now() + self.last_frame_hold);
                }
                ReceiverEffect::PublishWaitingPlaceholder => {
                    self.placeholder_after = None;
                    self.publish_waiting_placeholder()?;
                }
                ReceiverEffect::PublishReconnectingPlaceholder => {
                    self.placeholder_after = None;
                    self.publish_reconnecting_placeholder()?;
                }
                ReceiverEffect::AcceptControl | ReceiverEffect::AcceptVideo => {}
            }
        }
        Ok(effects)
    }

    pub(super) fn maybe_finalize_disconnect_hold(&mut self) -> Result<(), ReceiverError> {
        let Some(deadline) = self.placeholder_after else {
            return Ok(());
        };
        if Instant::now() < deadline {
            return Ok(());
        }
        self.apply_receiver_event(ReceiverEvent::DisconnectHoldElapsed)?;
        Ok(())
    }
}
