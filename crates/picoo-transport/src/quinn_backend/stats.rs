use quinn::Connection;

use super::{SharedState, DATAGRAM_SEND_BUFFER_SIZE};
use crate::TransportLinkStats;

pub(super) fn should_enqueue_access_unit(available: usize, required: usize) -> bool {
    available >= required
}

pub(super) fn link_stats(connection: &Connection, shared: &SharedState) -> TransportLinkStats {
    let stats = connection.stats();
    TransportLinkStats {
        rtt_ms: stats.path.rtt.as_secs_f64() * 1_000.0,
        lost_packets: stats.path.lost_packets,
        sent_packets: stats.path.sent_packets,
        recv_packets: stats.udp_rx.datagrams,
        dgram_recv: stats.frame_rx.datagram,
        video_queue_age_ms: shared.video_queue_age_ms,
        video_dropped_access_units: shared.video_dropped_access_units,
        video_buffered_bytes: DATAGRAM_SEND_BUFFER_SIZE
            .saturating_sub(connection.datagram_send_buffer_space())
            as u64,
    }
}
