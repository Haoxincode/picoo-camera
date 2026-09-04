//! Sender→receiver loopback helpers for desktop diagnostics and tests.

use std::time::Duration;

use bytes::Bytes;
use picoo_session::ReceiverStatus;

use super::ReceiverSession;
use crate::{ReceiverError, ReceiverIdentity};

/// Run sender→receiver loopback until one access unit reaches FrameHub.
///
/// Uses the unpaired test bypass — prefer [`run_paired_loopback_access_unit`] for
/// product-path validation (REQ-PICOO-PAIRING-003).
pub fn run_loopback_access_unit(payload: &[u8]) -> Result<Bytes, ReceiverError> {
    use picoo_sender::SenderSession;
    use picoo_transport::{Endpoint, QuicSenderTransport};

    let mut receiver = ReceiverSession::new();
    receiver.decoder = Box::new(picoo_media_decode::StubDecoder::new());
    receiver.set_jitter_target_ms(0);
    receiver.set_permit_unpaired_video(true);
    let bind = receiver.listen(Endpoint {
        host: "127.0.0.1".into(),
        port: 0,
    })?;

    let mut sender = SenderSession::new(QuicSenderTransport::new());
    let endpoint = Endpoint {
        host: bind.ip().to_string(),
        port: bind.port(),
    };
    sender.connect(endpoint)?;

    for _ in 0..500 {
        receiver.pump()?;
        sender.pump()?;
        if sender.is_connected() && receiver.is_connected() {
            break;
        }
        std::thread::sleep(Duration::from_millis(2));
    }

    if !receiver.is_connected() {
        return Err(ReceiverError::LoopbackTimeout);
    }

    // This helper intentionally exercises the receiver's explicit unpaired test bypass.
    // Production senders never enter Streaming before pairing has committed.
    sender.ingest_and_flush_unchecked_for_test(payload, true, 1, 1)?;

    for _ in 0..200 {
        receiver.pump()?;
        sender.pump().ok();
        if let Some(frame) = receiver.latest_frame() {
            return Ok(frame.pixel_data.clone());
        }
        std::thread::sleep(Duration::from_millis(2));
    }

    Err(ReceiverError::LoopbackTimeout)
}

/// Pairing/session loopback: first-time pairing (short code) then video → FrameHub.
///
/// This explicitly uses `StubDecoder` for arbitrary fixture bytes. It validates
/// the paired transport/session path, not a platform's production H.264 decoder.
/// Does **not** use `permit_unpaired_video` (REQ-PICOO-PAIRING-003).
pub fn run_paired_loopback_access_unit(payload: &[u8]) -> Result<Bytes, ReceiverError> {
    use picoo_sender::SenderSession;
    use picoo_session::SenderStatus;
    use picoo_transport::{Endpoint, QuicSenderTransport};

    let identity = ReceiverIdentity::default();
    let mut receiver = ReceiverSession::new().with_identity(identity.clone());
    receiver.decoder = Box::new(picoo_media_decode::StubDecoder::new());
    receiver.set_jitter_target_ms(0);
    let bind = receiver.listen(Endpoint {
        host: "127.0.0.1".into(),
        port: 0,
    })?;

    let mut sender = SenderSession::new(QuicSenderTransport::new());
    sender.connect(Endpoint {
        host: bind.ip().to_string(),
        port: bind.port(),
    })?;

    for _ in 0..500 {
        receiver.pump()?;
        sender.pump()?;
        if sender.is_connected() && receiver.is_connected() {
            break;
        }
        std::thread::sleep(Duration::from_millis(2));
    }
    if !receiver.is_connected() {
        return Err(ReceiverError::LoopbackTimeout);
    }

    sender.send_client_hello()?;

    for _ in 0..200 {
        receiver.pump()?;
        sender.pump()?;
        if receiver.pairing_short_code().is_some() && sender.pairing_short_code().is_some() {
            break;
        }
        std::thread::sleep(Duration::from_millis(2));
    }
    if receiver.pairing_short_code().is_none() {
        return Err(ReceiverError::LoopbackTimeout);
    }

    receiver.confirm_pairing_locally()?;
    sender.send_pairing_confirm(identity.receiver_id())?;

    for _ in 0..200 {
        receiver.pump()?;
        sender.pump()?;
        if receiver.status() == ReceiverStatus::Streaming
            && sender.status() == SenderStatus::Streaming
        {
            break;
        }
        std::thread::sleep(Duration::from_millis(2));
    }
    if receiver.status() != ReceiverStatus::Streaming || sender.status() != SenderStatus::Streaming
    {
        return Err(ReceiverError::LoopbackTimeout);
    }

    sender.ingest_and_flush(payload, true, 1, 1)?;

    for _ in 0..200 {
        receiver.pump()?;
        sender.pump().ok();
        if let Some(frame) = receiver.latest_frame() {
            return Ok(frame.pixel_data.clone());
        }
        std::thread::sleep(Duration::from_millis(2));
    }

    Err(ReceiverError::LoopbackTimeout)
}
