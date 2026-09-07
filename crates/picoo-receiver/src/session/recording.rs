//! Encoded recording attachment — REQ-PICOO-MEDIA-072.
use super::ReceiverSession;
use crate::ReceiverError;
use picoo_packet::AssembledAccessUnit;
use picoo_recording::{
    bundle::{GapReason, RecordingState},
    ingress::IngressFailure,
    worker::{RecordingResult, RecordingWorker},
};
#[cfg(test)]
use std::sync::Arc;
use std::{path::PathBuf, time::Instant};

impl ReceiverSession {
    pub fn start_encoded_recording(&mut self, parent: PathBuf) -> Result<(), ReceiverError> {
        if !self.video_allowed() || self.current_stream_config.is_none() {
            return Err(ReceiverError::Protocol(
                "recording requires authorized video configuration".into(),
            ));
        }
        if self
            .recording
            .as_ref()
            .is_some_and(|worker| worker.result().is_none())
        {
            return Err(ReceiverError::Protocol(
                "recording is already active".into(),
            ));
        }
        self.recording = Some(RecordingWorker::start(parent)?);
        self.recording_configuration_wait.clear();
        Ok(())
    }

    pub fn stop_encoded_recording(&mut self) {
        self.resolve_recording_configuration();
        if let Some(worker) = &mut self.recording {
            if !self.recording_configuration_wait.is_empty() {
                worker.terminate(IngressFailure::ConfigurationUnavailable);
                self.recording_configuration_wait.clear();
            }
            worker.stop();
        }
    }

    pub fn encoded_recording_state(&self) -> Option<RecordingState> {
        self.recording.as_ref().map(RecordingWorker::state)
    }

    pub fn encoded_recording_result(&self) -> Option<RecordingResult> {
        self.recording.as_ref().and_then(RecordingWorker::result)
    }

    pub(super) fn record_assembled_access_unit(&mut self, access_unit: &AssembledAccessUnit) {
        if !self
            .recording
            .as_ref()
            .is_some_and(RecordingWorker::is_accepting)
        {
            return;
        }
        self.resolve_recording_configuration();
        let (Some(configuration), Some(generation)) =
            (&self.current_stream_config, self.control_generation)
        else {
            return;
        };
        if let Err(failure) = self.recording_configuration_wait.push(
            generation,
            access_unit.clone(),
            configuration,
            Instant::now(),
        ) {
            if let Some(worker) = &mut self.recording {
                worker.terminate(failure);
            }
            self.recording_configuration_wait.clear();
        }
        self.resolve_recording_configuration();
    }

    fn resolve_recording_configuration(&mut self) {
        let (Some(worker), Some(configuration), Some(generation)) = (
            &mut self.recording,
            &self.current_stream_config,
            self.control_generation,
        ) else {
            return;
        };
        match self
            .recording_configuration_wait
            .resolve(generation, configuration, Instant::now())
        {
            Ok(ready) => {
                for input in ready {
                    let _ = worker.offer(input);
                }
            }
            Err(failure) => {
                worker.terminate(failure);
                self.recording_configuration_wait.clear();
            }
        }
    }

    pub(super) fn report_recording_gap(&mut self, reason: GapReason) {
        if let Some(worker) = &mut self.recording {
            let _ = worker.report_gap(reason, None);
        }
    }

    pub(super) fn pump_recording_control(&mut self) {
        self.resolve_recording_configuration();
        let requested = self
            .recording
            .as_ref()
            .is_some_and(RecordingWorker::take_refresh_request);
        if !requested {
            return;
        }
        if !self.video_allowed() {
            self.stop_encoded_recording();
            return;
        }
        let Some(session) = self.transport.active_session() else {
            self.report_recording_gap(GapReason::SourceStopped);
            self.stop_encoded_recording();
            return;
        };
        // This requests a new encoder RAP without entering live decoder recovery.
        if let Err(error) = self.send_request_keyframe_now(session) {
            tracing::warn!(%error, "recording keyframe request failed");
            self.report_recording_gap(GapReason::SourceStopped);
            self.stop_encoded_recording();
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use picoo_bitstream::{AccessUnit, Codec, CodecConfiguration, NalFormat};
    use picoo_protocol::control::{StreamConfig, VideoFormat};
    use std::time::{Duration, Instant};

    #[test]
    fn live_recovery_preserves_recording_aus_and_tail_loss_is_reported() {
        let parent = tempfile::tempdir().unwrap();
        let mut receiver = ReceiverSession::new();
        assert!(receiver
            .start_encoded_recording(parent.path().to_owned())
            .is_err());
        receiver.permit_unpaired_video = true;
        assert!(receiver
            .start_encoded_recording(parent.path().to_owned())
            .is_err());
        let fixture = include_bytes!("../../../picoo-testkit/fixtures/avc-1280x720-bt709-idr.h264");
        let picture = AccessUnit::parse(Codec::Avc, NalFormat::AnnexB, fixture).unwrap();
        let sps = picture.nals().iter().find(|nal| nal[0] & 31 == 7).unwrap();
        let pps = picture.nals().iter().find(|nal| nal[0] & 31 == 8).unwrap();
        let configuration = CodecConfiguration::from_avc_parameter_sets(sps, pps).unwrap();
        let format = VideoFormat::from_codec_configuration(&configuration, 30).unwrap();
        let data = AccessUnit::parse(Codec::Avc, NalFormat::AnnexB, fixture)
            .unwrap()
            .to_length_prefixed()
            .unwrap();
        receiver.current_stream_config = Some(Arc::new(StreamConfig {
            codec: format.codec,
            profile: format.profile,
            level_idc: u32::from(configuration.level_idc()),
            width: 1280,
            height: 720,
            fps: 30,
            codec_configuration: configuration.record().to_vec(),
            stream_epoch: 1,
            color_range: format.color.unwrap().range,
            ..Default::default()
        }));
        receiver.control_generation = Some(1);
        receiver
            .start_encoded_recording(parent.path().to_owned())
            .unwrap();
        for id in [2, 1] {
            let au = AssembledAccessUnit {
                data: data.clone().into(),
                frame_id: id,
                pts_us: (id - 1) * 33_333,
                encoded_at_us: 0,
                keyframe: true,
                discardable: false,
                stream_epoch: 1,
                fragment_count: 1,
                first_fragment_at: Instant::now(),
            };
            receiver.record_assembled_access_unit(&au);
            receiver.queue_assembled_access_unit(au).unwrap();
        }
        receiver
            .enter_decoder_recovery(super::super::recovery::RecoveryReason::ManualRepair, true)
            .unwrap();
        // Both complete future-epoch AUs survive live recovery while the
        // reliable configuration has not arrived yet (including non-IDR hints).
        for id in [2, 1] {
            receiver.record_assembled_access_unit(&AssembledAccessUnit {
                data: data.clone().into(),
                frame_id: id,
                pts_us: (id - 1) * 33_333,
                encoded_at_us: 0,
                keyframe: false,
                discardable: false,
                stream_epoch: 2,
                fragment_count: 1,
                first_fragment_at: Instant::now(),
            });
        }
        assert!(!receiver.recording_configuration_wait.is_empty());
        let mut next = receiver
            .current_stream_config
            .as_ref()
            .unwrap()
            .as_ref()
            .clone();
        next.stream_epoch = 2;
        receiver.current_stream_config = Some(Arc::new(next));
        receiver.resolve_recording_configuration();
        assert!(receiver.recording_configuration_wait.is_empty());
        receiver.report_recording_gap(GapReason::NetworkLoss);
        receiver.stop_encoded_recording();
        let deadline = Instant::now() + Duration::from_secs(15);
        let result = loop {
            if let Some(result) = receiver.encoded_recording_result() {
                break result;
            }
            assert!(Instant::now() < deadline);
            std::thread::sleep(Duration::from_millis(2));
        };
        assert_eq!(result.state, RecordingState::HasGaps);
        let stored = std::fs::read_to_string(result.path.unwrap().join("manifest.json")).unwrap();
        assert!(stored.contains("\"first_au\": 1"));
        assert!(stored.contains("\"last_au\": 2"));
        assert!(stored.contains("NetworkLoss"));
        assert!(stored.contains("\"stream_epoch\": 2"));
    }
}
