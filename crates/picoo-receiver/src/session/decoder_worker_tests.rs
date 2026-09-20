use super::*;
use std::sync::atomic::{AtomicBool, Ordering};
use std::time::Duration;

struct BlockingDecoder {
    started: Arc<AtomicBool>,
    release: Arc<AtomicBool>,
}

impl AccessUnitDecoder for BlockingDecoder {
    fn submit(
        &mut self,
        submission: picoo_media_decode::DecodeSubmission<'_>,
    ) -> Result<DecodeOutcome, DecodeError> {
        let _access_unit = submission.access_unit;
        let _stream_config = submission.token.stream_config.as_deref();

        self.started.store(true, Ordering::Release);
        while !self.release.load(Ordering::Acquire) {
            thread::sleep(Duration::from_millis(1));
        }
        Ok(DecodeOutcome::accepted_without_frame(false))
    }

    fn reset(&mut self) -> Result<(), DecodeError> {
        Ok(())
    }
}

fn unit(frame_id: u64, kind: FrameKind) -> EncodedAccessUnit {
    EncodedAccessUnit {
        connection_generation: 1,
        stream_generation: 1,
        frame_id,
        source_pts_us: frame_id,
        encoded_at_us: frame_id,
        received_at_us: frame_id,
        decode_submitted_at_us: frame_id,
        kind,
        data: Bytes::from_static(b"au"),
    }
}

#[test]
fn thread_affine_decoder_is_created_used_and_dropped_on_its_worker() {
    struct AffineDecoder {
        owner: thread::ThreadId,
        // Intentionally !Send: represents apartment-bound native state.
        _affinity: std::rc::Rc<()>,
        events: Sender<(&'static str, thread::ThreadId)>,
    }
    impl AccessUnitDecoder for AffineDecoder {
        fn submit(
            &mut self,
            _submission: picoo_media_decode::DecodeSubmission<'_>,
        ) -> Result<DecodeOutcome, DecodeError> {
            assert_eq!(thread::current().id(), self.owner);
            self.events.send(("decode", self.owner)).unwrap();
            Ok(DecodeOutcome {
                frames: Vec::new(),
                refresh_accepted: false,
            })
        }
        fn reset(&mut self) -> Result<(), DecodeError> {
            assert_eq!(thread::current().id(), self.owner);
            self.events.send(("reset", self.owner)).unwrap();
            Ok(())
        }
    }
    impl Drop for AffineDecoder {
        fn drop(&mut self) {
            assert_eq!(thread::current().id(), self.owner);
            self.events.send(("drop", self.owner)).unwrap();
        }
    }
    let caller = thread::current().id();
    let (events, received) = mpsc::channel();
    let worker = DecoderWorker::with_decoder_factory(
        move || {
            let owner = thread::current().id();
            events.send(("create", owner)).unwrap();
            Box::new(AffineDecoder {
                owner,
                _affinity: std::rc::Rc::new(()),
                events,
            })
        },
        picoo_transport::TransportEventWake::default(),
    );
    let (event, owner) = received.recv_timeout(Duration::from_secs(3)).unwrap();
    assert_eq!(event, "create");
    assert_ne!(owner, caller);
    assert_eq!(
        worker.submit(unit(1, FrameKind::Key), None, 0),
        DecodeSubmitOutcome::Queued
    );
    assert_eq!(
        received.recv_timeout(Duration::from_secs(3)).unwrap(),
        ("decode", owner)
    );
    worker.reset();
    assert_eq!(
        received.recv_timeout(Duration::from_secs(3)).unwrap(),
        ("reset", owner)
    );
    drop(worker);
    assert_eq!(
        received.recv_timeout(Duration::from_secs(3)).unwrap(),
        ("drop", owner)
    );
}

#[test]
fn queue_is_bounded_and_drops_discardable_before_reference_media() {
    let release = Arc::new(AtomicBool::new(false));
    let started = Arc::new(AtomicBool::new(false));
    let worker = DecoderWorker::with_decoder(Box::new(BlockingDecoder {
        started: Arc::clone(&started),
        release: Arc::clone(&release),
    }));
    assert_eq!(
        worker.submit(unit(1, FrameKind::Key), None, 0),
        DecodeSubmitOutcome::Queued
    );
    let deadline = Instant::now() + Duration::from_secs(1);
    while !started.load(Ordering::Acquire) && Instant::now() < deadline {
        thread::sleep(Duration::from_millis(1));
    }
    assert!(started.load(Ordering::Acquire), "decoder did not start");
    assert_eq!(
        worker.submit(unit(2, FrameKind::DiscardableDelta), None, 0),
        DecodeSubmitOutcome::Queued
    );
    assert_eq!(
        worker.submit(unit(3, FrameKind::ReferenceDelta), None, 0),
        DecodeSubmitOutcome::Queued
    );
    assert_eq!(
        worker.admission(FrameKind::ReferenceDelta),
        DecoderAdmission::Ready,
        "queued discardable AU remains replaceable"
    );
    assert_eq!(
        worker.submit(unit(4, FrameKind::ReferenceDelta), None, 0),
        DecodeSubmitOutcome::Queued,
        "reference AU replaces the queued discardable AU"
    );
    assert_eq!(
        worker.admission(FrameKind::ReferenceDelta),
        DecoderAdmission::WaitForCapacity
    );
    assert_eq!(
        worker.admission(FrameKind::DiscardableDelta),
        DecoderAdmission::DropDiscardable
    );
    assert_eq!(worker.admission(FrameKind::Key), DecoderAdmission::Ready);
    assert_eq!(
        worker.submit(unit(5, FrameKind::DiscardableDelta), None, 0),
        DecodeSubmitOutcome::Dropped {
            requires_refresh: false
        }
    );
    assert_eq!(
        worker.submit(unit(6, FrameKind::ReferenceDelta), None, 0),
        DecodeSubmitOutcome::Dropped {
            requires_refresh: true
        }
    );
    release.store(true, Ordering::Release);
}

#[test]
fn reset_invalidates_an_active_decode_generation() {
    let release = Arc::new(AtomicBool::new(false));
    let started = Arc::new(AtomicBool::new(false));
    let worker = DecoderWorker::with_decoder(Box::new(BlockingDecoder {
        started: Arc::clone(&started),
        release: Arc::clone(&release),
    }));
    assert_eq!(
        worker.submit(unit(1, FrameKind::Key), None, 0),
        DecodeSubmitOutcome::Queued
    );
    let deadline = Instant::now() + Duration::from_secs(1);
    while !started.load(Ordering::Acquire) && Instant::now() < deadline {
        thread::sleep(Duration::from_millis(1));
    }
    assert!(started.load(Ordering::Acquire), "decoder did not start");

    worker.reset();
    release.store(true, Ordering::Release);

    let deadline = Instant::now() + Duration::from_secs(1);
    while Instant::now() < deadline {
        if let Some(DecoderEvent::Completed {
            decoder_generation, ..
        }) = worker.poll_event()
        {
            assert!(!worker.is_current_generation(decoder_generation));
            return;
        }
        thread::sleep(Duration::from_millis(1));
    }
    panic!("active decode did not complete");
}

#[test]
fn preparation_finishes_on_owner_before_queued_live_decode() {
    let (entered, entered_rx) = mpsc::channel();
    let (release, release_rx) = mpsc::channel();
    let started = Arc::new(AtomicBool::new(false));
    let marker = started.clone();
    let caller = thread::current().id();
    let worker = DecoderWorker::with_preparation(
        move || {
            Box::new(BlockingDecoder {
                started: marker,
                release: Arc::new(AtomicBool::new(true)),
            })
        },
        move |_| {
            entered.send(thread::current().id()).unwrap();
            release_rx.recv_timeout(Duration::from_secs(3)).unwrap();
            Ok(super::super::decoder_capabilities::synthetic_capabilities())
        },
        picoo_transport::TransportEventWake::default(),
    );
    assert_ne!(
        entered_rx.recv_timeout(Duration::from_secs(3)).unwrap(),
        caller
    );
    assert_eq!(
        worker.submit(unit(1, FrameKind::Key), None, 0),
        DecodeSubmitOutcome::Queued
    );
    assert!(!started.load(Ordering::Acquire));
    assert!(worker.poll_event().is_none());
    release.send(()).unwrap();
    assert!(matches!(
        worker.events.recv_timeout(Duration::from_secs(3)).unwrap(),
        DecoderEvent::Capabilities(Ok(_))
    ));
    assert!(matches!(
        worker.events.recv_timeout(Duration::from_secs(3)).unwrap(),
        DecoderEvent::Started
    ));
}

#[test]
fn failed_preparation_never_decodes_queued_live_media() {
    let (entered, entered_rx) = mpsc::channel();
    let (release, release_rx) = mpsc::channel();
    let started = Arc::new(AtomicBool::new(false));
    let marker = started.clone();
    let mut worker = DecoderWorker::with_preparation(
        move || {
            Box::new(BlockingDecoder {
                started: marker,
                release: Arc::new(AtomicBool::new(true)),
            })
        },
        move |_| {
            entered.send(()).unwrap();
            release_rx.recv_timeout(Duration::from_secs(3)).unwrap();
            Err(DecodeError::NotInitialized)
        },
        picoo_transport::TransportEventWake::default(),
    );
    entered_rx.recv_timeout(Duration::from_secs(3)).unwrap();
    assert_eq!(
        worker.submit(unit(1, FrameKind::Key), None, 0),
        DecodeSubmitOutcome::Queued
    );
    release.send(()).unwrap();
    assert!(matches!(
        worker.events.recv_timeout(Duration::from_secs(3)).unwrap(),
        DecoderEvent::Capabilities(Err(_))
    ));
    worker.thread.take().unwrap().join().unwrap();
    assert!(!started.load(Ordering::Acquire));
    assert!(matches!(
        worker.submit(unit(2, FrameKind::Key), None, 0),
        DecodeSubmitOutcome::Dropped { .. }
    ));
}

#[test]
#[cfg(target_os = "macos")]
fn native_factory_prepares_complete_offers_on_worker() {
    let worker = DecoderWorker::with_preparation(
        picoo_media_decode::create_platform_decoder,
        picoo_media_decode::probe_capabilities,
        picoo_transport::TransportEventWake::default(),
    );
    let DecoderEvent::Capabilities(Ok(caps)) =
        worker.events.recv_timeout(Duration::from_secs(10)).unwrap()
    else {
        panic!("native worker probe failed");
    };
    caps.validate().unwrap();
    assert!(caps.offers.iter().any(
        |offer| offer.format.unwrap().codec == picoo_protocol::control::VideoCodec::Hevc as i32
    ));
    assert!(
        worker.poll_event().is_none(),
        "probe outputs are not live media events"
    );
}

#[test]
fn reset_failure_stops_instance_instead_of_reusing_its_capabilities() {
    struct ResetFailure;
    impl AccessUnitDecoder for ResetFailure {
        fn submit(
            &mut self,
            _: picoo_media_decode::DecodeSubmission<'_>,
        ) -> Result<DecodeOutcome, DecodeError> {
            panic!("failed instance must not accept media");
        }
        fn reset(&mut self) -> Result<(), DecodeError> {
            Err(DecodeError::NotInitialized)
        }
    }
    let mut worker = DecoderWorker::with_decoder(Box::new(ResetFailure));
    assert!(matches!(
        worker.events.recv_timeout(Duration::from_secs(3)).unwrap(),
        DecoderEvent::Capabilities(Ok(_))
    ));
    worker.reset();
    assert!(matches!(
        worker.events.recv_timeout(Duration::from_secs(3)).unwrap(),
        DecoderEvent::Unavailable(_)
    ));
    worker.thread.take().unwrap().join().unwrap();
    assert!(matches!(
        worker.submit(unit(1, FrameKind::Key), None, 0),
        DecodeSubmitOutcome::Dropped { .. }
    ));
    assert!(
        worker.poll_event().is_none(),
        "no silent replacement capabilities"
    );
}

#[test]
fn constructor_panic_completes_preparation_as_unavailable() {
    let mut worker = DecoderWorker::with_preparation(
        || panic!("native constructor failed"),
        |_| panic!("preparation cannot run without decoder"),
        picoo_transport::TransportEventWake::default(),
    );
    assert!(matches!(
        worker.events.recv_timeout(Duration::from_secs(3)).unwrap(),
        DecoderEvent::Capabilities(Err(_))
    ));
    worker.thread.take().unwrap().join().unwrap();
    assert!(matches!(
        worker.submit(unit(1, FrameKind::Key), None, 0),
        DecodeSubmitOutcome::Dropped { .. }
    ));
}
