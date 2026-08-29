//! Deterministic keyframe-tail dropper — REQ-PICOO-SESSION-003.
//!
//! Drops KEYFRAME video fragments with `fragment_index > 0` so reassembly sees an
//! incomplete IDR, sets `take_keyframe_loss`, and the receiver requests a new IDR.

use bytes::Bytes;
use picoo_protocol::{VideoPacket, VideoPacketFlags};
use picoo_transport::{
    CloseReason, Endpoint, PicooTransport, SessionId, TransportError, TransportEvent,
    TransportLinkStats,
};

/// When armed, drops non-zero-index fragments of keyframe access units.
pub struct DropKeyframeTailTransport<T: PicooTransport> {
    inner: T,
    armed: bool,
    pub dropped_tail_fragments: u64,
    pub forwarded_video: u64,
}

impl<T: PicooTransport> DropKeyframeTailTransport<T> {
    pub fn new(inner: T) -> Self {
        Self {
            inner,
            armed: false,
            dropped_tail_fragments: 0,
            forwarded_video: 0,
        }
    }

    pub fn arm(&mut self) {
        self.armed = true;
    }

    pub fn disarm(&mut self) {
        self.armed = false;
    }

    pub fn is_armed(&self) -> bool {
        self.armed
    }

    pub fn into_inner(self) -> T {
        self.inner
    }
}

impl<T: PicooTransport> PicooTransport for DropKeyframeTailTransport<T> {
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
        let is_key_tail = packet.flags.contains(VideoPacketFlags::KEYFRAME)
            && packet.fragment_index > 0
            && packet.fragment_count > 1;
        if self.armed && is_key_tail {
            self.dropped_tail_fragments += 1;
            return Ok(());
        }
        self.forwarded_video += 1;
        self.inner.send_video(session, packet)
    }

    fn poll_event(&mut self) -> Option<TransportEvent> {
        self.inner.poll_event()
    }

    fn close(&mut self, session: SessionId, reason: CloseReason) {
        self.inner.close(session, reason)
    }

    fn link_stats(&self) -> Option<TransportLinkStats> {
        self.inner.link_stats()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::MemoryTransport;

    #[test]
    fn drops_only_keyframe_tail_when_armed() {
        let mut t = DropKeyframeTailTransport::new(MemoryTransport::new());
        t.arm();
        let session = t
            .connect(Endpoint {
                host: "127.0.0.1".into(),
                port: 9,
            })
            .expect("connect");

        let head = VideoPacket {
            version: VideoPacket::VERSION,
            flags: VideoPacketFlags::KEYFRAME | VideoPacketFlags::START_OF_ACCESS_UNIT,
            stream_epoch: 1,
            frame_id: 1,
            pts_us: 0,
            fragment_index: 0,
            fragment_count: 2,
            payload: Bytes::from_static(b"k0"),
        };
        let tail = VideoPacket {
            version: VideoPacket::VERSION,
            flags: VideoPacketFlags::KEYFRAME | VideoPacketFlags::END_OF_ACCESS_UNIT,
            stream_epoch: 1,
            frame_id: 1,
            pts_us: 0,
            fragment_index: 1,
            fragment_count: 2,
            payload: Bytes::from_static(b"k1"),
        };
        t.send_video(session, head).unwrap();
        t.send_video(session, tail).unwrap();
        assert_eq!(t.dropped_tail_fragments, 1);
        assert_eq!(t.forwarded_video, 1);
    }
}
