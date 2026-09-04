//! Android/iOS sender pipeline: H.264 access unit → PCP FEC-protected fragmentation.
//!
//! REQ-PICOO-MEDIA-001, REQ-PICOO-STACK-001

mod session;
mod stream_config;

use bytes::Bytes;
use picoo_pairing::{PairingError, StoreError};
use picoo_protocol::{
    fec_group_ranges, make_fec_parity, VideoPacket, VideoPacketError, VideoPacketFlags,
    FEC_PARITY_PREFIX_SIZE, MAX_FEC_FRAGMENT_PAYLOAD, MAX_VIDEO_FRAGMENTS_PER_ACCESS_UNIT,
};
use picoo_transport::TransportError;
use thiserror::Error;

pub use picoo_rate_control::BitrateAction;
pub use session::{
    EncoderDirective, EncoderDirectiveKind, EncoderFailureOutcome, NativeEncoderAccessUnit,
    SenderSession, SessionStats, INITIAL_STREAM_EPOCH, MAX_STREAM_EPOCH,
};
pub use stream_config::StreamConfigParams;

#[derive(Debug, Error)]
pub enum SenderError {
    #[error("empty access unit")]
    EmptyAccessUnit,
    #[error("access unit requires too many datagram fragments")]
    AccessUnitTooLarge,
    #[error("frame id exhausted; start a new sender session")]
    FrameIdExhausted,
    #[error("not connected")]
    NotConnected,
    #[error("media is blocked until pairing and negotiation complete")]
    MediaNotReady,
    #[error("transport: {0}")]
    Transport(#[from] TransportError),
    #[error("packet error: {0}")]
    Packet(#[from] VideoPacketError),
    #[error("protocol: {0}")]
    Protocol(String),
    #[error("pairing: {0}")]
    Pairing(#[from] PairingError),
    #[error("pairing store: {0}")]
    Store(#[from] StoreError),
    #[error("stale stream epoch: got {got}, current {current}")]
    StaleStreamEpoch { got: u32, current: u32 },
    #[error("stream config for epoch {stream_epoch} has not been sent")]
    StreamConfigPending { stream_epoch: u32 },
    #[error("stream config height mismatch: expected {expected}, got {got}")]
    StreamConfigHeightMismatch { expected: u32, got: u32 },
    #[error("encoded access unit does not match the active encoder transaction/generation")]
    StaleEncoderFact,
    #[error("encoder transaction is waiting for its first matching IDR")]
    EncoderRefreshPending,
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

    pub fn clear_pending_packets(&mut self) {
        self.pending.clear();
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

        self.frame_id = self
            .frame_id
            .checked_add(1)
            .ok_or(SenderError::FrameIdExhausted)?;
        let frame_id = self.frame_id;
        let fragment_count = data.len().div_ceil(MAX_FEC_FRAGMENT_PAYLOAD);
        if fragment_count > usize::from(MAX_VIDEO_FRAGMENTS_PER_ACCESS_UNIT) {
            return Err(SenderError::AccessUnitTooLarge);
        }
        let fragment_count =
            u16::try_from(fragment_count).expect("bounded fragment count always fits the wire u16");
        let mut created = 0usize;
        let mut data_packets = Vec::with_capacity(usize::from(fragment_count));

        for fragment_index in 0..fragment_count {
            let start = fragment_index as usize * MAX_FEC_FRAGMENT_PAYLOAD;
            let end = (start + MAX_FEC_FRAGMENT_PAYLOAD).min(data.len());
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
                flags,
                stream_epoch,
                frame_id,
                pts_us,
                fragment_index,
                fragment_count,
                payload: Bytes::copy_from_slice(chunk),
            };
            packet.encode()?; // validate before queueing
            data_packets.push(packet);
            created += 1;
        }

        let base_flags = if is_keyframe {
            VideoPacketFlags::KEYFRAME
        } else {
            VideoPacketFlags::empty()
        };
        let fec_groups = fec_group_ranges(fragment_count);
        let mut parity_packets_by_group = Vec::with_capacity(fec_groups.len());
        for group in &fec_groups {
            let group_data = group
                .clone()
                .map(|index| data_packets[usize::from(index)].payload.as_ref())
                .collect::<Vec<_>>();
            let Some(parity_shards) = make_fec_parity(group.start, &group_data) else {
                parity_packets_by_group.push(Vec::new());
                continue;
            };
            let mut group_packets = Vec::with_capacity(parity_shards.len());
            for parity in parity_shards {
                let mut payload = Vec::with_capacity(FEC_PARITY_PREFIX_SIZE + parity.bytes.len());
                payload.push(parity.parity_index);
                payload.extend_from_slice(&parity.last_data_len.to_be_bytes());
                payload.extend_from_slice(&parity.bytes);
                let packet = VideoPacket {
                    flags: base_flags | VideoPacketFlags::FEC_PARITY,
                    stream_epoch,
                    frame_id,
                    pts_us,
                    fragment_index: parity.group_start,
                    fragment_count,
                    payload: Bytes::from(payload),
                };
                packet.encode()?;
                group_packets.push(packet);
                created += 1;
            }
            parity_packets_by_group.push(group_packets);
        }

        self.pending.extend(schedule_fec_packets(
            data_packets,
            parity_packets_by_group,
            &fec_groups,
        ));

        self.stats.access_units += 1;
        self.stats.packets += created as u64;
        self.stats.bytes += data.len() as u64;
        Ok(created)
    }
}

/// Interleave equal-position shards across FEC groups, while keeping all source
/// data ahead of parity. A short consecutive Wi-Fi loss burst is therefore
/// spread across independent Reed-Solomon blocks, but a healthy path completes
/// the AU from original data and never pays needless reconstruction work.
fn schedule_fec_packets(
    data_packets: Vec<VideoPacket>,
    parity_packets_by_group: Vec<Vec<VideoPacket>>,
    groups: &[std::ops::Range<u16>],
) -> Vec<VideoPacket> {
    #[derive(Clone, Copy)]
    enum Slot {
        Data(usize),
        Parity(usize),
    }

    const SLOTS: [Slot; 8] = [
        Slot::Data(0),
        Slot::Data(1),
        Slot::Data(2),
        Slot::Data(3),
        Slot::Data(4),
        Slot::Data(5),
        Slot::Parity(0),
        Slot::Parity(1),
    ];

    let mut data = data_packets.into_iter().map(Some).collect::<Vec<_>>();
    let mut parity = parity_packets_by_group
        .into_iter()
        .map(|packets| packets.into_iter().map(Some).collect::<Vec<_>>())
        .collect::<Vec<_>>();
    let total = data.len() + parity.iter().map(Vec::len).sum::<usize>();
    let mut scheduled = Vec::with_capacity(total);
    for slot in SLOTS {
        for (group_index, group) in groups.iter().enumerate() {
            let packet = match slot {
                Slot::Data(offset) => {
                    let index = usize::from(group.start).saturating_add(offset);
                    (index < usize::from(group.end))
                        .then(|| data[index].take())
                        .flatten()
                }
                Slot::Parity(index) => parity
                    .get_mut(group_index)
                    .and_then(|packets| packets.get_mut(index))
                    .and_then(Option::take),
            };
            if let Some(packet) = packet {
                scheduled.push(packet);
            }
        }
    }
    debug_assert_eq!(scheduled.len(), total);
    scheduled
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
    fn frame_id_exhaustion_never_wraps_or_queues_media() {
        let mut sender = SenderPipeline {
            frame_id: u64::MAX,
            ..Default::default()
        };
        assert!(matches!(
            sender.ingest_access_unit(b"h264", true, 0, 1),
            Err(SenderError::FrameIdExhausted)
        ));
        assert!(sender.pending_packets().is_empty());
    }

    #[test]
    fn large_access_unit_fragments_and_reassembles() {
        let payload = vec![7u8; MAX_FEC_FRAGMENT_PAYLOAD + 100];
        let mut sender = SenderPipeline::default();
        let count = sender
            .ingest_access_unit(&payload, false, 200, 2)
            .expect("ingest");
        assert_eq!(count, 4, "two data and two parity packets");

        let mut map = ReassemblyMap::new(8, 16);
        let mut assembled = None;
        for packet in sender.take_pending_packets() {
            if let Ok(Some(frame)) = map.ingest(packet) {
                assembled = Some(frame.data);
            }
        }
        assert_eq!(assembled.as_deref(), Some(payload.as_slice()));
    }

    #[test]
    fn fec_schedule_spreads_groups_and_keeps_parity_after_source_data() {
        let payload = vec![9u8; MAX_FEC_FRAGMENT_PAYLOAD * 10];
        let mut sender = SenderPipeline::default();
        sender
            .ingest_access_unit(&payload, false, 200, 2)
            .expect("ingest");
        let packets = sender.pending_packets();
        // 10 fragments balance into 0..5 and 5..10. Round-robin scheduling
        // alternates groups, but all source fragments precede parity so a
        // healthy receiver never reconstructs data that is already in flight.
        assert_eq!(packets[0].fragment_index, 0);
        assert_eq!(packets[1].fragment_index, 5);
        assert_eq!(packets[2].fragment_index, 1);
        assert_eq!(packets[3].fragment_index, 6);
        assert_eq!(packets[4].fragment_index, 2);
        assert_eq!(packets[5].fragment_index, 7);
        assert!(packets[..10]
            .iter()
            .all(|packet| !packet.flags.contains(VideoPacketFlags::FEC_PARITY)));
        assert!(packets[10..]
            .iter()
            .all(|packet| packet.flags.contains(VideoPacketFlags::FEC_PARITY)));
    }

    #[test]
    fn access_unit_over_reassembly_budget_is_rejected_before_queueing() {
        let payload =
            vec![
                0u8;
                MAX_FEC_FRAGMENT_PAYLOAD * usize::from(MAX_VIDEO_FRAGMENTS_PER_ACCESS_UNIT) + 1
            ];
        let mut sender = SenderPipeline::default();
        assert!(matches!(
            sender.ingest_access_unit(&payload, true, 0, 1),
            Err(SenderError::AccessUnitTooLarge)
        ));
        assert!(sender.pending_packets().is_empty());
    }
}
