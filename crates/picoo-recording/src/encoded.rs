//! Ordered encoded segment state machine — REQ-PICOO-MEDIA-069.
//! Owned exclusively by the recording worker. Callers reorder complete AUs first.
use crate::{
    apple::{AppendOutcome, AppleSegment},
    bundle::{GapReason, RecordingBundle, RecordingState, SegmentMetadata, SourceRange},
    ingress::RecordingInput,
    RecordingError,
};
use picoo_bitstream::{
    AccessUnit, Codec, CodecConfiguration, NalFormat, PictureKind, RandomAccessPoint,
};
use picoo_protocol::control::{StreamConfig, VideoCodec};
use sha2::{Digest, Sha256};
use std::{
    path::Path,
    sync::Arc,
    time::{Duration, Instant},
};

const SEGMENT_DURATION_US: u64 = 10_000_000;
const WRITE_DEADLINE: Duration = Duration::from_millis(250);

struct ActiveSegment {
    native: AppleSegment,
    configuration: Arc<StreamConfig>,
    metadata: SegmentMetadata,
    boundary_requested: bool,
}

/// This is a synchronous worker component, not a Receiver/UI API.
/// CRA/leading-picture sequences remain explicitly unsupported by the adapter.
pub struct EncodedWriter {
    bundle: RecordingBundle,
    active: Option<ActiveSegment>,
    refresh_requested: bool,
}

impl EncodedWriter {
    pub fn create(parent: &Path) -> Result<Self, RecordingError> {
        Ok(Self {
            bundle: RecordingBundle::create(parent)?,
            active: None,
            refresh_requested: true,
        })
    }

    pub fn path(&self) -> &Path {
        self.bundle.path()
    }
    pub fn state(&self) -> RecordingState {
        self.bundle.state()
    }
    pub fn waiting_for_refresh(&self) -> bool {
        self.active.is_none()
    }

    /// Consume the request once; a worker forwards it through Receiver control.
    pub fn take_refresh_request(&mut self) -> bool {
        std::mem::take(&mut self.refresh_requested)
    }

    pub fn write_ordered(&mut self, input: RecordingInput) -> Result<(), RecordingError> {
        let result = self.write_inner(input);
        if result.is_err() {
            self.abort("encoded segment write failed");
        }
        result
    }

    fn write_inner(&mut self, input: RecordingInput) -> Result<(), RecordingError> {
        if !matches!(
            self.state(),
            RecordingState::Arming | RecordingState::Recording
        ) {
            return Err(RecordingError::InvalidInput("recording already finalized"));
        }
        let config = &input.configuration;
        let au = &input.access_unit;
        if au.stream_epoch != config.stream_epoch {
            return Err(RecordingError::InvalidInput(
                "AU configuration epoch mismatch",
            ));
        }
        config
            .validated_video_format()
            .map_err(|_| RecordingError::InvalidInput("invalid source format"))?;
        let codec = match VideoCodec::try_from(config.codec) {
            Ok(VideoCodec::Avc) => Codec::Avc,
            Ok(VideoCodec::Hevc) => Codec::Hevc,
            _ => return Err(RecordingError::InvalidInput("unknown codec")),
        };
        let configuration =
            CodecConfiguration::parse(codec, config.codec_configuration.clone().into())
                .map_err(|_| RecordingError::InvalidInput("invalid codec configuration"))?;
        let picture = AccessUnit::parse(
            codec,
            NalFormat::LengthPrefixed(configuration.nal_length_size()),
            &au.data,
        )
        .map_err(|_| RecordingError::InvalidInput("invalid access unit"))?;
        configuration
            .validate_parameter_sets(&picture)
            .map_err(|_| RecordingError::InvalidInput("AU parameter sets differ"))?;
        let kind = picture.picture().kind;
        let idr = matches!(
            kind,
            PictureKind::RandomAccess(RandomAccessPoint::AvcIdr | RandomAccessPoint::HevcIdr)
        );
        if !idr && kind != PictureKind::Trailing {
            return Err(RecordingError::InvalidInput(
                "unsupported leading-picture sequence",
            ));
        }
        if let Some(active) = &self.active {
            let source = &active.metadata.source;
            let changed = source.connection_generation != input.connection_generation
                || active.configuration.stream_epoch != config.stream_epoch
                || active.configuration.codec_configuration != config.codec_configuration
                || active.configuration.fps != config.fps
                || active.configuration.rotation != config.rotation
                || active.configuration.mirrored != config.mirrored;
            if changed {
                self.close_segment()?;
                self.refresh_requested = true;
            } else if au.frame_id <= source.last_au || au.pts_us <= source.last_pts_us {
                return Err(RecordingError::InvalidInput("input is not ordered"));
            } else if source.last_au.checked_add(1) != Some(au.frame_id) {
                self.gap(
                    GapReason::NetworkLoss,
                    Some(SourceRange {
                        connection_generation: input.connection_generation,
                        stream_epoch: au.stream_epoch,
                        first_au: source.last_au + 1,
                        last_au: au.frame_id - 1,
                        first_pts_us: source.last_pts_us,
                        last_pts_us: au.pts_us,
                    }),
                )?;
            } else if au.pts_us - source.last_pts_us > 3_000_000 / u64::from(config.fps) {
                self.gap(
                    GapReason::TimeDiscontinuity,
                    Some(SourceRange {
                        connection_generation: input.connection_generation,
                        stream_epoch: au.stream_epoch,
                        first_au: source.last_au,
                        last_au: au.frame_id,
                        first_pts_us: source.last_pts_us,
                        last_pts_us: au.pts_us,
                    }),
                )?;
            } else if au.pts_us - source.first_pts_us >= SEGMENT_DURATION_US {
                if idr {
                    self.close_segment()?;
                } else if !active.boundary_requested {
                    self.active
                        .as_mut()
                        .expect("active segment")
                        .boundary_requested = true;
                    self.refresh_requested = true;
                }
            }
        }
        if self.active.is_none() {
            if !idr {
                return Ok(());
            }
            let native =
                AppleSegment::new(&self.bundle.next_partial_path()?, configuration, config.fps)?;
            self.active = Some(ActiveSegment {
                native,
                configuration: Arc::clone(config),
                boundary_requested: false,
                metadata: SegmentMetadata {
                    source: SourceRange {
                        connection_generation: input.connection_generation,
                        stream_epoch: au.stream_epoch,
                        first_au: au.frame_id,
                        last_au: au.frame_id,
                        first_pts_us: au.pts_us,
                        last_pts_us: au.pts_us,
                    },
                    codec: if codec == Codec::Avc { "avc" } else { "hevc" }.into(),
                    width: config.width,
                    height: config.height,
                    fps: config.fps,
                    rotation: config.rotation,
                    mirrored: config.mirrored,
                    configuration_sha256: format!(
                        "{:x}",
                        Sha256::digest(&config.codec_configuration)
                    ),
                },
            });
        }
        let active = self.active.as_mut().expect("IDR starts segment");
        let pts = au.pts_us - active.metadata.source.first_pts_us;
        let deadline = Instant::now() + WRITE_DEADLINE;
        loop {
            match active.native.append(&au.data, pts)? {
                AppendOutcome::Written => break,
                AppendOutcome::Busy if Instant::now() < deadline => {
                    std::thread::sleep(Duration::from_millis(1))
                }
                AppendOutcome::Busy => {
                    return Err(RecordingError::Platform("recording writer overrun".into()))
                }
            }
        }
        active.metadata.source.last_au = au.frame_id;
        active.metadata.source.last_pts_us = au.pts_us;
        self.bundle.mark_started(au.pts_us)?;
        if idr {
            self.refresh_requested = false;
        }
        Ok(())
    }

    pub fn gap(
        &mut self,
        reason: GapReason,
        source: Option<SourceRange>,
    ) -> Result<(), RecordingError> {
        let result = (|| {
            self.close_segment()?;
            self.bundle.record_gap(reason, source)?;
            self.refresh_requested = true;
            Ok(())
        })();
        if result.is_err() {
            self.abort("gap boundary finalization failed");
        }
        result
    }

    fn close_segment(&mut self) -> Result<(), RecordingError> {
        if let Some(active) = self.active.take() {
            self.bundle
                .commit_segment(active.native.finish()?, active.metadata)?;
        }
        Ok(())
    }

    pub fn finish(&mut self) -> Result<(), RecordingError> {
        let result = self.close_segment().and_then(|()| self.bundle.finish());
        if result.is_err() {
            self.abort("recording finalization failed");
        }
        result
    }

    pub fn abort(&mut self, reason: &str) {
        // Retain successfully accepted samples when the native writer is still
        // healthy. The overall result remains Failed even if this prefix closes.
        let _ = self.close_segment();
        let _ = self.bundle.fail(reason);
        self.active = None;
        self.refresh_requested = false;
    }
}

#[cfg(test)]
pub(crate) mod tests;
