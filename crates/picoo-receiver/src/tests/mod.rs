use std::time::{Duration, Instant};

use picoo_sender::SenderSession;
use picoo_transport::QuicSenderTransport;

use crate::ReceiverSession;

mod abr_epoch;
mod abr_ladder;
mod connect;
mod control_gate;
mod decode_platform;
mod decoder;
mod pairing;
mod qos;
mod session_surface;
mod stream;

fn use_stub_decoder(receiver: &mut ReceiverSession) {
    receiver.set_decoder_for_test(Box::new(picoo_media_decode::StubDecoder::new()));
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
