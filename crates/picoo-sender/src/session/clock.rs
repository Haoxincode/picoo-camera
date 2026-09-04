//! Sender side of PCP monotonic clock synchronization.

use picoo_protocol::control::{
    control_envelope::Payload as ControlPayload, ClockSyncPing, ClockSyncPong,
};
use picoo_transport::{PicooTransport, SessionId};

use super::SenderSession;

impl<T: PicooTransport> SenderSession<T> {
    pub(super) fn handle_clock_sync_ping(&mut self, session: SessionId, ping: ClockSyncPing) {
        if ping.sample_id == 0 || ping.stream_epoch != self.current_stream_epoch {
            return;
        }
        let Some(sender_receive_us) = self.media_clock_now_us(ping.stream_epoch) else {
            return;
        };
        let sender_send_us = self
            .media_clock_now_us(ping.stream_epoch)
            .unwrap_or(sender_receive_us)
            .max(sender_receive_us);
        let pong = ClockSyncPong {
            sample_id: ping.sample_id,
            receiver_send_us: ping.receiver_send_us,
            sender_receive_us,
            sender_send_us,
            stream_epoch: ping.stream_epoch,
        };
        let _ = self.send_control_payload(session, ControlPayload::ClockSyncPong(pong));
    }
}
