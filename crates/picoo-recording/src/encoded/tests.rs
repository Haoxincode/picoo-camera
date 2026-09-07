use super::*;
use picoo_packet::AssembledAccessUnit;
use picoo_protocol::control::VideoFormat;

fn input(codec_index: usize, epoch: u32, id: u64, pts_us: u64) -> RecordingInput {
    let (configuration, bytes) = crate::apple::tests::fixtures().remove(codec_index);
    let format = VideoFormat::from_codec_configuration(&configuration, 30).unwrap();
    RecordingInput {
        connection_generation: 1,
        configuration: Arc::new(StreamConfig {
            codec: format.codec,
            profile: format.profile,
            level_idc: u32::from(configuration.level_idc()),
            width: 1280,
            height: 720,
            fps: 30,
            codec_configuration: configuration.record().to_vec(),
            stream_epoch: epoch,
            color_range: format.color.unwrap().range,
            ..Default::default()
        }),
        access_unit: AssembledAccessUnit {
            data: bytes.into(),
            frame_id: id,
            pts_us,
            encoded_at_us: pts_us,
            keyframe: false, // wire hints are not RAP evidence
            discardable: false,
            stream_epoch: epoch,
            fragment_count: 1,
            first_fragment_at: Instant::now(),
        },
    }
}

fn manifest(writer: &EncodedWriter) -> serde_json::Value {
    serde_json::from_slice(&std::fs::read(writer.path().join("manifest.json")).unwrap()).unwrap()
}

#[test]
fn native_segments_switch_codec_and_preserve_source_mapping() {
    let parent = tempfile::tempdir().unwrap();
    let mut writer = EncodedWriter::create(parent.path()).unwrap();
    assert!(writer.take_refresh_request());
    assert!(!writer.take_refresh_request());
    writer.write_ordered(input(0, 1, 1, 500)).unwrap();
    writer.write_ordered(input(0, 1, 2, 33_833)).unwrap();
    writer.write_ordered(input(1, 2, 1, 90_000)).unwrap();
    writer.finish().unwrap();
    assert_eq!(writer.state(), RecordingState::Complete);
    let manifest = manifest(&writer);
    assert_eq!(manifest["actual_start_pts_us"], 500);
    let segments = manifest["segments"].as_array().unwrap();
    assert_eq!(segments.len(), 2);
    assert_eq!(segments[0]["metadata"]["codec"], "avc");
    assert_eq!(segments[1]["metadata"]["codec"], "hevc");
    assert_eq!(segments[0]["metadata"]["source"]["last_pts_us"], 33_833);
    assert_eq!(segments[1]["metadata"]["source"]["first_pts_us"], 90_000);
}

#[test]
fn missing_ordered_au_creates_a_gap_and_retains_both_segments() {
    let parent = tempfile::tempdir().unwrap();
    let mut writer = EncodedWriter::create(parent.path()).unwrap();
    writer.write_ordered(input(0, 1, 1, 0)).unwrap();
    writer.write_ordered(input(0, 1, 3, 66_666)).unwrap();
    writer.finish().unwrap();
    assert_eq!(writer.state(), RecordingState::HasGaps);
    let manifest = manifest(&writer);
    assert_eq!(manifest["segments"].as_array().unwrap().len(), 2);
    assert_eq!(manifest["gaps"][0]["reason"], "NetworkLoss");
}

#[test]
fn ten_second_idr_starts_a_new_independently_finalized_segment() {
    let parent = tempfile::tempdir().unwrap();
    let mut writer = EncodedWriter::create(parent.path()).unwrap();
    for id in 0..=301 {
        writer
            .write_ordered(input(0, 1, id, id * 1_000_000 / 30))
            .unwrap();
    }
    writer.finish().unwrap();
    assert_eq!(writer.state(), RecordingState::Complete);
    let manifest = manifest(&writer);
    let segments = manifest["segments"].as_array().unwrap();
    assert_eq!(segments.len(), 2);
    assert_eq!(
        segments[1]["metadata"]["source"]["first_pts_us"],
        10_000_000
    );
}

#[test]
fn unordered_input_preserves_valid_prefix_but_never_reports_complete() {
    let parent = tempfile::tempdir().unwrap();
    let mut writer = EncodedWriter::create(parent.path()).unwrap();
    writer.write_ordered(input(0, 1, 2, 100)).unwrap();
    assert!(writer.write_ordered(input(0, 1, 1, 99)).is_err());
    writer.finish().unwrap();
    assert_eq!(writer.state(), RecordingState::Failed);
    assert!(writer.path().join("segments/000001.mp4").exists());
    assert_eq!(manifest(&writer)["segments"].as_array().unwrap().len(), 1);
}

#[test]
fn arming_ignores_a_non_idr_hint_and_records_the_actual_start() {
    let parent = tempfile::tempdir().unwrap();
    let mut writer = EncodedWriter::create(parent.path()).unwrap();
    let mut delta = input(0, 1, 1, 0);
    // Header-only non-IDR policy fixture; it must never reach the native mux.
    delta.access_unit.data = vec![0, 0, 0, 2, 0x41, 0x80].into();
    delta.access_unit.keyframe = true;
    writer.write_ordered(delta).unwrap();
    assert_eq!(writer.state(), RecordingState::Arming);
    assert!(!writer.path().join("segments/000001.partial").exists());
    writer.write_ordered(input(0, 1, 2, 33_333)).unwrap();
    writer.finish().unwrap();
    assert_eq!(manifest(&writer)["actual_start_pts_us"], 33_333);
}

#[test]
fn source_time_jump_is_reported_even_when_au_ids_are_contiguous() {
    let parent = tempfile::tempdir().unwrap();
    let mut writer = EncodedWriter::create(parent.path()).unwrap();
    writer.write_ordered(input(0, 1, 1, 0)).unwrap();
    writer.write_ordered(input(0, 1, 2, 200_000)).unwrap();
    writer.finish().unwrap();
    assert_eq!(writer.state(), RecordingState::HasGaps);
    assert_eq!(manifest(&writer)["gaps"][0]["reason"], "TimeDiscontinuity");
}
