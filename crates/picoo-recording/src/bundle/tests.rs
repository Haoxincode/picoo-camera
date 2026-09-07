use super::*;

fn metadata() -> SegmentMetadata {
    SegmentMetadata {
        source: SourceRange {
            connection_generation: 1,
            stream_epoch: 2,
            first_au: 10,
            last_au: 20,
            first_pts_us: 100,
            last_pts_us: 300,
        },
        codec: "avc".into(),
        width: 1280,
        height: 720,
        fps: 30,
        rotation: 0,
        mirrored: false,
        configuration_sha256: "0".repeat(64),
    }
}

// Filesystem tests use a finalized-writer stand-in. Apple tests separately
// verify actual native MP4 bytes and finalization through the system reader.
fn completed(bundle: &RecordingBundle) -> FinalizedSegment {
    let path = bundle.next_partial_path().unwrap();
    fs::write(&path, b"finalized compressed segment").unwrap();
    FinalizedSegment { path }
}

fn manifest(bundle: &RecordingBundle) -> serde_json::Value {
    serde_json::from_slice(&fs::read(bundle.path().join("manifest.json")).unwrap()).unwrap()
}

#[test]
fn completed_segments_are_durable_named_hashed_and_gaps_remain_sticky() {
    let parent = tempfile::tempdir().unwrap();
    let mut bundle = RecordingBundle::create(parent.path()).unwrap();
    bundle.mark_started(100).unwrap();
    bundle.record_gap(GapReason::NetworkLoss, None).unwrap();
    let segment = completed(&bundle);
    let partial = segment.path.clone();
    bundle.commit_segment(segment, metadata()).unwrap();
    assert!(!partial.exists());
    let stored = manifest(&bundle);
    let file = bundle
        .path()
        .join(stored["segments"][0]["file"].as_str().unwrap());
    assert_eq!(fs::read(&file).unwrap(), b"finalized compressed segment");
    assert_eq!(
        stored["segments"][0]["sha256"],
        hash_reader(&mut File::open(file).unwrap()).unwrap()
    );
    bundle.finish().unwrap();
    bundle.finish().unwrap();
    assert_eq!(bundle.state(), RecordingState::HasGaps);
    assert_eq!(manifest(&bundle)["state"], "HasGaps");
    assert!(bundle.mark_started(999).is_err());
}

#[test]
fn failure_and_empty_recordings_cannot_be_completed_successfully() {
    let parent = tempfile::tempdir().unwrap();
    let mut bundle = RecordingBundle::create(parent.path()).unwrap();
    let partial = bundle.next_partial_path().unwrap();
    fs::write(&partial, b"unfinished").unwrap();
    bundle.fail("disk stopped").unwrap();
    bundle.finish().unwrap();
    assert_eq!(bundle.state(), RecordingState::Failed);
    assert_eq!(manifest(&bundle)["failure"], "disk stopped");
    assert_eq!(fs::read(partial).unwrap(), b"unfinished");
    let mut empty = RecordingBundle::create(parent.path()).unwrap();
    assert_ne!(empty.path(), bundle.path());
    empty.finish().unwrap();
    assert_eq!(empty.state(), RecordingState::Failed);
}

#[test]
fn promotion_never_overwrites_existing_files_and_keeps_partial() {
    let parent = tempfile::tempdir().unwrap();
    let mut bundle = RecordingBundle::create(parent.path()).unwrap();
    let segment = completed(&bundle);
    let partial = segment.path.clone();
    let complete = partial.with_extension("mp4");
    fs::write(&complete, b"existing file").unwrap();
    assert!(bundle.commit_segment(segment, metadata()).is_err());
    assert_eq!(fs::read(complete).unwrap(), b"existing file");
    assert!(partial.exists());
    bundle.finish().unwrap();
    assert_eq!(bundle.state(), RecordingState::Failed);
    assert_eq!(manifest(&bundle)["segments"].as_array().unwrap().len(), 0);
}

#[test]
fn persistence_failure_keeps_old_manifest_and_marks_memory_failed() {
    let parent = tempfile::tempdir().unwrap();
    let mut bundle = RecordingBundle::create(parent.path()).unwrap();
    let path = bundle.path().join("manifest.json");
    let original = fs::read(&path).unwrap();
    fs::rename(&path, bundle.path().join("previous.json")).unwrap();
    fs::create_dir(&path).unwrap();
    assert!(bundle.record_gap(GapReason::Overrun, None).is_err());
    assert_eq!(bundle.state(), RecordingState::Failed);
    bundle.finish().unwrap();
    assert_eq!(bundle.state(), RecordingState::Failed);
    assert_eq!(
        fs::read(bundle.path().join("previous.json")).unwrap(),
        original
    );
    assert_eq!(bundle.manifest.gaps.len(), 1);
}

#[test]
fn metadata_capacity_is_bounded_and_foreign_segments_are_rejected() {
    let parent = tempfile::tempdir().unwrap();
    let mut bundle = RecordingBundle::create(parent.path()).unwrap();
    let other = RecordingBundle::create(parent.path()).unwrap();
    let foreign = completed(&other);
    let path = foreign.path.clone();
    assert!(bundle.commit_segment(foreign, metadata()).is_err());
    assert!(path.exists());
    let mut bundle = RecordingBundle::create(parent.path()).unwrap();
    bundle.manifest.gaps = vec![
        Gap {
            reason: GapReason::NetworkLoss,
            source: None
        };
        MAX_GAPS
    ];
    assert!(bundle.record_gap(GapReason::Overrun, None).is_err());
    assert_eq!(bundle.manifest.gaps.len(), MAX_GAPS);
    bundle.finish().unwrap();
    assert_eq!(bundle.state(), RecordingState::Failed);
}

#[cfg(unix)]
#[test]
fn bundle_and_manifest_are_private_to_the_current_user() {
    use std::os::unix::fs::PermissionsExt;
    let parent = tempfile::tempdir().unwrap();
    let bundle = RecordingBundle::create(parent.path()).unwrap();
    assert_eq!(
        fs::metadata(bundle.path()).unwrap().permissions().mode() & 0o777,
        0o700
    );
    assert_eq!(
        fs::metadata(bundle.path().join("manifest.json"))
            .unwrap()
            .permissions()
            .mode()
            & 0o777,
        0o600
    );
}
