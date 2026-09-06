use std::time::{Duration, Instant};

use picoo_sender::{SenderError, SenderSession};
use picoo_transport::{PicooTransport, QuicSenderTransport, TransportError};

use crate::ReceiverSession;

fn native_au(
    data: &[u8],
    is_keyframe: bool,
    pts_us: u64,
    facts: (u64, u64, u32, u32),
) -> picoo_sender::NativeEncoderAccessUnit<'_> {
    picoo_sender::NativeEncoderAccessUnit {
        data,
        is_keyframe,
        pts_us,
        encoded_at_us: pts_us,
        transaction_id: facts.0,
        encoder_generation: facts.1,
        stream_epoch: facts.2,
        height: facts.3,
    }
}

fn trust_receiver<T: PicooTransport>(
    sender: &mut picoo_sender::SenderSession<T>,
    receiver: &mut ReceiverSession,
) {
    let identity = receiver.identity();
    sender
        .trusted_devices_mut()
        .upsert(picoo_pairing::TrustedDevice {
            device_id: identity.receiver_id().to_owned(),
            device_name: identity.display_name().to_owned(),
            public_key: identity.public_key().to_vec(),
            certificate_fingerprint: "test-receiver".into(),
            paired_at_ms: 1,
            last_connected_at_ms: None,
        });
    let sender_identity = sender.identity();
    receiver
        .trusted_devices_mut()
        .upsert(picoo_pairing::trusted_device_from_pairing(
            sender_identity.device_id(),
            sender_identity.device_name(),
            sender_identity.public_key(),
            1,
        ));
}

mod abr_epoch;
mod connect;
mod control_gate;
mod decode_platform;
mod decoder;
mod pairing;
mod pairing_trust;
mod qos;
mod session_surface;
mod source_configuration;
mod stream;

fn use_stub_decoder(receiver: &mut ReceiverSession) {
    receiver.set_decoder_for_test(Box::new(picoo_media_decode::StubDecoder::new()));
}

/// Bounded video-queue pressure is an expected lossy-media outcome, especially
/// while parallel tests share the two-thread QUIC runtime. Tests that send a
/// sustained stream should continue pumping and let their end-state assertions
/// detect a real stall.
fn video_send_accepted(result: Result<usize, SenderError>) -> bool {
    match result {
        Ok(_) => true,
        Err(SenderError::Transport(TransportError::VideoBackpressure)) => false,
        Err(error) => panic!("unexpected video send failure: {error}"),
    }
}

fn pump_pair_for(
    receiver: &mut ReceiverSession,
    sender: &mut SenderSession<QuicSenderTransport>,
    duration: Duration,
) {
    let deadline = Instant::now() + duration;
    while Instant::now() < deadline {
        receiver.pump().expect("rx pump");
        sender.pump().expect("tx pump");
        std::thread::sleep(Duration::from_millis(2));
    }
}

/// Source allocation geometry, before output presentation transforms on Mac.
fn source_dimensions(frame: &crate::ReceiverFrame) -> (u32, u32) {
    #[cfg(target_os = "macos")]
    {
        (frame.image().width(), frame.image().height())
    }
    #[cfg(not(target_os = "macos"))]
    {
        (frame.width, frame.height)
    }
}

fn source_frame_id(frame: &crate::ReceiverFrame) -> u64 {
    #[cfg(target_os = "macos")]
    {
        frame.identity().frame_id
    }
    #[cfg(not(target_os = "macos"))]
    {
        frame.frame_id
    }
}

fn configured_source() -> picoo_sender::StreamConfigParams {
    let (sps, pps) =
        picoo_bitstream::avc::extract_sps_pps(picoo_testkit::AVC_1280X720_BT709_IDR).unwrap();
    picoo_sender::StreamConfigParams {
        sps,
        pps,
        ..Default::default()
    }
}

pub(crate) fn wire_avc(annex: &[u8]) -> Vec<u8> {
    picoo_bitstream::canonical_access_unit(
        picoo_bitstream::Codec::Avc,
        picoo_bitstream::NalFormat::AnnexB,
        annex,
    )
    .unwrap()
    .into_owned()
}
