#![no_main]

use libfuzzer_sys::fuzz_target;
use picoo_pairing::{
    sign_transcript_phase, verify_transcript_phase, DeviceIdentity, PairingTranscript,
};

fn take<'a>(data: &'a [u8], cursor: &mut usize, requested: usize) -> &'a [u8] {
    let start = (*cursor).min(data.len());
    let end = start.saturating_add(requested).min(data.len());
    *cursor = end;
    &data[start..end]
}

fn exercise_valid(transcript: &PairingTranscript<'_>, phase: &[u8], signature_bytes: &[u8]) {
    let hash = transcript.hash().expect("valid transcript");
    let code = transcript.short_code().expect("valid transcript has SAS");
    assert_eq!(code.len(), 6);
    assert!(code.bytes().all(|byte| byte.is_ascii_digit()));

    let mut secret = [0_u8; 32];
    for (target, source) in secret.iter_mut().zip(signature_bytes.iter().copied()) {
        *target = source;
    }
    let identity = DeviceIdentity::from_secret_bytes("fuzz", &secret).expect("32-byte secret");
    let signature = sign_transcript_phase(&identity, &hash, phase);
    verify_transcript_phase(identity.public_key(), &hash, phase, &signature)
        .expect("fresh proof verifies");
    let _ = verify_transcript_phase(transcript.sender_public_key, &hash, phase, signature_bytes);
}

fuzz_target!(|data: &[u8]| {
    if let Some(phase) = data.strip_prefix(b"valid:") {
        let transcript = PairingTranscript {
            sender_id: "sender",
            sender_public_key: &[1; 32],
            sender_nonce: &[2; 32],
            receiver_id: "receiver",
            receiver_public_key: &[3; 32],
            receiver_nonce: &[4; 32],
            channel_binding: &[5; 32],
            connection_generation: 1,
        };
        exercise_valid(&transcript, phase, data);
        return;
    }

    let mut cursor: usize = 0;
    let length = |index: usize| usize::from(data.get(index).copied().unwrap_or(0) % 40);
    cursor = cursor.saturating_add(5).min(data.len());
    let sender_key = take(data, &mut cursor, length(0));
    let sender_nonce = take(data, &mut cursor, length(1));
    let receiver_key = take(data, &mut cursor, length(2));
    let receiver_nonce = take(data, &mut cursor, length(3));
    let binding = take(data, &mut cursor, length(4));
    let generation = data
        .get(cursor..cursor.saturating_add(8))
        .and_then(|bytes| bytes.try_into().ok())
        .map(u64::from_be_bytes)
        .unwrap_or(0);
    let sender_id = if data.first().is_some_and(|byte| byte & 1 == 0) {
        "sender"
    } else {
        ""
    };
    let receiver_id = if data.first().is_some_and(|byte| byte & 2 == 0) {
        "receiver"
    } else {
        ""
    };
    let transcript = PairingTranscript {
        sender_id,
        sender_public_key: sender_key,
        sender_nonce,
        receiver_id,
        receiver_public_key: receiver_key,
        receiver_nonce,
        channel_binding: binding,
        connection_generation: generation,
    };
    if transcript.hash().is_err() {
        assert!(transcript.short_code().is_err());
        return;
    }
    let phase_len = data.len().saturating_sub(cursor).min(64);
    let phase = take(data, &mut cursor, phase_len);
    exercise_valid(&transcript, phase, data);
});
