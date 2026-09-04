//! Lossy video datagram wrapper — PRD §21 / REQ-PICOO-SESSION-006.
//!
//! Drops a configurable fraction of outbound video packets while forwarding
//! control messages reliably. Used to prove the session stays usable under ~5% loss.

use bytes::Bytes;
use picoo_protocol::VideoPacket;
use picoo_transport::{
    ChannelBinding, CloseReason, Endpoint, PicooTransport, SessionId, TransportError,
    TransportEvent, TransportLinkStats,
};

/// Deterministic LCG so loss patterns are reproducible in CI.
fn next_u64(state: &mut u64) -> u64 {
    // Numerical Recipes LCG
    *state = state.wrapping_mul(6364136223846793005).wrapping_add(1);
    *state
}

pub struct LossyVideoTransport<T: PicooTransport> {
    inner: T,
    /// Probability in [0, 1] of dropping each video packet.
    drop_ratio: f64,
    rng: u64,
    pub attempted_video: u64,
    pub dropped_video: u64,
}

impl<T: PicooTransport> LossyVideoTransport<T> {
    pub fn new(inner: T, drop_ratio: f64) -> Self {
        Self::with_seed(inner, drop_ratio, 0xC0FFEE_u64)
    }

    pub fn with_seed(inner: T, drop_ratio: f64, seed: u64) -> Self {
        Self {
            inner,
            drop_ratio: drop_ratio.clamp(0.0, 1.0),
            rng: seed,
            attempted_video: 0,
            dropped_video: 0,
        }
    }

    pub fn into_inner(self) -> T {
        self.inner
    }

    pub fn observed_drop_ratio(&self) -> f64 {
        if self.attempted_video == 0 {
            0.0
        } else {
            self.dropped_video as f64 / self.attempted_video as f64
        }
    }

    /// Disable or retune loss for a post-recovery phase (PRD §21 delay gate).
    pub fn set_drop_ratio(&mut self, drop_ratio: f64) {
        self.drop_ratio = drop_ratio.clamp(0.0, 1.0);
    }

    fn should_drop(&mut self) -> bool {
        if self.drop_ratio <= 0.0 {
            return false;
        }
        if self.drop_ratio >= 1.0 {
            return true;
        }
        let sample = (next_u64(&mut self.rng) >> 11) as f64 / ((1u64 << 53) as f64);
        sample < self.drop_ratio
    }
}

impl<T: PicooTransport> PicooTransport for LossyVideoTransport<T> {
    fn connect(&mut self, endpoint: Endpoint) -> Result<SessionId, TransportError> {
        self.inner.connect(endpoint)
    }

    fn send_control(&mut self, session: SessionId, message: Bytes) -> Result<(), TransportError> {
        self.inner.send_control(session, message)
    }

    fn send_video(
        &mut self,
        session: SessionId,
        packet: VideoPacket,
    ) -> Result<(), TransportError> {
        self.attempted_video += 1;
        if self.should_drop() {
            self.dropped_video += 1;
            return Ok(());
        }
        self.inner.send_video(session, packet)
    }

    fn poll_event(&mut self) -> Option<TransportEvent> {
        self.inner.poll_event()
    }

    fn close(&mut self, session: SessionId, reason: CloseReason) {
        self.inner.close(session, reason)
    }

    fn channel_binding(&self, session: SessionId) -> Result<ChannelBinding, TransportError> {
        self.inner.channel_binding(session)
    }

    fn link_stats(&self) -> Option<TransportLinkStats> {
        self.inner.link_stats()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::MemoryTransport;
    use picoo_protocol::VideoPacketFlags;

    #[test]
    fn drops_approximately_five_percent() {
        let mut transport = LossyVideoTransport::new(MemoryTransport::new(), 0.05);
        let session = transport
            .connect(Endpoint {
                host: "127.0.0.1".into(),
                port: 1,
            })
            .expect("connect");
        for i in 0..2_000 {
            let packet = VideoPacket {
                stream_epoch: 1,
                frame_id: i,
                fragment_index: 0,
                fragment_count: 1,
                pts_us: i,
                flags: VideoPacketFlags::empty(),
                payload: Bytes::from_static(b"x"),
            };
            transport.send_video(session, packet).expect("send");
        }
        let ratio = transport.observed_drop_ratio();
        assert!(
            (0.03..0.08).contains(&ratio),
            "expected ~5% drops, got {ratio}"
        );
    }
}
