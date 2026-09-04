//! Android/iOS sender pipeline: H.264 access unit → PCP FEC-protected fragmentation.
//!
//! REQ-PICOO-MEDIA-001, REQ-PICOO-STACK-001

mod session;
mod stream_config;

use std::collections::VecDeque;

use bytes::Bytes;
use picoo_pairing::{PairingError, StoreError};
use picoo_protocol::{
    fec_group_ranges, make_fec_parity, VideoPacket, VideoPacketError, VideoPacketFlags,
    MAX_FEC_FRAGMENT_PAYLOAD, MAX_VIDEO_FRAGMENTS_PER_ACCESS_UNIT,
};
use picoo_transport::{TransportError, VideoDatagramBatch, VideoDatagramBatchError};
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
    #[error("video batch error: {0}")]
    VideoBatch(#[from] VideoDatagramBatchError),
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
    pub dropped_access_units: u64,
}

/// Number of Reed-Solomon parity shards emitted for each eligible FEC group.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub enum FecProtection {
    #[default]
    None,
    Light,
    Strong,
}

impl FecProtection {
    fn parity_shards(self) -> usize {
        match self {
            Self::None => 0,
            Self::Light => 1,
            Self::Strong => 2,
        }
    }
}

#[derive(Debug, Clone, Copy)]
struct FragmentDescriptor {
    index: u16,
    start: usize,
    end: usize,
}

#[derive(Default)]
pub struct SenderPipeline {
    frame_id: u64,
    stats: SenderStats,
    pending: VecDeque<VideoDatagramBatch>,
    au_scratch: Vec<u8>,
    descriptor_scratch: Vec<FragmentDescriptor>,
}

impl SenderPipeline {
    pub fn stats(&self) -> SenderStats {
        self.stats
    }

    pub fn pending_datagram_count(&self) -> usize {
        self.pending.iter().map(VideoDatagramBatch::len).sum()
    }

    pub fn take_pending_batches(&mut self) -> VecDeque<VideoDatagramBatch> {
        std::mem::take(&mut self.pending)
    }

    pub fn clear_pending_packets(&mut self) {
        self.pending.clear();
    }

    /// Fragment one H.264 access unit into final MTU-sized PCP datagrams.
    pub fn ingest_access_unit(
        &mut self,
        data: &[u8],
        is_keyframe: bool,
        pts_us: u64,
        stream_epoch: u32,
        fec: FecProtection,
    ) -> Result<usize, SenderError> {
        if data.is_empty() {
            return Err(SenderError::EmptyAccessUnit);
        }
        if data.len().div_ceil(MAX_FEC_FRAGMENT_PAYLOAD)
            > usize::from(MAX_VIDEO_FRAGMENTS_PER_ACCESS_UNIT)
        {
            return Err(SenderError::AccessUnitTooLarge);
        }
        self.au_scratch.clear();
        self.au_scratch.extend_from_slice(data);
        self.packetize_scratch(is_keyframe, pts_us, pts_us, stream_epoch, fec)
    }

    /// Packetize an AU carrying the native encoder-completion timestamp in
    /// the same monotonic domain as its source PTS.
    pub fn ingest_timed_access_unit(
        &mut self,
        data: &[u8],
        is_keyframe: bool,
        pts_us: u64,
        encoded_at_us: u64,
        stream_epoch: u32,
        fec: FecProtection,
    ) -> Result<usize, SenderError> {
        if data.is_empty() {
            return Err(SenderError::EmptyAccessUnit);
        }
        if data.len().div_ceil(MAX_FEC_FRAGMENT_PAYLOAD)
            > usize::from(MAX_VIDEO_FRAGMENTS_PER_ACCESS_UNIT)
        {
            return Err(SenderError::AccessUnitTooLarge);
        }
        self.au_scratch.clear();
        self.au_scratch.extend_from_slice(data);
        self.packetize_scratch(is_keyframe, pts_us, encoded_at_us, stream_epoch, fec)
    }

    /// Packetize one Rust-owned AU directly into final PCP wire datagrams.
    pub fn ingest_owned_access_unit(
        &mut self,
        data: Bytes,
        is_keyframe: bool,
        pts_us: u64,
        stream_epoch: u32,
        fec: FecProtection,
    ) -> Result<usize, SenderError> {
        self.packetize_access_unit(&data, is_keyframe, pts_us, pts_us, stream_epoch, fec)
    }

    fn packetize_scratch(
        &mut self,
        is_keyframe: bool,
        pts_us: u64,
        encoded_at_us: u64,
        stream_epoch: u32,
        fec: FecProtection,
    ) -> Result<usize, SenderError> {
        let data = self.au_scratch.as_slice();
        packetize_into_pending(
            data,
            is_keyframe,
            pts_us,
            encoded_at_us,
            stream_epoch,
            fec,
            &mut self.frame_id,
            &mut self.stats,
            &mut self.pending,
            &mut self.descriptor_scratch,
        )
    }

    fn packetize_access_unit(
        &mut self,
        data: &[u8],
        is_keyframe: bool,
        pts_us: u64,
        encoded_at_us: u64,
        stream_epoch: u32,
        fec: FecProtection,
    ) -> Result<usize, SenderError> {
        packetize_into_pending(
            data,
            is_keyframe,
            pts_us,
            encoded_at_us,
            stream_epoch,
            fec,
            &mut self.frame_id,
            &mut self.stats,
            &mut self.pending,
            &mut self.descriptor_scratch,
        )
    }
}

const PENDING_ACCESS_UNIT_CAPACITY: usize = 3;

#[allow(clippy::too_many_arguments)]
fn packetize_into_pending(
    data: &[u8],
    is_keyframe: bool,
    pts_us: u64,
    encoded_at_us: u64,
    stream_epoch: u32,
    fec: FecProtection,
    frame_id: &mut u64,
    stats: &mut SenderStats,
    pending: &mut VecDeque<VideoDatagramBatch>,
    descriptors: &mut Vec<FragmentDescriptor>,
) -> Result<usize, SenderError> {
    if data.is_empty() {
        return Err(SenderError::EmptyAccessUnit);
    }

    let fragment_count = data.len().div_ceil(MAX_FEC_FRAGMENT_PAYLOAD);
    if fragment_count > usize::from(MAX_VIDEO_FRAGMENTS_PER_ACCESS_UNIT) {
        return Err(SenderError::AccessUnitTooLarge);
    }
    let fragment_count =
        u16::try_from(fragment_count).expect("bounded fragment count always fits the wire u16");
    *frame_id = frame_id
        .checked_add(1)
        .ok_or(SenderError::FrameIdExhausted)?;
    let current_frame_id = *frame_id;
    descriptors.clear();
    descriptors.extend((0..fragment_count).map(|index| {
        let start = usize::from(index) * MAX_FEC_FRAGMENT_PAYLOAD;
        FragmentDescriptor {
            index,
            start,
            end: (start + MAX_FEC_FRAGMENT_PAYLOAD).min(data.len()),
        }
    }));
    let mut created = 0usize;
    let mut data_datagrams = Vec::with_capacity(usize::from(fragment_count));

    for descriptor in descriptors.iter() {
        let chunk = &data[descriptor.start..descriptor.end];

        let mut flags = VideoPacketFlags::empty();
        if is_keyframe {
            flags |= VideoPacketFlags::KEYFRAME;
        }
        if descriptor.index == 0 {
            flags |= VideoPacketFlags::START_OF_ACCESS_UNIT;
        }
        if descriptor.index + 1 == fragment_count {
            flags |= VideoPacketFlags::END_OF_ACCESS_UNIT;
        }

        let datagram = VideoPacket::encode_datagram(
            flags,
            stream_epoch,
            current_frame_id,
            pts_us,
            encoded_at_us,
            descriptor.index,
            fragment_count,
            chunk,
        )?;
        data_datagrams.push(datagram);
        created += 1;
    }

    let base_flags = if is_keyframe {
        VideoPacketFlags::KEYFRAME
    } else {
        VideoPacketFlags::empty()
    };
    let fec_groups = fec_group_ranges(fragment_count);
    let parity_count = fec.parity_shards();
    let mut parity_datagrams_by_group = Vec::with_capacity(fec_groups.len());
    for group in &fec_groups {
        if parity_count == 0 {
            parity_datagrams_by_group.push(Vec::new());
            continue;
        }
        let group_data = group
            .clone()
            .map(|index| {
                let descriptor = descriptors[usize::from(index)];
                &data[descriptor.start..descriptor.end]
            })
            .collect::<Vec<_>>();
        let Some(parity_shards) = make_fec_parity(group.start, &group_data) else {
            parity_datagrams_by_group.push(Vec::new());
            continue;
        };
        let mut group_datagrams = Vec::with_capacity(parity_count);
        for parity in parity_shards.into_iter().take(parity_count) {
            let last_data_len = parity.last_data_len.to_be_bytes();
            let prefix = [parity.parity_index, last_data_len[0], last_data_len[1]];
            let datagram = VideoPacket::encode_datagram_segments(
                base_flags | VideoPacketFlags::FEC_PARITY,
                stream_epoch,
                current_frame_id,
                pts_us,
                encoded_at_us,
                parity.group_start,
                fragment_count,
                &[&prefix, &parity.bytes],
            )?;
            group_datagrams.push(datagram);
            created += 1;
        }
        parity_datagrams_by_group.push(group_datagrams);
    }

    let datagrams = schedule_fec_datagrams(data_datagrams, parity_datagrams_by_group, &fec_groups);
    if pending.len() == PENDING_ACCESS_UNIT_CAPACITY {
        let drop_index = pending
            .iter()
            .position(|batch| !batch.is_keyframe())
            .unwrap_or(0);
        pending.remove(drop_index);
        stats.dropped_access_units = stats.dropped_access_units.saturating_add(1);
    }
    pending.push_back(VideoDatagramBatch::new(datagrams)?);

    stats.access_units += 1;
    stats.packets += created as u64;
    stats.bytes += data.len() as u64;
    Ok(created)
}

/// Interleave equal-position shards across FEC groups, while keeping all source
/// data ahead of parity. A short consecutive Wi-Fi loss burst is therefore
/// spread across independent Reed-Solomon blocks, but a healthy path completes
/// the AU from original data and never pays needless reconstruction work.
fn schedule_fec_datagrams(
    data_datagrams: Vec<Bytes>,
    parity_datagrams_by_group: Vec<Vec<Bytes>>,
    groups: &[std::ops::Range<u16>],
) -> Vec<Bytes> {
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

    let mut data = data_datagrams.into_iter().map(Some).collect::<Vec<_>>();
    let mut parity = parity_datagrams_by_group
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

    fn pending_packets(sender: &SenderPipeline) -> Vec<VideoPacket> {
        sender
            .pending
            .iter()
            .flat_map(VideoDatagramBatch::datagrams)
            .map(|datagram| VideoPacket::decode_bytes(datagram.clone()).expect("valid datagram"))
            .collect()
    }

    #[test]
    fn single_fragment_access_unit() {
        let mut sender = SenderPipeline::default();
        let count = sender
            .ingest_access_unit(b"h264-nalu", true, 100, 1, FecProtection::Strong)
            .expect("ingest");
        assert_eq!(count, 1);
        let packets = pending_packets(&sender);
        assert_eq!(packets.len(), 1);
        let packet = &packets[0];
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
            sender.ingest_access_unit(b"h264", true, 0, 1, FecProtection::Strong),
            Err(SenderError::FrameIdExhausted)
        ));
        assert_eq!(sender.pending_datagram_count(), 0);
    }

    #[test]
    fn large_access_unit_fragments_and_reassembles() {
        let payload = vec![7u8; MAX_FEC_FRAGMENT_PAYLOAD + 100];
        let mut sender = SenderPipeline::default();
        let count = sender
            .ingest_access_unit(&payload, false, 200, 2, FecProtection::Strong)
            .expect("ingest");
        assert_eq!(count, 4, "two data and two parity packets");

        let mut map = ReassemblyMap::new(8, 16);
        let mut assembled = None;
        for batch in sender.take_pending_batches() {
            for datagram in batch.into_datagrams() {
                let packet = VideoPacket::decode_bytes(datagram).expect("valid datagram");
                if let Ok(Some(frame)) = map.ingest(packet) {
                    assembled = Some(frame.data);
                }
            }
        }
        assert_eq!(assembled.as_deref(), Some(payload.as_slice()));
    }

    #[test]
    fn fec_schedule_spreads_groups_and_keeps_parity_after_source_data() {
        let payload = vec![9u8; MAX_FEC_FRAGMENT_PAYLOAD * 10];
        let mut sender = SenderPipeline::default();
        sender
            .ingest_access_unit(&payload, false, 200, 2, FecProtection::Strong)
            .expect("ingest");
        let packets = pending_packets(&sender);
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
            sender.ingest_access_unit(&payload, true, 0, 1, FecProtection::Strong),
            Err(SenderError::AccessUnitTooLarge)
        ));
        assert_eq!(sender.pending_datagram_count(), 0);
    }

    #[test]
    fn rejected_access_unit_does_not_create_a_wire_frame_id_gap() {
        let oversized =
            vec![
                0_u8;
                MAX_FEC_FRAGMENT_PAYLOAD * usize::from(MAX_VIDEO_FRAGMENTS_PER_ACCESS_UNIT) + 1
            ];
        let mut sender = SenderPipeline::default();
        assert!(sender
            .ingest_access_unit(&oversized, false, 0, 1, FecProtection::None)
            .is_err());
        sender
            .ingest_access_unit(b"valid", false, 1, 1, FecProtection::None)
            .expect("valid AU");
        assert_eq!(pending_packets(&sender)[0].frame_id, 1);
    }

    #[test]
    fn pending_access_unit_queue_is_bounded() {
        let mut sender = SenderPipeline::default();
        for pts in 0..=PENDING_ACCESS_UNIT_CAPACITY {
            sender
                .ingest_access_unit(b"frame", false, pts as u64, 1, FecProtection::None)
                .expect("ingest");
        }
        assert_eq!(sender.pending.len(), PENDING_ACCESS_UNIT_CAPACITY);
        assert_eq!(sender.stats().dropped_access_units, 1);
        assert_eq!(pending_packets(&sender)[0].frame_id, 2);
    }

    #[test]
    fn bounded_queue_evicts_oldest_delta_before_keyframe() {
        let mut sender = SenderPipeline::default();
        sender
            .ingest_access_unit(b"idr", true, 0, 1, FecProtection::None)
            .expect("IDR");
        for pts in 1..=3 {
            sender
                .ingest_access_unit(b"delta", false, pts, 1, FecProtection::None)
                .expect("delta");
        }
        let packets = pending_packets(&sender);
        let frame_ids = packets
            .iter()
            .map(|packet| packet.frame_id)
            .collect::<Vec<_>>();
        assert_eq!(frame_ids, vec![1, 3, 4]);
        assert!(packets[0].flags.contains(VideoPacketFlags::KEYFRAME));
    }

    #[test]
    fn adaptive_fec_emits_zero_one_or_two_parity_shards() {
        let payload = vec![5_u8; MAX_FEC_FRAGMENT_PAYLOAD * 2];
        for (protection, expected_parity) in [
            (FecProtection::None, 0),
            (FecProtection::Light, 1),
            (FecProtection::Strong, 2),
        ] {
            let mut sender = SenderPipeline::default();
            sender
                .ingest_access_unit(&payload, false, 0, 1, protection)
                .expect("ingest");
            let parity = pending_packets(&sender)
                .into_iter()
                .filter(|packet| packet.flags.contains(VideoPacketFlags::FEC_PARITY))
                .count();
            assert_eq!(parity, expected_parity);
        }
    }

    #[test]
    fn owned_access_unit_is_packetized_without_fragment_backing_allocations() {
        let payload = Bytes::from(vec![7_u8; MAX_FEC_FRAGMENT_PAYLOAD + 4]);
        let mut sender = SenderPipeline::default();
        sender
            .ingest_owned_access_unit(payload.clone(), false, 0, 1, FecProtection::None)
            .expect("ingest owned AU");
        let packets = pending_packets(&sender);
        assert_eq!(packets.len(), 2);
        assert_eq!(packets[0].payload.len(), MAX_FEC_FRAGMENT_PAYLOAD);
        assert_eq!(
            packets[1].payload.as_ref(),
            &payload[MAX_FEC_FRAGMENT_PAYLOAD..]
        );
    }
}
