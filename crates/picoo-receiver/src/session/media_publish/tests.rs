//! REQ-PICOO-NEXT-011: delayed batches carry original meaning through the real worker.
use super::*;
use crate::session::decoder_worker::{DecoderWorker, EncodedAccessUnit, FrameKind};
use bytes::Bytes;
use picoo_media_decode::{
    AccessUnitDecoder, DecodeOutcome, DecodeSubmission, DecodedOutput, StubDecoder,
};
use picoo_protocol::control::StreamConfig;

#[derive(Default)]
struct DelayedDecoder {
    pending: Vec<DecodedOutput>,
}

impl AccessUnitDecoder for DelayedDecoder {
    fn submit(
        &mut self,
        submission: DecodeSubmission<'_>,
    ) -> Result<DecodeOutcome, picoo_media_decode::DecodeError> {
        let emit = submission.token.timeline.frame_id == 3;
        let frames = if emit {
            std::mem::take(&mut self.pending)
        } else {
            Vec::new()
        };
        self.pending
            .extend(StubDecoder::new().submit(submission)?.frames);
        Ok(DecodeOutcome {
            frames,
            refresh_accepted: true,
        })
    }

    fn reset(&mut self) -> Result<(), picoo_media_decode::DecodeError> {
        // Deliberately faulty codec: exercise Receiver's own stale-output gate.
        Ok(())
    }
}

fn receiver() -> ReceiverSession {
    let mut receiver = ReceiverSession::new();
    receiver.decoder_worker = DecoderWorker::with_decoder(Box::<DelayedDecoder>::default());
    receiver.control_generation = Some(1);
    receiver.current_stream_config = Some(Arc::new(StreamConfig {
        width: 64,
        height: 32,
        stream_epoch: 1,
        ..Default::default()
    }));
    receiver
}

fn submit(receiver: &mut ReceiverSession, frame_id: u64, rotation: u32) {
    let config = Arc::make_mut(receiver.current_stream_config.as_mut().unwrap());
    config.rotation = rotation;
    config.mirrored = frame_id == 2;
    let epoch = config.stream_epoch;
    receiver.config_revision = frame_id;
    receiver
        .publish_timeline_access_unit(EncodedAccessUnit {
            connection_generation: receiver.control_generation.unwrap(),
            stream_generation: u64::from(epoch),
            frame_id,
            source_pts_us: frame_id * 100,
            encoded_at_us: frame_id * 200,
            received_at_us: frame_id * 300,
            decode_submitted_at_us: frame_id * 400,
            kind: FrameKind::Key,
            data: Bytes::from(vec![128; 64 * 32 * 3 / 2]),
        })
        .unwrap();
    receiver.drain_decoder_until_idle_for_test();
}

#[test]
fn delayed_batch_uses_each_original_frame_id_time_and_presentation() {
    let mut receiver = receiver();
    submit(&mut receiver, 1, 0);
    submit(&mut receiver, 2, 90);
    assert!(receiver.frames.latest().is_none());
    assert!(!receiver.decoder_recovery.awaiting_refresh());
    submit(&mut receiver, 3, 180);
    assert_eq!(
        receiver.ingress.decoded_frames, 2,
        "must publish both delayed pictures"
    );
    let frame = receiver.frames.latest().unwrap();
    #[cfg(not(target_os = "macos"))]
    {
        assert_eq!(frame.frame_id, 2);
        assert_eq!(frame.source_pts_us, 200);
        assert_eq!(frame.encoded_at_us, 400);
        assert_eq!(frame.received_at_us, 600);
        assert_eq!(frame.decode_submitted_at_us, 800);
        assert_eq!((frame.width, frame.height), (32, 64));
    }
    #[cfg(target_os = "macos")]
    {
        assert_eq!(frame.identity().frame_id, 2);
        assert_eq!(frame.source_pts_us(), 200);
        assert_eq!(frame.timeline().encoded_at_us, 400);
        assert_eq!(frame.timeline().received_at_us, 600);
        assert_eq!(frame.timeline().decode_submitted_at_us, 800);
        assert_eq!(frame.description().config_revision, 2);
        assert_eq!(
            frame.description().transform.rotation,
            picoo_frame_hub::Rotation::Clockwise90
        );
        assert!(frame.description().transform.mirror);
    }
}

#[test]
fn current_submission_cannot_relabel_old_decoder_connection_or_stream_output() {
    for boundary in ["decoder", "connection", "stream"] {
        let mut receiver = receiver();
        submit(&mut receiver, 1, 0);
        submit(&mut receiver, 2, 90);
        match boundary {
            "decoder" => receiver.decoder_worker.reset(),
            "connection" => receiver.control_generation = Some(2),
            "stream" => {
                Arc::make_mut(receiver.current_stream_config.as_mut().unwrap()).stream_epoch = 2
            }
            _ => unreachable!(),
        }
        submit(&mut receiver, 3, 180);
        assert!(
            receiver.frames.latest().is_none(),
            "stale {boundary} output was published"
        );
        assert_eq!(receiver.ingress.decoded_frames, 0);
        assert!(
            receiver.last_media_error.is_none(),
            "fixture codec must complete normally"
        );
    }
}
