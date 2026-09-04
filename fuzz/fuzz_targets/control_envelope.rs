#![no_main]

#[path = "../support.rs"]
mod support;

use libfuzzer_sys::fuzz_target;
use picoo_protocol::control::control_envelope::Payload;
use picoo_protocol::{
    decode_control_envelope, encode_control_envelope, receiver_payload_allowed,
    ReceiverControlPhase,
};

fuzz_target!(|input: &[u8]| {
    let bytes = support::decode_seed(input);
    let Ok(envelope) = decode_control_envelope(&bytes) else {
        return;
    };
    let payload = envelope.payload.expect("validated envelope payload");
    let encoded = encode_control_envelope(
        payload.clone(),
        envelope.message_id,
        envelope.connection_generation,
    );
    let roundtrip = decode_control_envelope(&encoded).expect("valid envelope roundtrip");
    assert_eq!(roundtrip.message_id, envelope.message_id);
    assert_eq!(
        roundtrip.connection_generation,
        envelope.connection_generation
    );

    // An untrusted endpoint must never reach media/configuration/statistics or
    // receiver-originated privileged commands, regardless of protobuf shape.
    if matches!(
        payload,
        Payload::StreamConfig(_)
            | Payload::Capabilities(_)
            | Payload::SenderStats(_)
            | Payload::ReceiverStats(_)
            | Payload::CameraCommand(_)
            | Payload::EncoderCommand(_)
            | Payload::ClockSyncPing(_)
            | Payload::ClockSyncPong(_)
    ) {
        assert!(!receiver_payload_allowed(
            ReceiverControlPhase::AwaitingClientHello,
            &payload
        ));
        assert!(!receiver_payload_allowed(
            ReceiverControlPhase::Pairing,
            &payload
        ));
    }
});
