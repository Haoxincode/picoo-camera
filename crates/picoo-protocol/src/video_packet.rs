use bytes::{Buf, BufMut, Bytes, BytesMut};
use thiserror::Error;

use crate::{MAX_DATAGRAM_SIZE, VIDEO_PACKET_HEADER_SIZE};

bitflags::bitflags! {
    #[derive(Debug, Clone, Copy, PartialEq, Eq)]
    pub struct VideoPacketFlags: u8 {
        const KEYFRAME = 0x01;
        const START_OF_ACCESS_UNIT = 0x02;
        const END_OF_ACCESS_UNIT = 0x04;
        const DISCARDABLE = 0x08;
        const FEC_PARITY = 0x10;
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct VideoPacket {
    pub flags: VideoPacketFlags,
    pub stream_epoch: u32,
    pub frame_id: u64,
    pub pts_us: u64,
    pub fragment_index: u16,
    pub fragment_count: u16,
    pub payload: Bytes,
}

#[derive(Debug, Error, PartialEq, Eq)]
pub enum VideoPacketError {
    #[error("buffer too short for VideoPacket header")]
    BufferTooShort,
    #[error("datagram exceeds maximum size of {MAX_DATAGRAM_SIZE} bytes")]
    DatagramTooLarge,
    #[error("fragment_index must be less than fragment_count")]
    InvalidFragmentIndex,
}

impl VideoPacket {
    pub fn encode(&self) -> Result<Bytes, VideoPacketError> {
        Self::encode_datagram(
            self.flags,
            self.stream_epoch,
            self.frame_id,
            self.pts_us,
            self.fragment_index,
            self.fragment_count,
            &self.payload,
        )
    }

    /// Encode a final wire datagram directly from an AU-backed payload slice.
    ///
    /// Sender packetization uses this entry point so it never creates an
    /// intermediate `VideoPacket` whose payload must be copied a second time.
    #[allow(clippy::too_many_arguments)]
    pub fn encode_datagram(
        flags: VideoPacketFlags,
        stream_epoch: u32,
        frame_id: u64,
        pts_us: u64,
        fragment_index: u16,
        fragment_count: u16,
        payload: &[u8],
    ) -> Result<Bytes, VideoPacketError> {
        Self::encode_datagram_segments(
            flags,
            stream_epoch,
            frame_id,
            pts_us,
            fragment_index,
            fragment_count,
            &[payload],
        )
    }

    /// Encode a final datagram from adjacent payload segments without first
    /// joining them into a temporary allocation.
    #[allow(clippy::too_many_arguments)]
    pub fn encode_datagram_segments(
        flags: VideoPacketFlags,
        stream_epoch: u32,
        frame_id: u64,
        pts_us: u64,
        fragment_index: u16,
        fragment_count: u16,
        payload_segments: &[&[u8]],
    ) -> Result<Bytes, VideoPacketError> {
        if fragment_index >= fragment_count {
            return Err(VideoPacketError::InvalidFragmentIndex);
        }

        let payload_len = payload_segments
            .iter()
            .try_fold(0_usize, |total, segment| total.checked_add(segment.len()));
        let Some(total) = payload_len.and_then(|len| VIDEO_PACKET_HEADER_SIZE.checked_add(len))
        else {
            return Err(VideoPacketError::DatagramTooLarge);
        };
        if total > MAX_DATAGRAM_SIZE {
            return Err(VideoPacketError::DatagramTooLarge);
        }

        let mut buf = BytesMut::with_capacity(total);
        buf.put_u8(flags.bits());
        buf.put_u32(stream_epoch);
        buf.put_u64(frame_id);
        buf.put_u64(pts_us);
        buf.put_u16(fragment_index);
        buf.put_u16(fragment_count);
        for segment in payload_segments {
            buf.put_slice(segment);
        }
        Ok(buf.freeze())
    }

    pub fn decode(buf: &[u8]) -> Result<Self, VideoPacketError> {
        Self::decode_bytes(Bytes::copy_from_slice(buf))
    }

    /// Decode a Quinn-owned datagram while retaining its payload storage.
    pub fn decode_bytes(mut buf: Bytes) -> Result<Self, VideoPacketError> {
        if buf.len() < VIDEO_PACKET_HEADER_SIZE {
            return Err(VideoPacketError::BufferTooShort);
        }
        if buf.len() > MAX_DATAGRAM_SIZE {
            return Err(VideoPacketError::DatagramTooLarge);
        }

        let flags = VideoPacketFlags::from_bits(buf.get_u8()).unwrap_or(VideoPacketFlags::empty());
        let stream_epoch = buf.get_u32();
        let frame_id = buf.get_u64();
        let pts_us = buf.get_u64();
        let fragment_index = buf.get_u16();
        let fragment_count = buf.get_u16();
        let payload = buf;

        let packet = Self {
            flags,
            stream_epoch,
            frame_id,
            pts_us,
            fragment_index,
            fragment_count,
            payload,
        };

        if packet.fragment_index >= packet.fragment_count {
            return Err(VideoPacketError::InvalidFragmentIndex);
        }

        Ok(packet)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn roundtrip_encode_decode() {
        let packet = VideoPacket {
            flags: VideoPacketFlags::KEYFRAME | VideoPacketFlags::START_OF_ACCESS_UNIT,
            stream_epoch: 2,
            frame_id: 42,
            pts_us: 1_000_000,
            fragment_index: 0,
            fragment_count: 3,
            payload: Bytes::from_static(b"h264-chunk"),
        };

        let encoded = packet.encode().unwrap();
        assert_eq!(
            encoded.len(),
            VIDEO_PACKET_HEADER_SIZE + packet.payload.len()
        );
        let decoded = VideoPacket::decode(&encoded).unwrap();
        assert_eq!(decoded, packet);
    }

    #[test]
    fn owned_datagram_decode_reuses_payload_storage() {
        let encoded =
            VideoPacket::encode_datagram(VideoPacketFlags::empty(), 1, 2, 3, 0, 1, b"payload")
                .expect("encode");
        let expected_payload = encoded[VIDEO_PACKET_HEADER_SIZE..].as_ptr();
        let decoded = VideoPacket::decode_bytes(encoded).expect("decode");
        assert_eq!(decoded.payload.as_ptr(), expected_payload);
    }

    #[test]
    fn rejects_oversized_datagram() {
        let packet = VideoPacket {
            flags: VideoPacketFlags::empty(),
            stream_epoch: 0,
            frame_id: 0,
            pts_us: 0,
            fragment_index: 0,
            fragment_count: 1,
            payload: Bytes::from(vec![0u8; MAX_DATAGRAM_SIZE]),
        };
        assert_eq!(packet.encode(), Err(VideoPacketError::DatagramTooLarge));
    }

    #[test]
    fn decode_never_panics_on_random_bytes() {
        // Lightweight stand-in that always runs; full fuzz target lives under /fuzz.
        let mut state: u64 = 0xC0FFEE_u64;
        for _ in 0..2_000 {
            state = state.wrapping_mul(6364136223846793005).wrapping_add(1);
            let len = (state % 1_400) as usize;
            let mut buf = vec![0u8; len];
            for byte in &mut buf {
                state = state.wrapping_mul(6364136223846793005).wrapping_add(1);
                *byte = (state >> 33) as u8;
            }
            let _ = VideoPacket::decode(&buf);
        }
    }

    #[test]
    fn header_size_is_twenty_five_bytes() {
        assert_eq!(VIDEO_PACKET_HEADER_SIZE, 25);
    }

    #[test]
    fn rejects_invalid_fragment_index() {
        let packet = VideoPacket {
            flags: VideoPacketFlags::empty(),
            stream_epoch: 0,
            frame_id: 0,
            pts_us: 0,
            fragment_index: 2,
            fragment_count: 2,
            payload: Bytes::from_static(b"x"),
        };
        assert_eq!(packet.encode(), Err(VideoPacketError::InvalidFragmentIndex));
    }
}
