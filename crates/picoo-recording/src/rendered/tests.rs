use super::*;
use objc2_core_foundation::{CFDictionary, CFString, CFType};
use objc2_core_video::*;
use picoo_frame_hub::{
    ApplePixelBufferLease, ChromaSiting, FrameBus, FrameDescription, FrameIdentity, FrameTimeline,
    ImageSize, NativeImage, PixelAspectRatio, PresentationTransform, SourceColor, VisibleRect,
};
use std::ptr::{self, NonNull};

fn frame(frame_id: u64, source_pts_us: u64) -> NativeVideoFrame {
    frame_with_generation(frame_id, source_pts_us, 3, 9)
}

fn frame_with_generation(
    frame_id: u64,
    source_pts_us: u64,
    decoder_generation: u64,
    config_revision: u64,
) -> NativeVideoFrame {
    let buffer = unsafe {
        let surface = CFDictionary::<CFString, CFType>::empty();
        let attributes = CFDictionary::<CFString, CFType>::from_slices(
            &[kCVPixelBufferIOSurfacePropertiesKey],
            &[surface.as_ref()],
        );
        let mut raw = ptr::null_mut();
        assert_eq!(
            CVPixelBufferCreate(
                None,
                1280,
                720,
                kCVPixelFormatType_420YpCbCr8BiPlanarVideoRange,
                Some(attributes.as_opaque()),
                NonNull::from(&mut raw),
            ),
            0
        );
        let buffer = objc2_core_foundation::CFRetained::from_raw(NonNull::new(raw).unwrap());
        assert_eq!(
            CVPixelBufferLockBaseAddress(&buffer, CVPixelBufferLockFlags::empty()),
            0
        );
        for plane in 0..2 {
            ptr::write_bytes(
                CVPixelBufferGetBaseAddressOfPlane(&buffer, plane).cast::<u8>(),
                if plane == 0 { 80 } else { 128 },
                CVPixelBufferGetBytesPerRowOfPlane(&buffer, plane)
                    * CVPixelBufferGetHeightOfPlane(&buffer, plane),
            );
        }
        assert_eq!(
            CVPixelBufferUnlockBaseAddress(&buffer, CVPixelBufferLockFlags::empty()),
            0
        );
        for (key, value) in [
            (
                kCVImageBufferYCbCrMatrixKey,
                kCVImageBufferYCbCrMatrix_ITU_R_709_2,
            ),
            (
                kCVImageBufferColorPrimariesKey,
                kCVImageBufferColorPrimaries_ITU_R_709_2,
            ),
            (
                kCVImageBufferTransferFunctionKey,
                kCVImageBufferTransferFunction_ITU_R_709_2,
            ),
        ] {
            buffer.set_attachment(key, value.as_ref(), CVAttachmentMode::ShouldPropagate);
        }
        buffer
    };
    let image =
        NativeImage::Apple(unsafe { ApplePixelBufferLease::retain_completed(&buffer).unwrap() });
    NativeVideoFrame::new(
        FrameIdentity {
            connection_generation: 1,
            stream_epoch: 2,
            decoder_generation,
            frame_id,
        },
        source_pts_us,
        FrameDescription {
            coded_size: ImageSize {
                width: 1280,
                height: 720,
            },
            visible_rect: VisibleRect {
                x: 0,
                y: 0,
                width: 1280,
                height: 720,
            },
            pixel_aspect_ratio: PixelAspectRatio {
                numerator: 1,
                denominator: 1,
            },
            color: SourceColor::Nv12Bt709Limited {
                chroma_siting: ChromaSiting::Left,
            },
            transform: PresentationTransform {
                rotation: Rotation::Clockwise90,
                mirror: true,
            },
            config_revision,
        },
        image,
        FrameTimeline {
            encoded_at_us: source_pts_us,
            received_at_us: source_pts_us,
            decode_submitted_at_us: source_pts_us,
            decoded_at: Instant::now(),
        },
    )
    .unwrap()
}

fn wait(worker: &RenderedRecordingWorker) -> RecordingResult {
    let deadline = Instant::now() + Duration::from_secs(30);
    loop {
        if let Some(result) = worker.result() {
            return result;
        }
        assert!(
            Instant::now() < deadline,
            "rendered recording did not finish"
        );
        std::thread::sleep(Duration::from_millis(2));
    }
}

fn wait_until_recording(worker: &RenderedRecordingWorker) {
    let deadline = Instant::now() + Duration::from_secs(15);
    while worker.state() != RecordingState::Recording {
        assert!(
            Instant::now() < deadline,
            "rendered recording did not leave Arming"
        );
        std::thread::sleep(Duration::from_millis(2));
    }
}

fn config() -> RenderedRecordingConfig {
    RenderedRecordingConfig {
        codec: Codec::Avc,
        width: 1280,
        height: 720,
        fps: 30,
        bitrate: 8_000_000,
        scene_revision: 7,
    }
}

#[test]
fn frame_bus_worker_drains_stop_and_writes_fixed_rate_rendered_bundle() {
    let parent = tempfile::tempdir().unwrap();
    let mut bus = FrameBus::new();
    let subscription = bus.subscribe_ordered().unwrap();
    let worker =
        RenderedRecordingWorker::start(parent.path().to_owned(), subscription, config()).unwrap();
    for (index, pts) in [0, 16_666, 33_333, 50_000, 66_666].into_iter().enumerate() {
        assert!(bus.publish(frame(index as u64 + 1, pts)).is_none());
        // Ordered frames expire after 150ms. Wait until the worker has left
        // Arming before enqueueing the rest, otherwise a slow CI GPU init
        // marks the whole prefix TooOld and the bundle finishes HasGaps.
        if index == 0 {
            wait_until_recording(&worker);
        }
    }
    worker.stop();
    // The stop call has already closed the subscription publication gate.
    assert!(bus.publish(frame(6, 100_000)).is_none());
    let result = wait(&worker);
    assert_eq!(result.state, RecordingState::Complete);
    let path = result.path.unwrap();
    let manifest: serde_json::Value =
        serde_json::from_slice(&std::fs::read(path.join("manifest.json")).unwrap()).unwrap();
    assert_eq!(manifest["mode"], "Rendered");
    assert_eq!(manifest["segments"][0]["metadata"]["scene_revision"], 7);
    assert_eq!(manifest["segments"][0]["metadata"]["source_rotation"], 90);
    assert_eq!(manifest["segments"][0]["metadata"]["source_mirrored"], true);
    assert_eq!(manifest["segments"][0]["metadata"]["rotation"], 0);
    assert_eq!(manifest["segments"][0]["metadata"]["mirrored"], false);
    assert_eq!(
        manifest["segments"][0]["metadata"]["output_last_pts_us"],
        66_666
    );
    let media = path.join(manifest["segments"][0]["file"].as_str().unwrap());
    let frames = crate::apple::tests::read_completed(&media);
    assert_eq!(
        frames.iter().map(|(_, pts)| *pts).collect::<Vec<_>>(),
        vec![0, 33_333, 66_666]
    );

    // Result publication follows slot release, so reuse is deterministic.
    let mut next_bus = FrameBus::new();
    let next = RenderedRecordingWorker::start(
        parent.path().to_owned(),
        next_bus.subscribe_ordered().unwrap(),
        config(),
    )
    .unwrap();
    assert!(next_bus.publish(frame(10, 0)).is_none());
    wait_until_recording(&next);
    next_bus.clear();
    let ended = wait(&next);
    assert_eq!(ended.state, RecordingState::HasGaps);
    let ended_manifest: serde_json::Value =
        serde_json::from_slice(&std::fs::read(ended.path.unwrap().join("manifest.json")).unwrap())
            .unwrap();
    assert_eq!(ended_manifest["gaps"][0]["reason"], "SourceStopped");
}

#[test]
fn gap_and_decoder_generation_changes_create_independent_real_segments() {
    let parent = tempfile::tempdir().unwrap();
    let mut recorder = RenderedRecorder::create(parent.path(), config()).unwrap();
    recorder.push(Arc::new(frame(1, 0))).unwrap();
    recorder.push(Arc::new(frame(2, 100_000))).unwrap();
    recorder
        .push(Arc::new(frame_with_generation(3, 133_333, 4, 9)))
        .unwrap();
    recorder.finish().unwrap();
    assert_eq!(recorder.state(), RecordingState::HasGaps);
    let manifest: serde_json::Value =
        serde_json::from_slice(&std::fs::read(recorder.path().join("manifest.json")).unwrap())
            .unwrap();
    assert_eq!(manifest["segments"].as_array().unwrap().len(), 3);
    assert_eq!(manifest["gaps"].as_array().unwrap().len(), 1);
    for segment in manifest["segments"].as_array().unwrap() {
        assert_eq!(segment["metadata"]["output_first_pts_us"], 0);
        let media = recorder.path().join(segment["file"].as_str().unwrap());
        assert_eq!(crate::apple::tests::read_completed(&media)[0].1, 0);
    }
}

#[test]
fn panic_after_a_valid_prefix_persists_failed_bundle_and_segment() {
    let parent = tempfile::tempdir().unwrap();
    let mut recorder = RenderedRecorder::create(parent.path(), config()).unwrap();
    let result = guarded(&mut recorder, |recorder| {
        recorder.push(Arc::new(frame(1, 0)))?;
        panic!("injected rendered worker panic");
    });
    assert!(matches!(result, Err(RecordingError::Platform(_))));
    assert_eq!(recorder.state(), RecordingState::Failed);
    let manifest: serde_json::Value =
        serde_json::from_slice(&std::fs::read(recorder.path().join("manifest.json")).unwrap())
            .unwrap();
    assert_eq!(manifest["state"], "Failed");
    assert_eq!(manifest["segments"].as_array().unwrap().len(), 1);
}
