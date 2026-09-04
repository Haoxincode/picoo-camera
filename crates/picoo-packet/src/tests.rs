use super::*;
use picoo_protocol::{make_fec_parity, VideoPacket};

fn fragment(
    epoch: u32,
    frame_id: u64,
    index: u16,
    count: u16,
    payload: &'static [u8],
) -> VideoPacket {
    VideoPacket {
        flags: VideoPacketFlags::empty(),
        stream_epoch: epoch,
        frame_id,
        pts_us: 0,
        encoded_at_us: 0,
        fragment_index: index,
        fragment_count: count,
        payload: Bytes::copy_from_slice(payload),
    }
}

fn parity_fragments(
    epoch: u32,
    frame_id: u64,
    group_start: u16,
    count: u16,
    data: &[&[u8]],
) -> Vec<VideoPacket> {
    make_fec_parity(group_start, data)
        .expect("FEC parity")
        .into_iter()
        .map(|parity| {
            let mut payload = Vec::with_capacity(FEC_PARITY_PREFIX_SIZE + parity.bytes.len());
            payload.push(parity.parity_index);
            payload.extend_from_slice(&parity.last_data_len.to_be_bytes());
            payload.extend_from_slice(&parity.bytes);
            VideoPacket {
                flags: VideoPacketFlags::FEC_PARITY,
                stream_epoch: epoch,
                frame_id,
                pts_us: 0,
                encoded_at_us: 0,
                fragment_index: group_start,
                fragment_count: count,
                payload: Bytes::from(payload),
            }
        })
        .collect()
}

#[test]
fn reassembles_fragments_same_epoch() {
    let mut map = ReassemblyMap::new(8, 16);
    assert!(map.ingest(fragment(1, 10, 0, 2, b"ab")).unwrap().is_none());
    assert_eq!(map.oldest_pending_pts_us(), Some(0));
    let assembled = map.ingest(fragment(1, 10, 1, 2, b"cd")).unwrap();
    assert_eq!(
        assembled.as_ref().map(|a| a.data.as_ref()),
        Some(&b"abcd"[..])
    );
    assert_eq!(map.missing_fragment_count(), 0);
    assert_eq!(map.resolved_fragment_count(), 2);
    assert_eq!(map.oldest_pending_pts_us(), None);
}

#[test]
fn fec_recovers_two_missing_data_fragments_without_waiting_for_deadline() {
    let data = [b"aa".as_slice(), b"bb", b"cc", b"dd", b"ee", b"f"];
    let mut map = ReassemblyMap::new(8, 16);
    for index in [0_u16, 2, 3, 5] {
        assert!(map
            .ingest(fragment(1, 10, index, 6, data[usize::from(index)]))
            .unwrap()
            .is_none());
    }
    let mut assembled = None;
    for parity in parity_fragments(1, 10, 0, 6, &data) {
        if let Some(access_unit) = map.ingest(parity).unwrap() {
            assembled = Some(access_unit.data);
        }
    }
    assert_eq!(assembled.as_deref(), Some(&b"aabbccddeef"[..]));
    assert_eq!(map.fec_recovered_fragment_count(), 2);
    assert_eq!(map.drop_count(), 0);
    assert_eq!(map.missing_fragment_count(), 0);
}

#[test]
fn fec_does_not_hide_loss_beyond_its_recovery_budget() {
    let data = [b"aa".as_slice(), b"bb", b"cc", b"dd", b"ee", b"f"];
    let mut map = ReassemblyMap::new(8, 16);
    for index in [0_u16, 2, 5] {
        assert!(map
            .ingest(fragment(1, 10, index, 6, data[usize::from(index)]))
            .unwrap()
            .is_none());
    }
    for parity in parity_fragments(1, 10, 0, 6, &data) {
        assert!(map.ingest(parity).unwrap().is_none());
    }
    map.expire_incomplete_older_than(Instant::now(), Duration::ZERO);
    assert_eq!(map.fec_recovered_fragment_count(), 0);
    assert_eq!(map.drop_count(), 1);
    assert_eq!(map.missing_fragment_count(), 3);
}

#[test]
fn malformed_fec_parity_is_rejected() {
    let mut map = ReassemblyMap::new(8, 16);
    let parity = VideoPacket {
        flags: VideoPacketFlags::FEC_PARITY,
        stream_epoch: 1,
        frame_id: 10,
        pts_us: 0,
        encoded_at_us: 0,
        fragment_index: 0,
        fragment_count: 2,
        payload: Bytes::from_static(b"bad"),
    };
    assert_eq!(map.ingest(parity), Err(ReassemblyError::InvalidFecParity));
}

#[test]
fn isolates_stream_epochs() {
    let mut map = ReassemblyMap::new(8, 16);
    assert!(map.ingest(fragment(1, 10, 0, 2, b"ab")).unwrap().is_none());
    assert!(map.ingest(fragment(2, 10, 0, 1, b"xy")).unwrap().is_some());
}

#[test]
fn whole_access_unit_gap_blocks_until_the_missing_frame_arrives() {
    let mut map = ReassemblyMap::new(8, 16);
    assert!(map.ingest(fragment(1, 1, 0, 1, b"one")).unwrap().is_some());
    assert!(map
        .ingest(fragment(1, 3, 0, 1, b"three"))
        .unwrap()
        .is_some());
    assert_eq!(map.oldest_unresolved_frame_id(), Some(2));
    let gap_noticed_at = map.frame_gaps.get(&2).expect("gap 2").noticed_at;

    let assembled = map
        .ingest(fragment(1, 2, 0, 1, b"two"))
        .unwrap()
        .expect("late older AU completes");
    assert_eq!(assembled.first_fragment_at, gap_noticed_at);
    assert_eq!(map.oldest_unresolved_frame_id(), None);
    assert!(!map.take_reference_chain_loss());
}

#[test]
fn whole_access_unit_gap_expires_into_reference_recovery() {
    let mut map = ReassemblyMap::new(8, 16);
    assert!(map.ingest(fragment(1, 1, 0, 1, b"one")).unwrap().is_some());
    assert!(map
        .ingest(fragment(1, 3, 0, 1, b"three"))
        .unwrap()
        .is_some());

    map.expire_incomplete_older_than(Instant::now(), Duration::ZERO);
    assert_eq!(map.oldest_unresolved_frame_id(), None);
    assert_eq!(map.drop_count(), 1);
    assert!(map.take_reference_chain_loss());
    assert!(map.ingest(fragment(1, 2, 0, 1, b"late")).unwrap().is_none());
    assert_eq!(map.drop_count(), 1);
}

#[test]
fn dropping_incomplete_keyframe_sets_loss_flag() {
    let mut map = ReassemblyMap::new(1, 16);
    let key = VideoPacket {
        flags: VideoPacketFlags::KEYFRAME,
        stream_epoch: 1,
        frame_id: 1,
        pts_us: 0,
        encoded_at_us: 0,
        fragment_index: 0,
        fragment_count: 2,
        payload: Bytes::copy_from_slice(b"k0"),
    };
    assert!(map.ingest(key).unwrap().is_none());
    // Force drop of the incomplete keyframe by admitting another frame.
    let other = fragment(1, 2, 0, 1, b"z");
    assert!(map.ingest(other).unwrap().is_some());
    assert!(map.take_keyframe_loss());
    assert!(!map.take_keyframe_loss());
}

#[test]
fn reassembly_deadline_reports_and_discards_incomplete_keyframe() {
    let mut map = ReassemblyMap::new(8, 16);
    let key_head = VideoPacket {
        flags: VideoPacketFlags::KEYFRAME | VideoPacketFlags::START_OF_ACCESS_UNIT,
        stream_epoch: 1,
        frame_id: 1,
        pts_us: 0,
        encoded_at_us: 0,
        fragment_index: 0,
        fragment_count: 2,
        payload: Bytes::copy_from_slice(b"k0"),
    };
    assert!(map.ingest(key_head).unwrap().is_none());

    map.expire_incomplete_older_than(Instant::now(), Duration::ZERO);
    assert!(map.take_keyframe_loss());
    assert_eq!(map.missing_fragment_count(), 1);
    assert_eq!(map.resolved_fragment_count(), 2);

    let key_tail = VideoPacket {
        flags: VideoPacketFlags::KEYFRAME | VideoPacketFlags::END_OF_ACCESS_UNIT,
        stream_epoch: 1,
        frame_id: 1,
        pts_us: 0,
        encoded_at_us: 0,
        fragment_index: 1,
        fragment_count: 2,
        payload: Bytes::copy_from_slice(b"k1"),
    };
    assert!(map.ingest(key_tail).unwrap().is_none());
    assert!(!map.take_keyframe_loss());
    assert_eq!(map.drop_count(), 1, "loss must be counted once");
    assert_eq!(map.missing_fragment_count(), 1, "loss must be counted once");
}

#[test]
fn clearing_pending_preserves_counters_without_inventing_network_loss() {
    let mut map = ReassemblyMap::new(8, 16);
    assert!(map.ingest(fragment(1, 1, 0, 2, b"a")).unwrap().is_none());
    map.expire_incomplete_older_than(Instant::now(), Duration::ZERO);
    assert_eq!(map.drop_count(), 1);
    assert_eq!(map.missing_fragment_count(), 1);
    assert_eq!(map.resolved_fragment_count(), 2);

    assert!(map.ingest(fragment(1, 2, 0, 3, b"b")).unwrap().is_none());

    map.clear_pending();

    assert_eq!(map.drop_count(), 1);
    assert_eq!(map.missing_fragment_count(), 1);
    assert_eq!(map.resolved_fragment_count(), 2);
    assert!(map
        .ingest(fragment(1, 2, 1, 3, b"late-tail"))
        .unwrap()
        .is_none());
    assert!(map.ingest(fragment(1, 3, 0, 1, b"next")).unwrap().is_some());
    assert_eq!(map.missing_fragment_count(), 1);
    assert_eq!(map.resolved_fragment_count(), 3);
}

#[test]
fn clearing_pending_rejects_late_tails_after_terminal_cache_rotates() {
    let mut map = ReassemblyMap::new(1, 16);
    assert!(map
        .ingest(fragment(1, 1, 0, 2, b"old-head"))
        .unwrap()
        .is_none());
    map.clear_pending();

    // Rotate the single-frame map's two-entry terminal cache with newer,
    // fully resolved frames. The monotonic clear boundary must still keep
    // the abandoned frame terminal.
    assert!(map.ingest(fragment(1, 2, 0, 1, b"two")).unwrap().is_some());
    assert!(map
        .ingest(fragment(1, 3, 0, 1, b"three"))
        .unwrap()
        .is_some());
    assert!(map.ingest(fragment(1, 4, 0, 1, b"four")).unwrap().is_some());
    assert!(map
        .ingest(fragment(1, 1, 1, 2, b"late-tail"))
        .unwrap()
        .is_none());
    assert_eq!(map.drop_count(), 0);
    assert_eq!(map.missing_fragment_count(), 0);
}

#[test]
fn incomplete_delta_requires_refresh_unless_marked_discardable() {
    let mut map = ReassemblyMap::new(8, 16);
    assert!(map.ingest(fragment(1, 1, 0, 2, b"p0")).unwrap().is_none());
    map.expire_incomplete_older_than(Instant::now(), Duration::ZERO);
    assert!(map.take_reference_chain_loss());

    let mut discardable = fragment(1, 2, 0, 2, b"b0");
    discardable.flags = VideoPacketFlags::DISCARDABLE;
    assert!(map.ingest(discardable).unwrap().is_none());
    map.expire_incomplete_older_than(Instant::now(), Duration::ZERO);
    assert!(!map.take_reference_chain_loss());
}

#[test]
fn cross_access_unit_reordering_completes_both_frames_before_deadline() {
    let mut map = ReassemblyMap::new(8, 16);
    let old_head = VideoPacket {
        flags: VideoPacketFlags::KEYFRAME | VideoPacketFlags::START_OF_ACCESS_UNIT,
        stream_epoch: 1,
        frame_id: 1,
        pts_us: 1,
        encoded_at_us: 1,
        fragment_index: 0,
        fragment_count: 2,
        payload: Bytes::copy_from_slice(b"k0"),
    };
    assert!(map.ingest(old_head).unwrap().is_none());

    let mut newer = fragment(1, 2, 0, 1, b"p");
    newer.flags = VideoPacketFlags::START_OF_ACCESS_UNIT | VideoPacketFlags::END_OF_ACCESS_UNIT;
    assert_eq!(
        map.ingest(newer)
            .unwrap()
            .as_ref()
            .map(|au| au.data.as_ref()),
        Some(&b"p"[..])
    );

    let mut old_tail = fragment(1, 1, 1, 2, b"k1");
    old_tail.flags = VideoPacketFlags::KEYFRAME | VideoPacketFlags::END_OF_ACCESS_UNIT;
    old_tail.pts_us = 1;
    old_tail.encoded_at_us = 1;
    assert_eq!(
        map.ingest(old_tail)
            .unwrap()
            .as_ref()
            .map(|au| au.data.as_ref()),
        Some(&b"k0k1"[..])
    );
    assert!(!map.take_keyframe_loss());
}

#[test]
fn inconsistent_fragment_timeline_discards_the_complete_access_unit() {
    let mut map = ReassemblyMap::new(8, 16);
    let mut head = fragment(1, 7, 0, 2, b"a");
    head.pts_us = 10;
    head.encoded_at_us = 20;
    assert!(map.ingest(head).expect("head").is_none());

    let mut tail = fragment(1, 7, 1, 2, b"b");
    tail.pts_us = 10;
    tail.encoded_at_us = 21;
    assert!(matches!(
        map.ingest(tail),
        Err(ReassemblyError::InconsistentFrameMetadata)
    ));
    assert!(map.take_reference_chain_loss());
}

#[test]
fn capacity_evicts_oldest_non_keyframe_before_keyframe() {
    let mut map = ReassemblyMap::new(2, 16);
    let mut key_head = fragment(1, 1, 0, 2, b"k0");
    key_head.flags = VideoPacketFlags::KEYFRAME;
    assert!(map.ingest(key_head).unwrap().is_none());
    assert!(map.ingest(fragment(1, 2, 0, 2, b"p0")).unwrap().is_none());
    assert!(map.ingest(fragment(1, 3, 0, 1, b"new")).unwrap().is_some());
    assert!(!map.take_keyframe_loss());
    assert!(map.ingest(fragment(1, 2, 1, 2, b"p1")).unwrap().is_none());
    assert_eq!(map.drop_count(), 1, "evicted frame must stay terminal");

    let mut key_tail = fragment(1, 1, 1, 2, b"k1");
    key_tail.flags = VideoPacketFlags::KEYFRAME;
    assert_eq!(
        map.ingest(key_tail)
            .unwrap()
            .as_ref()
            .map(|au| au.data.as_ref()),
        Some(&b"k0k1"[..])
    );
}

#[test]
fn oversized_keyframe_is_counted_and_requests_idr_once() {
    let mut map = ReassemblyMap::new(2, 2);
    let mut first = fragment(1, 1, 0, 3, b"k0");
    first.flags = VideoPacketFlags::KEYFRAME | VideoPacketFlags::START_OF_ACCESS_UNIT;
    assert_eq!(map.ingest(first), Err(ReassemblyError::TooManyFragments));
    assert_eq!(map.drop_count(), 1);
    assert!(map.take_keyframe_loss());

    let mut second = fragment(1, 1, 1, 3, b"k1");
    second.flags = VideoPacketFlags::KEYFRAME;
    assert_eq!(map.ingest(second), Err(ReassemblyError::TooManyFragments));
    assert_eq!(map.drop_count(), 1);
    assert!(!map.take_keyframe_loss());
}

#[test]
fn late_duplicate_cannot_recreate_a_completed_frame() {
    let mut map = ReassemblyMap::new(2, 2);
    assert!(map.ingest(fragment(1, 1, 0, 1, b"done")).unwrap().is_some());
    assert!(map.ingest(fragment(1, 1, 0, 1, b"late")).unwrap().is_none());
    assert_eq!(map.drop_count(), 0);
}

#[test]
fn explicit_ingest_time_drives_gap_and_fragment_deadlines() {
    let origin = Instant::now();
    let mut map = ReassemblyMap::new(8, 16);
    assert!(map
        .ingest_at(fragment(1, 1, 0, 1, b"one"), origin)
        .unwrap()
        .is_some());
    assert!(map
        .ingest_at(
            fragment(1, 3, 0, 1, b"three"),
            origin + Duration::from_millis(5),
        )
        .unwrap()
        .is_some());

    map.expire_incomplete_older_than(
        origin + Duration::from_millis(14),
        Duration::from_millis(10),
    );
    assert_eq!(map.whole_access_unit_gap_drop_count(), 0);
    map.expire_incomplete_older_than(
        origin + Duration::from_millis(15),
        Duration::from_millis(10),
    );
    assert_eq!(map.whole_access_unit_gap_drop_count(), 1);
}
