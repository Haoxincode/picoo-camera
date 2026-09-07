//! AVAssetWriter passthrough segments — REQ-PICOO-MEDIA-066.
//! Construct and use on the recording worker. No pixel buffers or encoders.

use crate::{FinalizedSegment, RecordingError};
use objc2::{
    rc::{autoreleasepool, Retained},
    AnyThread,
};
use objc2_av_foundation::{
    AVAssetWriter, AVAssetWriterInput, AVAssetWriterStatus, AVFileTypeMPEG4, AVMediaTypeVideo,
};
use objc2_core_foundation::CFRetained;
use objc2_core_media::{CMFormatDescription, CMTime};
use objc2_foundation::{NSString, NSURL};
use picoo_bitstream::{
    AccessUnit, CodecConfiguration, NalFormat, NalLengthSize, PictureKind, RandomAccessPoint,
};
use std::{path::Path, sync::mpsc, time::Duration};
mod partial;
mod sample;

#[derive(Debug, PartialEq, Eq)]
pub enum AppendOutcome {
    Written,
    Busy,
}

/// One independently decodable MP4 segment, with one immutable sample description.
/// Paths must name a new partial file. Promotion and manifest updates belong to
/// the recording owner after successful finish, never this platform adapter.
pub struct AppleSegment {
    path: std::path::PathBuf,
    partial: partial::RetainedPartial,
    writer: Retained<AVAssetWriter>,
    input: Retained<AVAssetWriterInput>,
    format: CFRetained<CMFormatDescription>,
    configuration: CodecConfiguration,
    fps: u32,
    last_pts_us: Option<u64>,
}

impl AppleSegment {
    pub fn new(
        path: &Path,
        configuration: CodecConfiguration,
        fps: u32,
    ) -> Result<Self, RecordingError> {
        autoreleasepool(|_| Self::create(path, configuration, fps))
    }

    fn create(
        path: &Path,
        configuration: CodecConfiguration,
        fps: u32,
    ) -> Result<Self, RecordingError> {
        if !matches!(fps, 30 | 60) || configuration.nal_length_size() != NalLengthSize::Four {
            return Err(RecordingError::InvalidInput(
                "unsupported fps or NAL length",
            ));
        }
        let path = path
            .to_str()
            .ok_or(RecordingError::InvalidInput("non-UTF8 output path"))?;
        let format = sample::format(&configuration)?;
        let url = NSURL::fileURLWithPath(&NSString::from_str(path));
        // SAFETY: Known MP4 UTI, valid file URL, and a retained video format.
        // nil output settings explicitly request compressed passthrough.
        let (writer, input) = unsafe {
            let writer = AVAssetWriter::assetWriterWithURL_fileType_error(
                &url,
                AVFileTypeMPEG4.ok_or(RecordingError::InvalidInput("MP4 UTI unavailable"))?,
            )
            .map_err(|error| RecordingError::Platform(error.localizedDescription().to_string()))?;
            let input = AVAssetWriterInput::initWithMediaType_outputSettings_sourceFormatHint(
                AVAssetWriterInput::alloc(),
                AVMediaTypeVideo
                    .ok_or(RecordingError::InvalidInput("video media type unavailable"))?,
                None,
                Some(&format),
            );
            input.setExpectsMediaDataInRealTime(true);
            input.setMediaTimeScale(1_000_000);
            if !writer.canAddInput(&input) {
                return Err(RecordingError::Platform(
                    "writer rejected compressed video input".into(),
                ));
            }
            writer.addInput(&input);
            writer.setMovieFragmentInterval(CMTime {
                value: 10,
                timescale: 1,
                flags: objc2_core_media::CMTimeFlags::Valid,
                epoch: 0,
            });
            if !writer.startWriting() {
                return Err(writer_error(&writer));
            }
            writer.startSessionAtSourceTime(sample::time(0)?);
            (writer, input)
        };
        let partial = partial::RetainedPartial::open(Path::new(path))?;
        Ok(Self {
            partial,
            path: std::path::PathBuf::from(path),
            writer,
            input,
            format,
            configuration,
            fps,
            last_pts_us: None,
        })
    }

    /// PTS is relative to this segment's first source AU, not wall time.
    /// A busy result consumes nothing. The recording worker owns retry deadlines.
    pub fn append(&mut self, data: &[u8], pts_us: u64) -> Result<AppendOutcome, RecordingError> {
        autoreleasepool(|_| {
            if self.last_pts_us.is_some_and(|previous| pts_us <= previous) {
                return Err(RecordingError::InvalidInput("non-increasing segment PTS"));
            }
            let picture = AccessUnit::parse(
                self.configuration.codec(),
                NalFormat::LengthPrefixed(NalLengthSize::Four),
                data,
            )
            .map_err(|error| RecordingError::Platform(error.to_string()))?;
            self.configuration
                .validate_parameter_sets(&picture)
                .map_err(|error| RecordingError::Platform(error.to_string()))?;
            let sync = match picture.picture().kind {
                PictureKind::RandomAccess(
                    RandomAccessPoint::AvcIdr | RandomAccessPoint::HevcIdr,
                ) => true,
                PictureKind::Trailing => false,
                _ => {
                    return Err(RecordingError::InvalidInput(
                        "segment requires a closed IDR sequence",
                    ))
                }
            };
            if self.last_pts_us.is_none() && (!sync || pts_us != 0) {
                return Err(RecordingError::InvalidInput(
                    "segment must start at zero with an IDR",
                ));
            }
            // SAFETY: The adapter serializes access to these retained native objects.
            unsafe {
                if self.writer.status() != AVAssetWriterStatus::Writing {
                    return Err(writer_error(&self.writer));
                }
                if !self.input.isReadyForMoreMediaData() {
                    return Ok(AppendOutcome::Busy);
                }
                let sample = sample::create(data, &self.format, pts_us, self.fps, sync)?;
                if !self.input.appendSampleBuffer(&sample) {
                    return Err(writer_error(&self.writer));
                }
            }
            self.last_pts_us = Some(pts_us);
            Ok(AppendOutcome::Written)
        })
    }

    /// Wait only on the dedicated writer thread. Failure never grants promotion.
    pub fn finish(mut self) -> Result<FinalizedSegment, RecordingError> {
        let result = self.finish_native();
        if let Err(error) = &result {
            if let Err(preservation) = self.cancel_preserving_partial() {
                return Err(RecordingError::Platform(format!(
                    "{error}; partial preservation: {preservation}"
                )));
            }
        }
        result
    }

    fn finish_native(&mut self) -> Result<FinalizedSegment, RecordingError> {
        if self.last_pts_us.is_none() {
            return Err(RecordingError::InvalidInput("empty segment"));
        }
        // SAFETY: A cancelled/failed writer cannot accept finishWriting.
        if unsafe { self.writer.status() } != AVAssetWriterStatus::Writing {
            return Err(writer_error(&self.writer));
        }
        let (sender, receiver) = mpsc::sync_channel(1);
        let completion = block2::RcBlock::new(move || {
            let _ = sender.try_send(());
        });
        // SAFETY: All appends have returned; the block owns its thread-safe sender.
        // The adapter keeps both native objects alive until completion/cancellation.
        unsafe {
            self.input.markAsFinished();
            self.writer.finishWritingWithCompletionHandler(&completion);
        }
        receiver
            .recv_timeout(Duration::from_secs(10))
            .map_err(|_| RecordingError::FinalizationTimeout)?;
        // SAFETY: Completion has fired and the retained writer remains alive.
        if unsafe { self.writer.status() } != AVAssetWriterStatus::Completed {
            return Err(writer_error(&self.writer));
        }
        Ok(FinalizedSegment {
            path: self.path.clone(),
        })
    }
    /// Explicit failure cleanup on the recording worker. Never promotes a file.
    pub fn cancel_preserving_partial(&mut self) -> Result<(), RecordingError> {
        // SAFETY: This owner serializes appends and finalization. Native cancel
        // stops writing before the retained file is copied to a missing path.
        unsafe {
            if self.writer.status() == AVAssetWriterStatus::Writing {
                self.writer.cancelWriting();
            }
            if self.writer.status() != AVAssetWriterStatus::Completed {
                self.partial.restore()?;
            }
        }
        Ok(())
    }
}

impl Drop for AppleSegment {
    fn drop(&mut self) {
        // Explicit finish/cancel report errors; Drop is best-effort cleanup.
        let _ = self.cancel_preserving_partial();
    }
}

fn writer_error(writer: &AVAssetWriter) -> RecordingError {
    // SAFETY: Reading the status of a live retained native writer is supported.
    RecordingError::Platform(
        unsafe { writer.error() }
            .map(|error| error.localizedDescription().to_string())
            .unwrap_or_else(|| "native writer rejected the operation".into()),
    )
}

#[cfg(test)]
pub(crate) mod tests;
