//! Android/iOS sender pipeline: H.264 access unit → PCP/1 VideoPacket fragmentation.
//!
//! REQ-PICOO-MEDIA-001, REQ-PICOO-STACK-001

mod session;

use bytes::Bytes;
use picoo_protocol::{
    VideoPacket, VideoPacketError, VideoPacketFlags, MAX_DATAGRAM_SIZE, VIDEO_PACKET_HEADER_SIZE,
};
use picoo_transport::TransportError;
use thiserror::Error;

pub use session::{SenderSession, SessionStats};

const MAX_FRAGMENT_PAYLOAD: usize = MAX_DATAGRAM_SIZE - VIDEO_PACKET_HEADER_SIZE;

#[derive(Debug, Error)]
pub enum SenderError {
    #[error("empty access unit")]
    EmptyAccessUnit,
    #[error("not connected")]
    NotConnected,
    #[error("transport: {0}")]
    Transport(#[from] TransportError),
    #[error("packet error: {0}")]
    Packet(#[from] VideoPacketError),
}

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct SenderStats {
    pub access_units: u64,
    pub packets: u64,
    pub bytes: u64,
}

#[derive(Default)]
pub struct SenderPipeline {
    frame_id: u64,
    stats: SenderStats,
    pending: Vec<VideoPacket>,
}

impl SenderPipeline {
    pub fn stats(&self) -> SenderStats {
        self.stats
    }

    pub fn pending_packets(&self) -> &[VideoPacket] {
        &self.pending
    }

    pub fn take_pending_packets(&mut self) -> Vec<VideoPacket> {
        std::mem::take(&mut self.pending)
    }

    /// Fragment one H.264 access unit into MTU-sized VideoPackets.
    pub fn ingest_access_unit(
        &mut self,
        data: &[u8],
        is_keyframe: bool,
        pts_us: u64,
        stream_epoch: u32,
    ) -> Result<usize, SenderError> {
        if data.is_empty() {
            return Err(SenderError::EmptyAccessUnit);
        }

        self.frame_id = self.frame_id.wrapping_add(1);
        let frame_id = self.frame_id;
        let fragment_count = data.len().div_ceil(MAX_FRAGMENT_PAYLOAD) as u16;
        let mut created = 0usize;

        for fragment_index in 0..fragment_count {
            let start = fragment_index as usize * MAX_FRAGMENT_PAYLOAD;
            let end = (start + MAX_FRAGMENT_PAYLOAD).min(data.len());
            let chunk = &data[start..end];

            let mut flags = VideoPacketFlags::empty();
            if is_keyframe {
                flags |= VideoPacketFlags::KEYFRAME;
            }
            if fragment_index == 0 {
                flags |= VideoPacketFlags::START_OF_ACCESS_UNIT;
            }
            if fragment_index + 1 == fragment_count {
                flags |= VideoPacketFlags::END_OF_ACCESS_UNIT;
            }

            let packet = VideoPacket {
                version: VideoPacket::VERSION,
                flags,
                stream_epoch,
                frame_id,
                pts_us,
                fragment_index,
                fragment_count,
                payload: Bytes::copy_from_slice(chunk),
            };
            packet.encode()?; // validate before queueing
            self.pending.push(packet);
            created += 1;
        }

        self.stats.access_units += 1;
        self.stats.packets += created as u64;
        self.stats.bytes += data.len() as u64;
        Ok(created)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use picoo_packet::ReassemblyMap;

    #[test]
    fn single_fragment_access_unit() {
        let mut sender = SenderPipeline::default();
        let count = sender
            .ingest_access_unit(b"h264-nalu", true, 100, 1)
            .expect("ingest");
        assert_eq!(count, 1);
        assert_eq!(sender.pending_packets().len(), 1);
        let packet = &sender.pending_packets()[0];
        assert!(packet.flags.contains(VideoPacketFlags::KEYFRAME));
        assert!(packet
            .flags
            .contains(VideoPacketFlags::START_OF_ACCESS_UNIT));
        assert!(packet.flags.contains(VideoPacketFlags::END_OF_ACCESS_UNIT));
    }

    #[test]
    fn large_access_unit_fragments_and_reassembles() {
        let payload = vec![7u8; MAX_FRAGMENT_PAYLOAD + 100];
        let mut sender = SenderPipeline::default();
        let count = sender
            .ingest_access_unit(&payload, false, 200, 2)
            .expect("ingest");
        assert_eq!(count, 2);

        let mut map = ReassemblyMap::new(8, 16);
        let mut assembled = None;
        for packet in sender.take_pending_packets() {
            if let Ok(frame) = map.ingest(packet) {
                assembled = frame;
            }
        }
        assert_eq!(assembled.as_deref(), Some(payload.as_slice()));
    }
}
