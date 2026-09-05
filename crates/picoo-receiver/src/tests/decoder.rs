use std::time::Duration;

use picoo_sender::SenderSession;
use picoo_transport::{Endpoint, QuicSenderTransport};

use crate::{run_loopback_access_unit, run_paired_loopback_access_unit, ReceiverSession};

use super::use_stub_decoder;

#[test]
fn decoder_is_reset_at_every_session_teardown_boundary() {
    use std::sync::atomic::{AtomicUsize, Ordering};
    use std::sync::Arc;

    struct ResetCounter(Arc<AtomicUsize>);

    impl picoo_media_decode::AccessUnitDecoder for ResetCounter {
        fn decode_access_unit(
            &mut self,
            _access_unit: &[u8],
            _stream_config: Option<&picoo_protocol::control::StreamConfig>,
        ) -> Result<picoo_media_decode::DecodeOutcome, picoo_media_decode::DecodeError> {
            Ok(picoo_media_decode::DecodeOutcome::accepted_without_frame(
                false,
            ))
        }

        fn reset(&mut self) -> Result<(), picoo_media_decode::DecodeError> {
            self.0.fetch_add(1, Ordering::SeqCst);
            Ok(())
        }
    }

    let resets = Arc::new(AtomicUsize::new(0));
    let mut receiver = ReceiverSession::new();
    receiver.set_decoder_for_test(Box::new(ResetCounter(Arc::clone(&resets))));

    receiver
        .inject_peer_disconnect_for_test()
        .expect("peer disconnect reset");
    wait_for_atomic(&resets, 1);
    assert_eq!(resets.load(Ordering::SeqCst), 1);

    receiver
        .activate_connection_for_test(1)
        .expect("activate StopStream generation");
    receiver.set_permit_unpaired_video(true);
    receiver
        .handle_stop_stream(picoo_transport::SessionId(1))
        .expect("StopStream reset");
    wait_for_atomic(&resets, 2);
    assert_eq!(resets.load(Ordering::SeqCst), 2);

    receiver
        .activate_connection_for_test(2)
        .expect("activate close generation");
    receiver.close();
    wait_for_atomic(&resets, 3);
    assert_eq!(resets.load(Ordering::SeqCst), 3);
    receiver.close();
    std::thread::sleep(Duration::from_millis(20));
    assert_eq!(
        resets.load(Ordering::SeqCst),
        3,
        "repeated teardown must not execute decoder reset twice"
    );

    fn wait_for_atomic(counter: &AtomicUsize, expected: usize) {
        let deadline = std::time::Instant::now() + Duration::from_secs(1);
        while std::time::Instant::now() < deadline {
            if counter.load(Ordering::SeqCst) >= expected {
                return;
            }
            std::thread::sleep(Duration::from_millis(1));
        }
    }
}

#[test]
fn decoder_failure_is_reported_without_stopping_ingress_and_clears_after_recovery() {
    struct AlwaysFails;
    struct DropsRefresh;

    impl picoo_media_decode::AccessUnitDecoder for AlwaysFails {
        fn decode_access_unit(
            &mut self,
            _access_unit: &[u8],
            _stream_config: Option<&picoo_protocol::control::StreamConfig>,
        ) -> Result<picoo_media_decode::DecodeOutcome, picoo_media_decode::DecodeError> {
            Err(picoo_media_decode::DecodeError::Platform(
                "fixture failure".into(),
            ))
        }

        fn reset(&mut self) -> Result<(), picoo_media_decode::DecodeError> {
            Ok(())
        }
    }

    impl picoo_media_decode::AccessUnitDecoder for DropsRefresh {
        fn decode_access_unit(
            &mut self,
            _access_unit: &[u8],
            _stream_config: Option<&picoo_protocol::control::StreamConfig>,
        ) -> Result<picoo_media_decode::DecodeOutcome, picoo_media_decode::DecodeError> {
            Ok(picoo_media_decode::DecodeOutcome::accepted_without_frame(
                false,
            ))
        }

        fn reset(&mut self) -> Result<(), picoo_media_decode::DecodeError> {
            Ok(())
        }
    }

    let mut receiver = ReceiverSession::new();
    receiver.set_decoder_for_test(Box::new(AlwaysFails));
    receiver
        .publish_access_unit(bytes::Bytes::from_static(b"broken-au"), false)
        .expect("a media failure must not terminate the receiver pump");
    receiver.drain_decoder_until_idle_for_test();
    assert_eq!(receiver.ingress_stats().access_units, 1);
    assert_eq!(receiver.ingress_stats().decoded_frames, 0);
    assert_eq!(
        receiver.last_media_error(),
        Some("platform decoder: fixture failure")
    );
    assert!(receiver.awaiting_decoder_refresh_for_test());

    receiver
        .publish_access_unit(bytes::Bytes::from_static(b"blocked-delta"), false)
        .expect("delta is dropped while awaiting IDR");
    assert_eq!(receiver.ingress_stats().decode_invocations, 1);
    assert_eq!(receiver.ingress_stats().recovery_dropped_access_units, 1);

    receiver.set_decoder_for_test(Box::new(DropsRefresh));
    receiver
        .publish_access_unit(bytes::Bytes::from_static(b"dropped-idr"), true)
        .expect("a dropped IDR does not fail the session");
    receiver.drain_decoder_until_idle_for_test();
    assert!(
        receiver.awaiting_decoder_refresh_for_test(),
        "FrameDropped/Ok(None) must not reopen the delta gate"
    );

    use_stub_decoder(&mut receiver);
    receiver
        .publish_access_unit(bytes::Bytes::from_static(b"recovered-au"), true)
        .expect("decoder recovery");
    receiver.drain_decoder_until_idle_for_test();
    assert_eq!(receiver.ingress_stats().decoded_frames, 1);
    assert_eq!(receiver.last_media_error(), None);
    assert!(!receiver.awaiting_decoder_refresh_for_test());
}

#[test]
fn slow_decoder_never_blocks_session_close() {
    use std::sync::atomic::{AtomicBool, Ordering};
    use std::sync::Arc;

    struct SlowDecoder(Arc<AtomicBool>);

    impl picoo_media_decode::AccessUnitDecoder for SlowDecoder {
        fn decode_access_unit(
            &mut self,
            _access_unit: &[u8],
            _stream_config: Option<&picoo_protocol::control::StreamConfig>,
        ) -> Result<picoo_media_decode::DecodeOutcome, picoo_media_decode::DecodeError> {
            self.0.store(true, Ordering::Release);
            std::thread::sleep(Duration::from_millis(250));
            Ok(picoo_media_decode::DecodeOutcome::accepted_without_frame(
                false,
            ))
        }

        fn reset(&mut self) -> Result<(), picoo_media_decode::DecodeError> {
            Ok(())
        }
    }

    let started = Arc::new(AtomicBool::new(false));
    let mut receiver = ReceiverSession::new();
    receiver.set_decoder_for_test(Box::new(SlowDecoder(Arc::clone(&started))));
    receiver
        .publish_access_unit(bytes::Bytes::from_static(b"slow-au"), true)
        .expect("queue decode");
    let start_deadline = std::time::Instant::now() + Duration::from_secs(1);
    while !started.load(Ordering::Acquire) && std::time::Instant::now() < start_deadline {
        std::thread::sleep(Duration::from_millis(1));
    }
    assert!(
        started.load(Ordering::Acquire),
        "slow decoder never started"
    );

    let close_started = std::time::Instant::now();
    receiver.close();
    assert!(
        close_started.elapsed() < Duration::from_millis(100),
        "session close waited for pixel decode"
    );
}

#[test]
fn slow_decoder_never_blocks_session_drop() {
    use std::sync::atomic::{AtomicBool, Ordering};
    use std::sync::Arc;

    struct SlowDecoder(Arc<AtomicBool>);

    impl picoo_media_decode::AccessUnitDecoder for SlowDecoder {
        fn decode_access_unit(
            &mut self,
            _access_unit: &[u8],
            _stream_config: Option<&picoo_protocol::control::StreamConfig>,
        ) -> Result<picoo_media_decode::DecodeOutcome, picoo_media_decode::DecodeError> {
            self.0.store(true, Ordering::Release);
            std::thread::sleep(Duration::from_millis(250));
            Ok(picoo_media_decode::DecodeOutcome::accepted_without_frame(
                false,
            ))
        }

        fn reset(&mut self) -> Result<(), picoo_media_decode::DecodeError> {
            Ok(())
        }
    }

    let started = Arc::new(AtomicBool::new(false));
    let mut receiver = ReceiverSession::new();
    receiver.set_decoder_for_test(Box::new(SlowDecoder(Arc::clone(&started))));
    receiver
        .publish_access_unit(bytes::Bytes::from_static(b"slow-au"), true)
        .expect("queue decode");
    let start_deadline = std::time::Instant::now() + Duration::from_secs(1);
    while !started.load(Ordering::Acquire) && std::time::Instant::now() < start_deadline {
        std::thread::sleep(Duration::from_millis(1));
    }
    assert!(
        started.load(Ordering::Acquire),
        "slow decoder never started"
    );

    let drop_started = std::time::Instant::now();
    drop(receiver);
    assert!(
        drop_started.elapsed() < Duration::from_millis(100),
        "session drop waited for pixel decode"
    );
}

#[test]
fn loopback_sender_to_receiver_latest_frame_store() {
    let payload = b"test-access-unit";
    let frame = run_loopback_access_unit(payload).expect("loopback");
    assert_eq!(&frame.as_ref()[..payload.len()], payload);
}

#[test]
fn single_decode_per_access_unit_into_latest_frame_store() {
    // REQ-PICOO-MEDIA-006: one decode invocation per reassembled AU (hub fans out).
    let payload = b"single-decode-au";
    let mut receiver = ReceiverSession::new();
    use_stub_decoder(&mut receiver);
    receiver.set_jitter_target_ms(0);
    receiver.set_permit_unpaired_video(true);
    let bind = receiver
        .listen(Endpoint {
            host: "127.0.0.1".into(),
            port: 0,
        })
        .expect("listen");

    let mut sender = SenderSession::new(QuicSenderTransport::new());
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

    sender
        .ingest_and_flush_unchecked_for_test(payload, true, 1, 1)
        .expect("ingest");
    for _ in 0..200 {
        receiver.pump().expect("rx");
        sender.pump().ok();
        if receiver.ingress_stats().decode_invocations > 0 && receiver.latest_frame().is_some() {
            break;
        }
        std::thread::sleep(Duration::from_millis(2));
    }

    let stats = receiver.ingress_stats();
    assert_eq!(stats.access_units, 1);
    assert_eq!(stats.decode_invocations, 1);
    let frame = receiver.latest_frame().expect("typed video frame");
    assert_eq!(frame.stream_generation, 1);
    assert_eq!(frame.frame_id, 1);
    assert_eq!(frame.source_pts_us, 1);
    assert!(frame.received_at_us > 0);
}

#[test]
fn paired_loopback_reaches_latest_frame_store_without_unpaired_bypass() {
    let payload = b"paired-product-path-au";
    let frame = run_paired_loopback_access_unit(payload).expect("paired loopback");
    assert_eq!(&frame.as_ref()[..payload.len()], payload);
}
