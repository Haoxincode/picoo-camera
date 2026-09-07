use super::*;
use objc2_av_foundation::{
    AVAssetReader, AVAssetReaderStatus, AVAssetReaderTrackOutput, AVAssetTrack, AVURLAsset,
};
use objc2_core_media::CMBlockBuffer;
use objc2_foundation::{NSArray, NSError};
use picoo_bitstream::Codec;
use std::{ptr::NonNull, time::Instant};

type ReadFrames = Result<Vec<(Vec<u8>, i64)>, String>;

pub(crate) fn fixtures() -> Vec<(CodecConfiguration, Vec<u8>)> {
    let avc = include_bytes!("../../../picoo-testkit/fixtures/avc-1280x720-bt709-idr.h264");
    let picture = AccessUnit::parse(Codec::Avc, NalFormat::AnnexB, avc).unwrap();
    let sps = picture.nals().iter().find(|nal| nal[0] & 31 == 7).unwrap();
    let pps = picture.nals().iter().find(|nal| nal[0] & 31 == 8).unwrap();
    let avc_config = CodecConfiguration::from_avc_parameter_sets(sps, pps).unwrap();
    let avc = picture.to_length_prefixed().unwrap();
    let hevc_config = CodecConfiguration::parse(
        Codec::Hevc,
        include_bytes!("../../../picoo-testkit/fixtures/hevc-1280x720-bt709-config.bin")
            .as_slice()
            .to_vec()
            .into(),
    )
    .unwrap();
    let hevc =
        include_bytes!("../../../picoo-testkit/fixtures/hevc-1280x720-bt709-idr.bin").to_vec();
    vec![(avc_config, avc), (hevc_config, hevc)]
}

fn read_track(tracks: *mut NSArray<AVAssetTrack>, error: *mut NSError) -> ReadFrames {
    // SAFETY: AVAsset supplies these pointers for the callback's duration. No
    // native objects leave this function; only owned compressed bytes do.
    unsafe {
        if !error.is_null() {
            return Err(error.as_ref().unwrap().localizedDescription().to_string());
        }
        let tracks = tracks.as_ref().ok_or("missing tracks")?;
        let track = tracks.firstObject().ok_or("missing video track")?;
        let asset = track.asset().ok_or("missing owning asset")?;
        let reader =
            AVAssetReader::assetReaderWithAsset_error(&asset).map_err(|e| e.to_string())?;
        let output =
            AVAssetReaderTrackOutput::assetReaderTrackOutputWithTrack_outputSettings(&track, None);
        reader.addOutput(&output);
        if !reader.startReading() {
            return Err("reader did not start".into());
        }
        let mut frames = Vec::new();
        while let Some(sample) = output.copyNextSampleBuffer() {
            if sample.num_samples() == 0 {
                continue;
            }
            let block = sample.data_buffer().ok_or("missing AU")?;
            let size = block.data_length();
            if size == 0 || size > 2 * 1024 * 1024 || frames.len() >= 100 {
                return Err("excessive readback".into());
            }
            let mut bytes = vec![0; size];
            let status = CMBlockBuffer::copy_data_bytes(
                &block,
                0,
                size,
                NonNull::new(bytes.as_mut_ptr().cast()).unwrap(),
            );
            if status != 0 {
                return Err(format!("AU copy failed {status}"));
            }
            let pts = sample.presentation_time_stamp();
            if pts.timescale <= 0 {
                return Err("invalid PTS".into());
            }
            frames.push((bytes, pts.value * 1_000_000 / i64::from(pts.timescale)));
        }
        if reader.status() != AVAssetReaderStatus::Completed {
            return Err("readback failed".into());
        }
        Ok(frames)
    }
}

fn read(path: &Path) -> Vec<(Vec<u8>, i64)> {
    let completed = path.with_extension("mp4");
    std::fs::rename(path, &completed).unwrap();
    read_completed(&completed)
}

fn read_completed(path: &Path) -> Vec<(Vec<u8>, i64)> {
    autoreleasepool(|_| {
        let (sender, receiver) = mpsc::sync_channel(1);
        let callback = block2::RcBlock::new(
            move |tracks: *mut NSArray<AVAssetTrack>, error: *mut NSError| {
                let result = autoreleasepool(|_| read_track(tracks, error));
                let _ = sender.try_send(result);
            },
        );
        let url = NSURL::fileURLWithPath(&NSString::from_str(path.to_str().unwrap()));
        // SAFETY: The callback captures only a Send channel and consumes native
        // callback references synchronously. The asset remains retained here.
        let _asset = unsafe {
            let asset = AVURLAsset::URLAssetWithURL_options(&url, None);
            asset.loadTracksWithMediaType_completionHandler(AVMediaTypeVideo.unwrap(), &callback);
            asset
        };
        receiver
            .recv_timeout(Duration::from_secs(10))
            .unwrap()
            .unwrap()
    })
}

#[test]
fn avc_and_hevc_segments_preserve_compressed_bytes_and_microsecond_pts() {
    for (configuration, au) in fixtures() {
        for fps in [30, 60] {
            let directory = tempfile::tempdir().unwrap();
            let path = directory.path().join("segment.partial");
            let mut segment = AppleSegment::new(&path, configuration.clone(), fps).unwrap();
            for index in 0..5 {
                let pts = index * 1_000_000 / u64::from(fps);
                let deadline = Instant::now() + Duration::from_secs(2);
                while segment.append(&au, pts).unwrap() == AppendOutcome::Busy {
                    assert!(Instant::now() < deadline, "writer remained busy");
                    std::thread::sleep(Duration::from_millis(1));
                }
            }
            segment.finish().unwrap();
            let frames = read(&path);
            assert_eq!(frames.len(), 5);
            for (index, (bytes, pts)) in frames.iter().enumerate() {
                assert_eq!(bytes, &au);
                assert_eq!(*pts, index as i64 * 1_000_000 / i64::from(fps));
            }
        }
    }
}

#[test]
fn segment_rejects_overwrite_invalid_start_and_nonmonotonic_time() {
    let (configuration, au) = fixtures().remove(0);
    let directory = tempfile::tempdir().unwrap();
    let existing = directory.path().join("existing.partial");
    std::fs::write(&existing, b"keep").unwrap();
    assert!(AppleSegment::new(&existing, configuration.clone(), 30).is_err());
    assert_eq!(std::fs::read(&existing).unwrap(), b"keep");
    let path = directory.path().join("new.partial");
    let mut segment = AppleSegment::new(&path, configuration, 30).unwrap();
    assert!(segment.append(&au, 1).is_err());
    let delta_header = [0, 0, 0, 2, 0x41, 0x80];
    assert!(segment.append(&delta_header, 0).is_err());
    assert_eq!(segment.append(&au, 0).unwrap(), AppendOutcome::Written);
    assert!(segment.append(&au, 0).is_err());
    assert!(segment.append(&au, u64::MAX).is_err());
    segment.finish().unwrap();
    assert_eq!(read(&path).len(), 1);
}

#[test]
fn native_finalization_bundle_promotion_and_system_readback() {
    use crate::bundle::{RecordingBundle, RecordingState, SegmentMetadata, SourceRange};
    use sha2::{Digest, Sha256};
    for (configuration, au) in fixtures() {
        let parent = tempfile::tempdir().unwrap();
        let mut bundle = RecordingBundle::create(parent.path()).unwrap();
        let path = bundle.next_partial_path().unwrap();
        let codec = match configuration.codec() {
            Codec::Avc => "avc",
            Codec::Hevc => "hevc",
        };
        let mut segment = AppleSegment::new(&path, configuration, 30).unwrap();
        assert_eq!(segment.append(&au, 0).unwrap(), AppendOutcome::Written);
        bundle.mark_started(500).unwrap();
        bundle
            .commit_segment(
                segment.finish().unwrap(),
                SegmentMetadata {
                    source: SourceRange {
                        connection_generation: 1,
                        stream_epoch: 1,
                        first_au: 1,
                        last_au: 1,
                        first_pts_us: 500,
                        last_pts_us: 500,
                    },
                    codec: codec.into(),
                    width: 1280,
                    height: 720,
                    fps: 30,
                    rotation: 0,
                    mirrored: false,
                    configuration_sha256: format!("{:x}", Sha256::digest(b"fixture configuration")),
                },
            )
            .unwrap();
        bundle.finish().unwrap();
        assert_eq!(bundle.state(), RecordingState::Complete);
        assert!(!path.exists());
        assert_eq!(read_completed(&path.with_extension("mp4")), vec![(au, 0)]);
    }
}

#[test]
fn native_cancellation_preserves_avc_and_hevc_written_bytes() {
    use std::io::Read;
    for (configuration, au) in fixtures() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("cancelled.partial");
        let mut writer = AppleSegment::new(&path, configuration, 30).unwrap();
        let mut original = std::fs::File::open(&path).unwrap();
        for frame in 0..91 {
            let deadline = Instant::now() + Duration::from_secs(2);
            while writer.append(&au, frame * 1_000_000 / 30).unwrap() == AppendOutcome::Busy {
                assert!(Instant::now() < deadline);
                std::thread::sleep(Duration::from_millis(1));
            }
        }
        let deadline = Instant::now() + Duration::from_secs(2);
        while original.metadata().unwrap().len() == 0 {
            assert!(Instant::now() < deadline);
            std::thread::sleep(Duration::from_millis(2));
        }
        writer.cancel_preserving_partial().unwrap();
        let mut expected = Vec::new();
        original.read_to_end(&mut expected).unwrap();
        assert!(!expected.is_empty());
        assert_eq!(std::fs::read(&path).unwrap(), expected);
        assert!(!path.with_extension("mp4").exists());
        assert!(writer.finish().is_err());
        assert_eq!(std::fs::read(&path).unwrap(), expected);
    }
}
