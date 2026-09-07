//! Bounded complete-format probes on the Decoder owner thread — REQ-PICOO-MEDIA-052.
use std::sync::Arc;

use picoo_bitstream::{Codec, CodecConfiguration};
use picoo_protocol::control::{
    Capabilities, ColorRange, DecoderOffer, StreamConfig, VideoCodec, VideoProfile,
};

use crate::{
    AccessUnitDecoder, AccessUnitTimeline, DecodeError, DecodeSubmission, DecodeToken, FrameKind,
};

struct Candidate {
    codec: Codec,
    height: u32,
    fps: u32,
    record: &'static [u8],
    access_unit: &'static [u8],
}

macro_rules! candidate {
    ($family:literal, $codec:ident, $stem:literal, $height:expr, $fps:expr) => {
        Candidate {
            codec: Codec::$codec,
            height: $height,
            fps: $fps,
            record: include_bytes!(concat!("../probes/", $family, "/", $stem, ".config")),
            access_unit: include_bytes!(concat!("../probes/", $family, "/", $stem, ".au")),
        }
    };
}

const CANDIDATES: &[Candidate] = &[
    candidate!("apple-native-formats", Avc, "1-720-30", 720, 30),
    candidate!("apple-native-formats", Avc, "1-720-60", 720, 60),
    candidate!("apple-native-formats", Avc, "1-1080-30", 1080, 30),
    candidate!("apple-native-formats", Avc, "1-1080-60", 1080, 60),
    candidate!("apple-native-formats", Hevc, "2-720-30", 720, 30),
    candidate!("apple-native-formats", Hevc, "2-720-60", 720, 60),
    candidate!("apple-native-formats", Hevc, "2-1080-30", 1080, 30),
    candidate!("apple-native-formats", Hevc, "2-1080-60", 1080, 60),
    candidate!("xiaomi-native-formats", Avc, "1-720-30", 720, 30),
    candidate!("xiaomi-native-formats", Avc, "1-720-60", 720, 60),
    candidate!("xiaomi-native-formats", Avc, "1-1080-30", 1080, 30),
    candidate!("xiaomi-native-formats", Avc, "1-1080-60", 1080, 60),
    candidate!("xiaomi-native-formats", Hevc, "2-720-30", 720, 30),
    candidate!("xiaomi-native-formats", Hevc, "2-720-60", 720, 60),
    candidate!("xiaomi-native-formats", Hevc, "2-1080-30", 1080, 30),
    candidate!("xiaomi-native-formats", Hevc, "2-1080-60", 1080, 60),
];

/// Probe the supplied backend on its creating thread before accepting live media.
/// Production callers must use the native factory. No frames escape this function,
/// and this API does not select another backend when a candidate fails.
/// Successful configuration and output are not a sustained throughput measurement.
pub fn probe_capabilities(
    decoder: &mut dyn AccessUnitDecoder,
) -> Result<Capabilities, DecodeError> {
    decoder.reset()?;
    let mut offers: Vec<DecoderOffer> = Vec::new();
    for (index, candidate) in CANDIDATES.iter().enumerate() {
        let config = candidate.configuration()?;
        let format = config.validated_video_format().map_err(probe_error)?;
        let level = config.level_idc;
        let token = Arc::new(DecodeToken {
            timeline: AccessUnitTimeline {
                connection_generation: 0,
                stream_generation: index as u64 + 1,
                frame_id: index as u64 + 1,
                source_pts_us: index as u64 * 1_000_000,
                encoded_at_us: 0,
                received_at_us: 0,
                decode_submitted_at_us: 0,
                kind: FrameKind::Key,
            },
            decoder_generation: 0,
            config_revision: index as u64 + 1,
            stream_config: Some(Arc::new(config)),
        });
        let result = decoder.submit(DecodeSubmission {
            access_unit: candidate.access_unit,
            token: token.clone(),
        });
        let accepted = match result {
            Ok(outcome) => {
                if outcome
                    .frames
                    .iter()
                    .any(|output| !Arc::ptr_eq(&output.token, &token))
                {
                    return Err(probe_error("probe output lost original token"));
                }
                let valid_image = outcome.frames.len() == 1
                    && outcome.frames.iter().all(|output| {
                        #[cfg(any(target_os = "macos", windows))]
                        {
                            let rect = output.frame.description().native_format.visible_rect;
                            rect.width == if candidate.height == 720 { 1280 } else { 1920 }
                                && rect.height == candidate.height
                        }
                        #[cfg(not(any(target_os = "macos", windows)))]
                        {
                            let _ = output;
                            false
                        }
                    });
                outcome.refresh_accepted && valid_image
            }
            Err(error) => {
                tracing::debug!(?format, %error, "native decoder candidate unavailable");
                false
            }
        };
        // Drop native outputs before reset, and never retain a probe image in an offer.
        decoder.reset()?;
        if accepted {
            if let Some(existing) = offers.iter_mut().find(|offer| offer.format == Some(format)) {
                existing.max_level_idc = existing.max_level_idc.max(level);
            } else {
                offers.push(DecoderOffer {
                    format: Some(format),
                    max_level_idc: level,
                    max_access_unit_bytes: picoo_protocol::MAX_MEDIA_ACCESS_UNIT_BYTES,
                });
            }
        }
    }
    let capabilities = Capabilities { offers };
    capabilities.validate().map_err(probe_error)?;
    Ok(capabilities)
}

impl Candidate {
    fn configuration(&self) -> Result<StreamConfig, DecodeError> {
        let record = CodecConfiguration::parse(self.codec, bytes::Bytes::from_static(self.record))
            .map_err(probe_error)?;
        Ok(StreamConfig {
            codec: match self.codec {
                Codec::Avc => VideoCodec::Avc,
                Codec::Hevc => VideoCodec::Hevc,
            } as i32,
            profile: match self.codec {
                Codec::Avc => VideoProfile::AvcHigh,
                Codec::Hevc => VideoProfile::HevcMain,
            } as i32,
            level_idc: u32::from(record.level_idc()),
            width: if self.height == 720 { 1280 } else { 1920 },
            height: self.height,
            fps: self.fps,
            color_range: ColorRange::Limited as i32,
            codec_configuration: self.record.to_vec(),
            stream_epoch: 1,
            ..Default::default()
        })
    }
}

fn probe_error(error: impl std::fmt::Display) -> DecodeError {
    DecodeError::Platform(format!("native decoder capability probe: {error}"))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::DecodeOutcome;

    struct RejectingDecoder {
        submissions: usize,
        resets: usize,
        fail_reset: bool,
    }
    impl AccessUnitDecoder for RejectingDecoder {
        fn submit(&mut self, _: DecodeSubmission<'_>) -> Result<DecodeOutcome, DecodeError> {
            self.submissions += 1;
            Err(DecodeError::UnsupportedAccessUnit)
        }
        fn reset(&mut self) -> Result<(), DecodeError> {
            self.resets += 1;
            if self.fail_reset {
                Err(DecodeError::NotInitialized)
            } else {
                Ok(())
            }
        }
    }

    #[test]
    fn unsupported_backend_never_inherits_fixture_capabilities() {
        let mut decoder = RejectingDecoder {
            submissions: 0,
            resets: 0,
            fail_reset: false,
        };
        assert!(probe_capabilities(&mut decoder).is_err());
        assert_eq!(decoder.submissions, CANDIDATES.len());
        assert_eq!(decoder.resets, CANDIDATES.len() + 1);
    }

    #[test]
    fn reset_failure_aborts_before_trying_another_configuration() {
        let mut decoder = RejectingDecoder {
            submissions: 0,
            resets: 0,
            fail_reset: true,
        };
        assert!(probe_capabilities(&mut decoder).is_err());
        assert_eq!(decoder.submissions, 0);
        assert_eq!(decoder.resets, 1);
    }

    #[test]
    #[cfg(target_os = "macos")]
    fn real_native_probe_admits_complete_formats_without_exposing_frames() {
        let mut decoder = crate::create_platform_decoder();
        let caps = probe_capabilities(decoder.as_mut()).unwrap();
        caps.validate().unwrap();
        for codec in [VideoCodec::Avc, VideoCodec::Hevc] {
            for height in [720, 1080] {
                for fps in [30, 60] {
                    assert!(
                        caps.offers.iter().any(|offer| {
                            let format = offer.format.unwrap();
                            format.codec == codec as i32
                                && format.visible_rect.unwrap().height == height
                                && format.frame_rate.unwrap().numerator == fps
                        }),
                        "missing {codec:?}/{height}/{fps}"
                    );
                }
            }
        }
        assert!(caps.offers.iter().any(|offer| {
            let format = offer.format.unwrap();
            format.coded_size.unwrap().height == 736
                && format.tier == picoo_protocol::control::VideoTier::HevcHigh as i32
        }));
    }
}
