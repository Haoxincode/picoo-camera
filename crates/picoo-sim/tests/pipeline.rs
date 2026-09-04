use std::time::Duration;

use picoo_protocol::control::{
    control_envelope::Payload as ControlPayload, StartStream, StreamConfig,
};
use picoo_sender::FecProtection;
use picoo_sim::{
    CameraFrame, DatagramSelector, EncoderFailure, NetworkScript, SimError, SimHarness,
};

const WIDTH: u32 = 16;
const HEIGHT: u32 = 16;

fn frame(marker: u8, keyframe: bool, pts_us: u64, epoch: u32, generation: u64) -> CameraFrame {
    CameraFrame::synthetic(marker, 2_500, keyframe, pts_us)
        .for_encoder(epoch, generation, WIDTH, HEIGHT)
}

fn config(epoch: u32) -> StreamConfig {
    StreamConfig {
        codec: "h264".into(),
        profile: "baseline".into(),
        level: "3.1".into(),
        width: WIDTH,
        height: HEIGHT,
        fps: 30,
        bitrate: 1_000_000,
        rotation: 0,
        mirrored: false,
        color_range: "limited".into(),
        sps: vec![1],
        pps: vec![2],
        stream_epoch: epoch,
    }
}

fn authenticated_stream(script: NetworkScript) -> SimHarness {
    let mut sim = SimHarness::new(script);
    sim.connect(1);
    sim.authenticate();
    sim.queue_start_stream();
    sim.advance(Duration::from_millis(2));
    sim.queue_stream_config(1, WIDTH, HEIGHT, Duration::ZERO);
    sim.advance(Duration::from_millis(2));
    sim
}

#[test]
fn stream_config_negotiates_live_state_and_enables_privileged_output_control() {
    let mut sim = SimHarness::new(NetworkScript::default());
    sim.connect(1);
    sim.authenticate();
    sim.queue_stream_config(1, WIDTH, HEIGHT, Duration::ZERO);
    sim.advance(Duration::from_millis(2));
    assert!(sim.snapshot().streaming);
    assert!(sim.issue_camera_command());
    assert_eq!(sim.snapshot().counters.privileged_controls, 1);
}

#[test]
fn unauthenticated_media_and_privileged_control_never_cross_the_gate() {
    let mut sim = SimHarness::new(NetworkScript::default());
    sim.connect(1);
    assert!(!sim.issue_camera_command());
    sim.queue_start_stream();
    sim.submit_camera_frame(frame(1, true, 1_000, 1, 1), FecProtection::None)
        .expect("packetize untrusted media");
    sim.advance(Duration::from_millis(2));

    let state = sim.snapshot();
    assert!(!state.authenticated);
    assert!(!state.streaming);
    assert_eq!(state.latest_sequence, 0);
    assert_eq!(state.counters.privileged_controls, 0);
    assert!(state.counters.illegal_control_drops >= 1);
    assert!(state.counters.unauthenticated_media_drops > 0);
}

#[test]
fn reordered_duplicated_fec_media_decodes_each_access_unit_once() {
    let script = NetworkScript {
        reverse_each_access_unit: true,
        duplicate_every: Some(2),
        ..Default::default()
    };
    let mut sim = authenticated_stream(script);
    sim.submit_camera_frame(frame(7, true, 10_000, 1, 1), FecProtection::Strong)
        .expect("send IDR");
    sim.advance(Duration::from_millis(3));

    let state = sim.snapshot();
    assert_eq!(state.counters.decoded, 1);
    assert_eq!(state.counters.duplicate_decode_attempts, 0);
    assert_eq!(state.latest_sequence, 1);
    assert_eq!(sim.consume_preview_latest().unwrap().pixel_data[0], 7);
}

#[test]
fn incomplete_idr_breaks_reference_chain_until_a_complete_idr() {
    let mut sim = authenticated_stream(NetworkScript::default());
    sim.network_mut()
        .script_mut()
        .drop_datagrams
        .insert(DatagramSelector {
            stream_epoch: 1,
            frame_id: 1,
            fragment_index: 1,
            fec_parity: false,
        });
    sim.submit_camera_frame(frame(1, true, 10_000, 1, 1), FecProtection::None)
        .unwrap();
    sim.advance(Duration::from_millis(3));
    sim.expire_reassembly(Duration::ZERO);
    assert!(sim.snapshot().waiting_for_idr);

    sim.network_mut().script_mut().drop_datagrams.clear();
    sim.submit_camera_frame(frame(2, false, 20_000, 1, 1), FecProtection::None)
        .unwrap();
    sim.advance(Duration::from_millis(3));
    assert_eq!(sim.snapshot().counters.decoded, 0);

    sim.submit_camera_frame(frame(3, true, 30_000, 1, 1), FecProtection::None)
        .unwrap();
    sim.advance(Duration::from_millis(3));
    let state = sim.snapshot();
    assert_eq!(state.counters.decoded, 1);
    assert!(state.reference_chain_intact);
    assert!(!state.waiting_for_idr);
}

#[test]
fn newer_reference_au_waits_for_an_older_incomplete_au_outcome() {
    let mut sim = authenticated_stream(NetworkScript::default());
    sim.submit_camera_frame(frame(1, true, 10_000, 1, 1), FecProtection::None)
        .unwrap();
    sim.advance(Duration::from_millis(3));
    sim.network_mut()
        .script_mut()
        .drop_datagrams
        .insert(DatagramSelector {
            stream_epoch: 1,
            frame_id: 2,
            fragment_index: 1,
            fec_parity: false,
        });
    sim.submit_camera_frame(frame(2, false, 20_000, 1, 1), FecProtection::None)
        .unwrap();
    sim.submit_camera_frame(frame(3, false, 30_000, 1, 1), FecProtection::None)
        .unwrap();
    sim.advance(Duration::from_millis(3));
    let blocked = sim.snapshot();
    assert_eq!(blocked.counters.decoded, 1);
    assert_eq!(blocked.completed_access_unit_depth, 1);

    sim.expire_reassembly(Duration::ZERO);
    let expired = sim.snapshot();
    assert_eq!(expired.counters.decoded, 1);
    assert_eq!(expired.completed_access_unit_depth, 0);
    assert!(expired.waiting_for_idr);

    sim.network_mut().script_mut().drop_datagrams.clear();
    sim.submit_camera_frame(frame(4, true, 40_000, 1, 1), FecProtection::None)
        .unwrap();
    sim.advance(Duration::from_millis(3));
    assert_eq!(sim.snapshot().counters.decoded, 2);
}

#[test]
fn future_idr_waits_for_late_stream_config_and_then_releases() {
    let mut sim = SimHarness::new(NetworkScript::default());
    sim.connect(1);
    sim.authenticate();
    sim.queue_start_stream();
    sim.advance(Duration::from_millis(2));

    assert!(sim.begin_encoder_reconfiguration(2, 2, 2, WIDTH, HEIGHT));
    assert!(sim.report_encoder_started(2, 2));
    sim.queue_stream_config(2, WIDTH, HEIGHT, Duration::from_millis(10));
    sim.submit_camera_frame(frame(8, true, 20_000, 2, 2), FecProtection::None)
        .unwrap();
    assert_eq!(sim.take_encoder_commit().unwrap().stream_epoch, 2);

    sim.advance(Duration::from_millis(2));
    assert_eq!(sim.snapshot().latest_sequence, 0);
    sim.advance(Duration::from_millis(10));
    assert_eq!(sim.snapshot().latest_sequence, 1);
    assert_eq!(sim.snapshot().configured_stream_epoch, Some(2));
}

#[test]
fn encoder_transactions_commit_one_generation_and_rollback_failures() {
    let mut sim = authenticated_stream(NetworkScript::default());
    assert!(sim.begin_encoder_reconfiguration(20, 2, 9, WIDTH, HEIGHT));
    assert!(sim.report_encoder_started(20, 9));
    assert_eq!(
        sim.submit_camera_frame(frame(2, false, 20_000, 2, 9), FecProtection::None),
        Err(SimError::EncoderRefreshPending)
    );
    sim.submit_camera_frame(frame(2, true, 21_000, 2, 9), FecProtection::None)
        .unwrap();
    let commit = sim.take_encoder_commit().expect("epoch 2 commit");
    assert_eq!((commit.stream_epoch, commit.encoder_generation), (2, 9));

    assert!(sim.begin_encoder_reconfiguration(21, 3, 10, WIDTH, HEIGHT));
    assert_eq!(sim.report_encoder_failed(21, 0), EncoderFailure::RolledBack);
    let state = sim.snapshot();
    assert_eq!(
        (
            state.committed_encoder_epoch,
            state.committed_encoder_generation
        ),
        (2, 9)
    );

    assert!(sim.begin_encoder_reconfiguration(22, 3, 11, WIDTH, HEIGHT));
    assert!(sim.report_encoder_started(22, 11));
    assert_eq!(
        sim.report_encoder_failed(22, 11),
        EncoderFailure::RecoveryRequired
    );
    let state = sim.snapshot();
    assert_eq!(
        (
            state.committed_encoder_epoch,
            state.committed_encoder_generation
        ),
        (2, 9)
    );
}

#[test]
fn consecutive_stream_epochs_publish_only_their_matching_idr_generation() {
    let mut sim = authenticated_stream(NetworkScript::default());
    for (transaction, epoch, generation, marker) in [(30, 2, 20, 2), (31, 3, 21, 3)] {
        assert!(sim.begin_encoder_reconfiguration(transaction, epoch, generation, WIDTH, HEIGHT,));
        assert!(sim.report_encoder_started(transaction, generation));
        sim.queue_stream_config(epoch, WIDTH, HEIGHT, Duration::ZERO);
        sim.advance(Duration::from_millis(2));
        sim.submit_camera_frame(
            frame(marker, true, u64::from(epoch) * 10_000, epoch, generation),
            FecProtection::None,
        )
        .unwrap();
        sim.advance(Duration::from_millis(2));
        let latest = sim.consume_preview_latest().expect("generation IDR");
        assert_eq!(latest.stream_generation, u64::from(epoch));
        assert_eq!(latest.pixel_data[0], marker);
    }
    let state = sim.snapshot();
    assert_eq!(state.committed_encoder_epoch, 3);
    assert_eq!(state.committed_encoder_generation, 21);
    assert_eq!(state.counters.decoded, 2);
}

#[test]
fn camera_backgrounding_does_not_create_media_or_unbounded_work() {
    let mut sim = authenticated_stream(NetworkScript::default());
    sim.suspend_camera();
    for index in 0..100 {
        assert!(!sim
            .submit_camera_frame(
                frame(index as u8, index == 0, index * 1_000, 1, 1),
                FecProtection::None,
            )
            .unwrap());
    }
    assert_eq!(sim.snapshot().network_in_flight, 0);
    sim.resume_camera();
    sim.submit_camera_frame(frame(9, true, 200_000, 1, 1), FecProtection::None)
        .unwrap();
    sim.advance(Duration::from_millis(2));
    let state = sim.snapshot();
    assert_eq!(state.counters.camera_suspended_drops, 100);
    assert_eq!(state.counters.decoded, 1);
}

#[test]
fn stale_connection_events_cannot_mutate_a_fast_reconnect() {
    let mut sim = SimHarness::new(NetworkScript::default());
    sim.connect(1);
    sim.authenticate();
    sim.queue_control_with_identity(
        ControlPayload::StartStream(StartStream {}),
        1,
        1,
        Duration::from_millis(10),
        false,
    );
    sim.disconnect(1);
    sim.connect(2);
    sim.authenticate();
    sim.queue_start_stream();
    sim.advance(Duration::from_millis(2));
    assert!(sim.snapshot().streaming);
    sim.advance(Duration::from_millis(10));
    let state = sim.snapshot();
    assert_eq!(state.connection_generation, Some(2));
    assert!(state.streaming);
    assert!(state.counters.stale_generation_drops >= 2);
}

#[test]
fn duplicate_out_of_order_and_illegal_phase_controls_are_rejected() {
    let mut sim = SimHarness::new(NetworkScript::default());
    sim.connect(1);
    sim.authenticate();
    sim.queue_control_with_identity(
        ControlPayload::StartStream(StartStream {}),
        2,
        1,
        Duration::ZERO,
        true,
    );
    sim.queue_control_with_identity(
        ControlPayload::StreamConfig(config(1)),
        1,
        1,
        Duration::from_millis(2),
        false,
    );
    sim.advance(Duration::from_millis(5));
    let state = sim.snapshot();
    assert!(state.streaming);
    assert_eq!(state.configured_stream_epoch, None);
    assert!(state.counters.replayed_control_drops >= 2);

    sim.queue_control_with_identity(
        ControlPayload::StartStream(StartStream {}),
        3,
        1,
        Duration::ZERO,
        false,
    );
    sim.advance(Duration::from_millis(2));
    assert!(sim.snapshot().counters.illegal_control_drops >= 1);
}

#[test]
fn slow_preview_and_fast_vcam_are_latest_only_and_never_backpressure_decode() {
    let mut sim = authenticated_stream(NetworkScript::default());
    for index in 0..120_u64 {
        sim.submit_camera_frame(
            frame(index as u8, index == 0, 10_000 + index * 34_000, 1, 1),
            FecProtection::None,
        )
        .unwrap();
        sim.advance(Duration::from_millis(2));
        for _ in 0..3 {
            let (_, bytes) = sim.consume_virtual_camera(1280, 720);
            assert_eq!(bytes, 1280 * 720 * 3 / 2);
        }
    }
    let state = sim.snapshot();
    assert_eq!(state.counters.decoded, 120);
    assert_eq!(state.latest_sequence, 120);
    assert_eq!(state.jitter_depth, 0);
    assert_eq!(state.network_in_flight, 0);
    assert_eq!(sim.consume_preview_latest().unwrap().sequence, 120);
}

#[test]
fn fixed_seed_network_damage_is_reproducible_and_bounded() {
    fn run(seed: u64) -> (u64, u64, usize) {
        let mut sim = authenticated_stream(NetworkScript {
            loss_basis_points: 500,
            seed,
            jitter_us: 4_000,
            duplicate_every: Some(11),
            max_in_flight: 64,
            ..Default::default()
        });
        for index in 0..200_u64 {
            sim.submit_camera_frame(
                frame(index as u8, index % 30 == 0, 10_000 + index * 34_000, 1, 1),
                FecProtection::Strong,
            )
            .unwrap();
            sim.advance(Duration::from_millis(6));
            sim.expire_reassembly(Duration::from_millis(20));
        }
        sim.advance(Duration::from_millis(100));
        let state = sim.snapshot();
        (
            sim.network_mut().dropped(),
            state.counters.decoded,
            state.network_in_flight,
        )
    }

    let first = run(0x1234_5678);
    let second = run(0x1234_5678);
    assert_eq!(first, second);
    assert!(first.0 > 0);
    assert_eq!(first.2, 0);
}

#[test]
fn burst_loss_drops_an_access_unit_atomically() {
    let mut sim = authenticated_stream(NetworkScript {
        burst_drop: Some((1, 3)),
        ..Default::default()
    });
    sim.submit_camera_frame(frame(1, true, 10_000, 1, 1), FecProtection::None)
        .unwrap();
    sim.advance(Duration::from_millis(3));
    assert_eq!(sim.snapshot().counters.decoded, 0);

    sim.submit_camera_frame(frame(2, true, 20_000, 1, 1), FecProtection::None)
        .unwrap();
    sim.advance(Duration::from_millis(3));
    assert_eq!(sim.snapshot().counters.decoded, 1);
}

#[test]
fn bounded_media_saturation_cannot_starve_reliable_control() {
    let mut sim = SimHarness::new(NetworkScript {
        base_delay_us: 5_000,
        max_in_flight: 2,
        ..Default::default()
    });
    sim.connect(1);
    sim.authenticate();
    sim.submit_camera_frame(frame(1, true, 10_000, 1, 1), FecProtection::None)
        .unwrap();
    sim.queue_start_stream();
    assert_eq!(sim.snapshot().network_in_flight, 2);
    sim.advance(Duration::from_millis(6));
    assert!(sim.snapshot().streaming);
    assert!(sim.network_mut().overflow_dropped() > 0);
}

#[test]
fn receiver_restart_discards_old_generation_and_starts_from_new_idr() {
    let mut sim = authenticated_stream(NetworkScript {
        base_delay_us: 20_000,
        ..Default::default()
    });
    sim.submit_camera_frame(frame(1, true, 10_000, 1, 1), FecProtection::None)
        .unwrap();
    sim.restart_receiver(2);
    sim.authenticate();
    sim.queue_start_stream();
    sim.advance(Duration::from_millis(21));
    sim.queue_stream_config(1, WIDTH, HEIGHT, Duration::ZERO);
    sim.advance(Duration::from_millis(21));
    assert_eq!(sim.snapshot().latest_sequence, 0);
    sim.submit_camera_frame(frame(2, true, 50_000, 1, 1), FecProtection::None)
        .unwrap();
    sim.advance(Duration::from_millis(21));
    let state = sim.snapshot();
    assert_eq!(state.latest_sequence, 1);
    assert!(state.counters.stale_generation_drops > 0);
}
