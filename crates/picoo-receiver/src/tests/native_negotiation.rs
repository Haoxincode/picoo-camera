//! Actual probe -> PCP offers -> native encoder event -> QUIC -> VideoToolbox.
//! REQ-PICOO-MEDIA-053; synthetic source pixels, no camera content retained.
use super::*;
use picoo_bitstream::{Codec, CodecConfiguration};
use picoo_sender::{NativeEncoderEvent, StreamConfigParams};
use picoo_transport::Endpoint;

#[test]
fn native_offers_admit_apple_and_xiaomi_complete_formats_over_quic() {
    macro_rules! probe {
        ($family:literal, $stem:literal, $codec:ident, $height:expr, $fps:expr) => {
            run_format(
                Codec::$codec,
                $height,
                $fps,
                include_bytes!(concat!(
                    "../../../picoo-media-decode/probes/",
                    $family,
                    "/",
                    $stem,
                    ".config"
                )),
                include_bytes!(concat!(
                    "../../../picoo-media-decode/probes/",
                    $family,
                    "/",
                    $stem,
                    ".au"
                )),
            )
        };
    }
    macro_rules! family {
        ($family:literal) => {
            probe!($family, "1-720-30", Avc, 720, 30);
            probe!($family, "1-720-60", Avc, 720, 60);
            probe!($family, "1-1080-30", Avc, 1080, 30);
            probe!($family, "1-1080-60", Avc, 1080, 60);
            probe!($family, "2-720-30", Hevc, 720, 30);
            probe!($family, "2-720-60", Hevc, 720, 60);
            probe!($family, "2-1080-30", Hevc, 1080, 30);
            probe!($family, "2-1080-60", Hevc, 1080, 60);
        };
    }
    family!("apple-native-formats");
    family!("xiaomi-native-formats");
    family!("xiaomi-product-formats");
}

fn run_format(codec: Codec, height: u32, fps: u32, record: &'static [u8], au: &[u8]) {
    let width = if height == 720 { 1280 } else { 1920 };
    let config = StreamConfigParams {
        configuration: CodecConfiguration::parse(codec, bytes::Bytes::from_static(record))
            .unwrap()
            .into(),
        width,
        height,
        fps,
        bitrate_bps: 6_000_000,
        stream_epoch: 1,
        mirrored: false,
        rotation: 0,
    };
    let mut receiver = ReceiverSession::new();
    receiver.set_native_decoder_for_test();
    receiver.set_jitter_target_ms(0);
    let bound = receiver
        .listen(Endpoint {
            host: "127.0.0.1".into(),
            port: 0,
        })
        .unwrap();
    let mut sender = SenderSession::new(QuicSenderTransport::new());
    trust_receiver(&mut sender, &mut receiver);
    sender.set_stream_config(config.clone());
    sender
        .connect(Endpoint {
            host: bound.ip().to_string(),
            port: bound.port(),
        })
        .unwrap();
    sender.send_client_hello().unwrap();
    let deadline = Instant::now() + Duration::from_secs(10);
    while sender.receiver_capabilities().is_none() && Instant::now() < deadline {
        receiver.pump().unwrap();
        sender.pump().unwrap();
        std::thread::sleep(Duration::from_millis(2));
    }
    let actual = config.to_proto().unwrap().validated_video_format().unwrap();
    assert!(sender
        .receiver_capabilities()
        .expect("native worker offers over PCP")
        .supports(
            &actual,
            u32::from(config.configuration.level_idc()),
            au.len() as u32
        ));
    assert_eq!(
        receiver.ingress_stats().decode_invocations,
        0,
        "probe is not live media"
    );
    for sequence in 0..3 {
        let pts = 1 + sequence * 1_000_000 / u64::from(fps);
        let result = sender
            .submit_encoder_event(NativeEncoderEvent {
                data: au,
                is_keyframe: true,
                pts_us: pts,
                encoded_at_us: pts,
                encoder_generation: 1,
                stream_epoch: 1,
                width,
                height,
                stream_config: (sequence == 0).then(|| config.clone()),
            })
            .unwrap();
        assert!(result.encoder_accepted);
        pump_pair_for(&mut receiver, &mut sender, Duration::from_millis(80));
    }
    let deadline = Instant::now() + Duration::from_secs(3);
    while receiver.latest_frame().is_none() && Instant::now() < deadline {
        receiver.pump().unwrap();
        sender.pump().unwrap();
        std::thread::sleep(Duration::from_millis(2));
    }
    let frame = receiver.latest_frame().unwrap_or_else(|| {
        panic!(
            "{codec:?}/{height}/{fps}: {:?}",
            receiver.last_media_error()
        )
    });
    assert_eq!(source_dimensions(frame), (width, height));
    assert_eq!(
        receiver
            .stream_config()
            .unwrap()
            .validated_video_format()
            .unwrap(),
        actual
    );
    eprintln!("native PCP {codec:?}/{height}/{fps} passed");
}
