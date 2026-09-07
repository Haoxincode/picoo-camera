#[cfg(all(not(windows), not(target_vendor = "apple")))]
use std::time::Duration;

#[cfg(all(not(windows), not(target_vendor = "apple")))]
use picoo_sender::SenderSession;

#[cfg(all(not(windows), not(target_vendor = "apple")))]
use crate::ReceiverSession;

#[cfg(all(not(windows), not(target_vendor = "apple")))]
#[test]
fn network_feedback_keeps_source_fixed_and_explicit_changes_still_decode() {
    // REQ-PICOO-MEDIA-027: network feedback cannot replace source configuration.
    use openh264::encoder::Encoder;
    use openh264::formats::YUVBuffer;
    use picoo_bitstream::avc::extract_sps_pps;
    use picoo_frame_hub::nv12_byte_size;
    use picoo_protocol::control::ReceiverStats as ReceiverStatsMsg;
    use picoo_sender::StreamConfigParams;
    use picoo_session::ReceiverStatus;
    use picoo_transport::{Endpoint, QuicSenderTransport};

    fn encode_pattern(w: usize, h: usize, seed: u8) -> (Vec<u8>, Vec<u8>, Vec<u8>) {
        let mut planes = vec![128u8; w * h * 3 / 2];
        planes[..w * h].fill(seed.saturating_add(32));
        let yuv = YUVBuffer::from_vec(planes, w, h);
        let mut encoder = Encoder::with_api_config(
            openh264::OpenH264API::from_source(),
            openh264::encoder::EncoderConfig::new().profile(openh264::encoder::Profile::High),
        )
        .expect("encoder");
        let annex = encoder.encode(&yuv).expect("encode").to_vec();
        let (sps, pps) = extract_sps_pps(&annex).expect("sps/pps");
        (super::wire_avc(&annex), sps, pps)
    }

    let (au_hi, sps_hi, pps_hi) = encode_pattern(1920, 1080, 1);
    let (au_lo, sps_lo, pps_lo) = encode_pattern(1280, 720, 9);

    let mut receiver = ReceiverSession::new();
    receiver.set_jitter_target_ms(0);
    let bind = receiver
        .listen(Endpoint {
            host: "127.0.0.1".into(),
            port: 0,
        })
        .expect("listen");
    let mut sender = SenderSession::new(QuicSenderTransport::new());
    super::trust_receiver(&mut sender, &mut receiver);
    sender
        .connect(Endpoint {
            host: bind.ip().to_string(),
            port: bind.port(),
        })
        .expect("connect");
    for _ in 0..500 {
        receiver.pump().expect("rx");
        sender.pump().expect("tx");
        if sender.is_connected() && receiver.is_connected() {
            break;
        }
        std::thread::sleep(Duration::from_millis(2));
    }
    sender.send_client_hello().expect("hello");
    for _ in 0..200 {
        receiver.pump().expect("rx");
        sender.pump().expect("tx");
        if receiver.status() == ReceiverStatus::Streaming {
            break;
        }
        std::thread::sleep(Duration::from_millis(2));
    }
    assert_eq!(receiver.status(), ReceiverStatus::Streaming);
    assert_eq!(sender.bitrate_active_height(), 1080);

    sender.set_stream_config(StreamConfigParams {
        width: 1920,
        height: 1080,
        fps: 30,
        bitrate_bps: 6_000_000,
        stream_epoch: 1,
        mirrored: false,
        rotation: 0,
        configuration: picoo_bitstream::CodecConfiguration::from_avc_parameter_sets(
            &sps_hi, &pps_hi,
        )
        .unwrap()
        .into(),
    });
    for _ in 0..40 {
        receiver.pump().ok();
        sender.pump().ok();
        if sender.stream_config_sent() {
            break;
        }
        std::thread::sleep(Duration::from_millis(2));
    }
    sender.ingest_and_flush(&au_hi, true, 1, 1).expect("hi");
    for _ in 0..200 {
        receiver.pump().ok();
        sender.pump().ok();
        if receiver.latest_frame().is_some_and(|f| f.width == 1920) {
            break;
        }
        std::thread::sleep(Duration::from_millis(2));
    }
    assert!(receiver.latest_frame().is_some_and(|f| f.width == 1920));

    let original_epoch = sender.current_stream_epoch();
    for _ in 0..1000 {
        sender.apply_receiver_stats_for_test(ReceiverStatsMsg {
            packet_loss: 0.2,
            frame_age_ms: 500.0,
            ..Default::default()
        });
        assert!(sender.pending_encoder_directive().is_none());
        assert_eq!(sender.current_stream_epoch(), original_epoch);
        assert_eq!(sender.bitrate_active_height(), 1080);
    }
    assert_eq!(receiver.stream_config().unwrap().height, 1080);
    assert_eq!(receiver.latest_frame().unwrap().width, 1920);

    // An explicit user request still owns a fresh configuration transaction.
    let epoch = sender.begin_stream_reconfiguration(720);
    assert!(epoch > original_epoch);
    let transaction_id = sender.encoder_transaction_id_for_epoch(epoch);
    let cfg_lo = StreamConfigParams {
        width: 1280,
        height: 720,
        fps: 30,
        bitrate_bps: 3_000_000,
        stream_epoch: epoch,
        mirrored: false,
        rotation: 0,
        configuration: picoo_bitstream::CodecConfiguration::from_avc_parameter_sets(
            &sps_lo, &pps_lo,
        )
        .unwrap()
        .into(),
    };
    sender.set_stream_config(cfg_lo.clone());
    assert!(sender.report_encoder_started(transaction_id, 2, epoch, 720,));
    sender
        .ingest_encoder_access_unit(super::native_au(
            &au_lo,
            true,
            2,
            (transaction_id, 2, epoch, 720),
        ))
        .expect("commit 720p generation");
    sender.flush_pending().expect("send 720p IDR");
    assert_eq!(sender.bitrate_active_height(), 720);
    for _ in 0..80 {
        receiver.pump().ok();
        sender.pump().ok();
        if sender.stream_config_sent() && receiver.stream_config().is_some_and(|c| c.height == 720)
        {
            break;
        }
        std::thread::sleep(Duration::from_millis(2));
    }
    assert_eq!(
        receiver.stream_config().map(|c| (c.width, c.height)),
        Some((1280, 720)),
        "StreamConfig must be 1280x720 after explicit configuration commit"
    );
    let mut ok = false;
    for _ in 0..400 {
        receiver.pump().ok();
        sender.pump().ok();
        if let Some(frame) = receiver.latest_frame() {
            if frame.width == 1280 && frame.height == 720 {
                assert_eq!(frame.pixel_data.len(), nv12_byte_size(1280, 720));
                ok = true;
                break;
            }
        }
        std::thread::sleep(Duration::from_millis(2));
    }
    assert!(ok, "LatestFrameStore must show explicitly requested frames");

    for _ in 0..1000 {
        sender.apply_receiver_stats_for_test(ReceiverStatsMsg::default());
        assert!(sender.pending_encoder_directive().is_none());
        assert_eq!(sender.current_stream_epoch(), epoch);
        assert_eq!(sender.bitrate_active_height(), 720);
    }
}
