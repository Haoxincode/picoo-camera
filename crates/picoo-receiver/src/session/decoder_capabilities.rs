//! Decoder-owned evidence and connection-scoped negotiation — REQ-PICOO-MEDIA-053.
use picoo_protocol::control::{Capabilities, StreamConfig};
use picoo_transport::SessionId;

use super::ReceiverSession;
use crate::ReceiverError;

#[derive(Default)]
pub(super) enum DecoderReadiness {
    #[default]
    Pending,
    Ready(Capabilities),
    Unavailable(String),
}

pub(super) struct PendingConfiguration {
    pub session: SessionId,
    pub control_generation: Option<u64>,
    pub config: StreamConfig,
}

impl ReceiverSession {
    pub(super) fn finish_decoder_negotiation(&mut self) -> Result<(), ReceiverError> {
        let Some(session) = self.transport.active_session() else {
            return Ok(());
        };
        if !self.video_allowed() || self.control_generation.is_none() {
            return Ok(());
        }
        match &self.decoder_readiness {
            DecoderReadiness::Pending => return Ok(()),
            DecoderReadiness::Unavailable(error) => {
                let error = error.clone();
                self.apply_receiver_event(super::reducer::ReceiverEvent::AbortConnection {
                    generation: session.0,
                    reason: super::reducer::ReceiverCloseReason::DecoderUnavailable,
                })?;
                self.last_media_error = Some(error);
                return Ok(());
            }
            DecoderReadiness::Ready(_) => {}
        }
        if self.receiver_capabilities_sent.is_none() {
            self.send_capabilities(session)?;
            self.receiver_capabilities_sent = Some(());
        }
        if let Some(pending) = self.pending_decoder_configuration.take() {
            if pending.session == session && pending.control_generation == self.control_generation {
                if let Err(error) = self.handle_stream_config(session, pending.config) {
                    self.reject_control_session(session);
                    return Err(error);
                }
            }
        }
        Ok(())
    }
}

// Explicit fixture evidence for injected test decoders; never compiled into a
// normal Receiver. Native preparation itself is tested against the real factory.
#[cfg(any(test, feature = "loopback-diagnostics"))]
pub(super) fn synthetic_capabilities() -> Capabilities {
    use picoo_protocol::control::{
        ColorRange, DecoderOffer, FrameRate, Resolution, VideoCodec, VideoFormat,
    };
    let mut offers = Vec::new();
    for codec in [VideoCodec::Avc, VideoCodec::Hevc] {
        for (width, height, visible_height) in [
            (1280, 720, 720),
            (1280, 736, 720),
            (1920, 1080, 1080),
            (1920, 1088, 1080),
        ] {
            for fps in [30, 60] {
                let mut format = VideoFormat::sdr_709(
                    codec,
                    Resolution { width, height },
                    FrameRate {
                        numerator: fps,
                        denominator: 1,
                    },
                    ColorRange::Limited,
                );
                format.visible_rect.as_mut().unwrap().height = visible_height;
                offers.push(DecoderOffer {
                    format: Some(format),
                    max_level_idc: if codec == VideoCodec::Avc { 42 } else { 123 },
                    max_access_unit_bytes: picoo_protocol::MAX_MEDIA_ACCESS_UNIT_BYTES,
                });
            }
        }
    }
    let caps = Capabilities { offers };
    caps.validate().expect("explicit test offers");
    caps
}

#[cfg(test)]
mod tests {
    use super::*;
    use picoo_protocol::control::{ColorRange, VideoCodec, VideoProfile};

    fn source(epoch: u32) -> StreamConfig {
        let record = include_bytes!(
            "../../../picoo-media-decode/probes/xiaomi-native-formats/2-720-60.config"
        );
        let parsed = picoo_bitstream::CodecConfiguration::parse(
            picoo_bitstream::Codec::Hevc,
            bytes::Bytes::copy_from_slice(record),
        )
        .unwrap();
        StreamConfig {
            codec: VideoCodec::Hevc as i32,
            profile: VideoProfile::HevcMain as i32,
            level_idc: u32::from(parsed.level_idc()),
            width: 1280,
            height: 720,
            fps: 60,
            color_range: ColorRange::Limited as i32,
            codec_configuration: record.to_vec(),
            stream_epoch: epoch,
            ..Default::default()
        }
    }

    fn evidence(config: &StreamConfig) -> Capabilities {
        Capabilities {
            offers: vec![picoo_protocol::control::DecoderOffer {
                format: Some(config.validated_video_format().unwrap()),
                max_level_idc: config.level_idc,
                max_access_unit_bytes: 4096,
            }],
        }
    }

    #[test]
    fn pending_probe_retains_only_latest_valid_configuration_without_committing() {
        let mut receiver = ReceiverSession::new();
        receiver.control_generation = Some(12);
        receiver
            .handle_stream_config(SessionId(3), source(7))
            .unwrap();
        receiver
            .handle_stream_config(SessionId(3), source(6))
            .unwrap();
        assert!(receiver.current_stream_config.is_none());
        assert_eq!(receiver.config_revision, 0);
        let pending = receiver.pending_decoder_configuration.as_ref().unwrap();
        assert_eq!(
            (
                pending.session,
                pending.control_generation,
                pending.config.stream_epoch
            ),
            (SessionId(3), Some(12), 7)
        );
        let mut malformed = source(8);
        malformed.width = 1920;
        assert!(receiver
            .handle_stream_config(SessionId(3), malformed)
            .is_err());
        assert_eq!(
            receiver
                .pending_decoder_configuration
                .as_ref()
                .unwrap()
                .config
                .stream_epoch,
            7
        );
    }

    #[test]
    fn actual_hevc_padding_and_tier_must_match_one_native_offer() {
        let config = source(7);
        let caps = evidence(&config);
        let mut receiver = ReceiverSession::new();
        receiver.receiver_capabilities_sent = Some(());
        let mut wrong = caps.clone();
        wrong.offers[0].format.as_mut().unwrap().tier =
            picoo_protocol::control::VideoTier::HevcMain as i32;
        receiver.decoder_readiness = DecoderReadiness::Ready(wrong);
        assert!(receiver
            .handle_stream_config(SessionId(3), config.clone())
            .is_err());
        assert!(receiver.current_stream_config.is_none());
        let mut wrong = caps.clone();
        wrong.offers[0]
            .format
            .as_mut()
            .unwrap()
            .coded_size
            .as_mut()
            .unwrap()
            .height = 720;
        receiver.decoder_readiness = DecoderReadiness::Ready(wrong);
        assert!(receiver
            .handle_stream_config(SessionId(3), config.clone())
            .is_err());
        assert_eq!(receiver.config_revision, 0);
        receiver.decoder_readiness = DecoderReadiness::Ready(caps);
        receiver.handle_stream_config(SessionId(3), config).unwrap();
        assert_eq!(receiver.config_revision, 1);
        assert_eq!(
            receiver
                .current_stream_config
                .as_ref()
                .unwrap()
                .stream_epoch,
            7
        );
    }

    #[test]
    fn unavailable_decoder_cannot_commit_even_a_valid_configuration() {
        let mut receiver = ReceiverSession::new();
        receiver.decoder_readiness = DecoderReadiness::Unavailable("device failed".into());
        assert!(receiver
            .handle_stream_config(SessionId(3), source(1))
            .is_err());
        assert!(receiver.current_stream_config.is_none());
        assert!(receiver.pending_decoder_configuration.is_none());
    }
    #[test]
    fn disconnect_clears_pending_configuration_without_discarding_decoder_evidence() {
        let mut receiver = ReceiverSession::new();
        receiver
            .apply_receiver_event(super::super::reducer::ReceiverEvent::TransportConnected {
                generation: 3,
            })
            .unwrap();
        receiver
            .handle_stream_config(SessionId(3), source(7))
            .unwrap();
        assert!(receiver.pending_decoder_configuration.is_some());
        receiver.close();
        assert!(receiver.pending_decoder_configuration.is_none());
        assert!(receiver.current_stream_config.is_none());
    }
}
