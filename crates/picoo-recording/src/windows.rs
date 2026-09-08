//! MF compressed passthrough on the recording worker — REQ-PICOO-MEDIA-080.
use crate::{AppendOutcome, FinalizedSegment, RecordingError};
use ::windows::{
    core::HSTRING,
    Win32::{Media::MediaFoundation::*, System::Com::*},
};
use picoo_bitstream::{
    AccessUnit, Codec, CodecConfiguration, NalFormat, NalLengthSize, PictureKind, RandomAccessPoint,
};
use std::{
    marker::PhantomData,
    path::{Path, PathBuf},
    ptr,
    rc::Rc,
    sync::mpsc,
    time::Duration,
};
mod callback;

struct Runtime(PhantomData<Rc<()>>);
impl Runtime {
    fn new() -> Result<Self, RecordingError> {
        unsafe {
            platform(CoInitializeEx(None, COINIT_MULTITHREADED).ok())?;
            if let Err(error) = MFStartup(MF_VERSION, MFSTARTUP_FULL) {
                CoUninitialize();
                return Err(native(error));
            }
        }
        Ok(Self(PhantomData))
    }
}
impl Drop for Runtime {
    fn drop(&mut self) {
        unsafe {
            let _ = MFShutdown();
            CoUninitialize();
        }
    }
}

/// One immutable codec configuration and independently decodable file. Not Send:
/// native COM/MF creation and cleanup must stay on the recording worker.
pub struct WindowsSegment {
    writer: Option<IMFSinkWriter>,
    sink: Option<IMFMediaSink>,
    stream: Option<IMFByteStream>,
    media: Option<IMFMediaType>,
    events: mpsc::Receiver<callback::Event>,
    path: PathBuf,
    configuration: CodecConfiguration,
    fps: u32,
    last_pts_us: Option<u64>,
    failed: bool,
    // Last: release every native owner before shutting down its runtime.
    _runtime: Runtime,
}

impl WindowsSegment {
    pub fn new(
        path: &Path,
        configuration: CodecConfiguration,
        fps: u32,
    ) -> Result<Self, RecordingError> {
        if !matches!(fps, 30 | 60)
            || configuration.nal_length_size() != NalLengthSize::Four
            || configuration.record().len() > 64 * 1024
        {
            return Err(RecordingError::InvalidInput("unsupported segment format"));
        }
        let facts = configuration
            .source_facts()
            .map_err(|_| RecordingError::InvalidInput("invalid segment configuration"))?;
        let color = facts
            .color
            .ok_or(RecordingError::InvalidInput("missing segment color"))?;
        if (
            color.primaries,
            color.transfer,
            color.matrix,
            color.full_range,
        ) != (1, 1, 1, false)
        {
            return Err(RecordingError::InvalidInput("unsupported segment color"));
        }
        let runtime = Runtime::new()?;
        unsafe {
            let media = platform(MFCreateMediaType())?;
            platform(media.SetGUID(&MF_MT_MAJOR_TYPE, &MFMediaType_Video))?;
            platform(media.SetGUID(
                &MF_MT_SUBTYPE,
                match configuration.codec() {
                    Codec::Avc => &MFVideoFormat_H264,
                    Codec::Hevc => &MFVideoFormat_HEVC,
                },
            ))?;
            platform(media.SetUINT64(
                &MF_MT_FRAME_SIZE,
                (u64::from(facts.visible_width) << 32) | u64::from(facts.visible_height),
            ))?;
            platform(media.SetUINT64(&MF_MT_FRAME_RATE, (u64::from(fps) << 32) | 1))?;
            platform(media.SetUINT64(&MF_MT_PIXEL_ASPECT_RATIO, (1_u64 << 32) | 1))?;
            platform(
                media.SetUINT32(&MF_MT_INTERLACE_MODE, MFVideoInterlace_Progressive.0 as u32),
            )?;
            let mut sequence = Vec::new();
            for nal in configuration
                .vps()
                .iter()
                .chain(configuration.sps())
                .chain(configuration.pps())
            {
                sequence.extend_from_slice(&[0, 0, 0, 1]);
                sequence.extend_from_slice(nal);
            }
            platform(media.SetBlob(&MF_MT_MPEG_SEQUENCE_HEADER, &sequence))?;
            let stream = platform(MFCreateFile(
                MF_ACCESSMODE_READWRITE,
                MF_OPENMODE_FAIL_IF_EXIST,
                MF_FILEFLAGS_NONE,
                &HSTRING::from(path.as_os_str()),
            ))?;
            let sink = platform(match configuration.codec() {
                Codec::Avc => MFCreateFMPEG4MediaSink(&stream, &media, None),
                Codec::Hevc => MFCreateMPEG4MediaSink(&stream, &media, None),
            })?;
            let (send, events) = mpsc::sync_channel(2);
            // Install the cleanup owner before any operation that may fail.
            let mut segment = Self {
                writer: None,
                sink: Some(sink),
                stream: Some(stream),
                media: Some(media),
                events,
                path: path.to_owned(),
                configuration,
                fps,
                last_pts_us: None,
                failed: false,
                _runtime: runtime,
            };
            let callback: IMFSinkWriterCallback = callback::Completion { send }.into();
            let mut attributes = None;
            platform(MFCreateAttributes(&mut attributes, 2))?;
            let attributes =
                attributes.ok_or(RecordingError::InvalidInput("missing writer attributes"))?;
            platform(attributes.SetUnknown(&MF_SINK_WRITER_ASYNC_CALLBACK, &callback))?;
            platform(attributes.SetUINT32(&MF_SINK_WRITER_DISABLE_THROTTLING, 1))?;
            let writer = platform(MFCreateSinkWriterFromMediaSink(
                segment.sink.as_ref().unwrap(),
                &attributes,
            ))?;
            segment.writer = Some(writer);
            let writer = segment.writer.as_ref().unwrap();
            platform(writer.SetInputMediaType(0, segment.media.as_ref().unwrap(), None))?;
            platform(writer.BeginWriting())?;
            Ok(segment)
        }
    }

    pub fn append(&mut self, data: &[u8], pts_us: u64) -> Result<AppendOutcome, RecordingError> {
        if self.failed {
            return Err(RecordingError::InvalidInput("segment already failed"));
        }
        let result = self.append_inner(data, pts_us);
        self.failed = result.is_err();
        if result.is_ok() {
            self.last_pts_us = Some(pts_us);
        }
        result
    }

    fn append_inner(&mut self, data: &[u8], pts_us: u64) -> Result<AppendOutcome, RecordingError> {
        if data.is_empty()
            || data.len() > 2 * 1024 * 1024
            || self.last_pts_us.is_some_and(|previous| pts_us <= previous)
        {
            return Err(RecordingError::InvalidInput(
                "invalid segment AU or timestamp",
            ));
        }
        let picture = AccessUnit::parse(
            self.configuration.codec(),
            NalFormat::LengthPrefixed(NalLengthSize::Four),
            data,
        )
        .map_err(|_| RecordingError::InvalidInput("invalid segment AU"))?;
        self.configuration
            .validate_parameter_sets(&picture)
            .map_err(|_| RecordingError::InvalidInput("segment parameters changed"))?;
        let sync = match picture.picture().kind {
            PictureKind::RandomAccess(RandomAccessPoint::AvcIdr | RandomAccessPoint::HevcIdr) => {
                true
            }
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
        let timestamp = pts_us
            .checked_mul(10)
            .and_then(|value| i64::try_from(value).ok())
            .ok_or(RecordingError::InvalidInput("segment timestamp overflow"))?;
        let payload = picture
            .to_annex_b()
            .map_err(|_| RecordingError::InvalidInput("invalid segment NAL representation"))?;
        unsafe {
            let sample = platform(MFCreateSample())?;
            let buffer = platform(MFCreateMemoryBuffer(payload.len() as u32))?;
            let mut destination = ptr::null_mut();
            platform(buffer.Lock(&mut destination, None, None))?;
            if destination.is_null() {
                let _ = buffer.Unlock();
                return Err(RecordingError::InvalidInput("null native sample storage"));
            }
            ptr::copy_nonoverlapping(payload.as_ptr(), destination, payload.len());
            platform(buffer.Unlock())?;
            platform(buffer.SetCurrentLength(payload.len() as u32))?;
            platform(sample.AddBuffer(&buffer))?;
            platform(sample.SetSampleTime(timestamp))?;
            platform(sample.SetSampleDuration(10_000_000 / i64::from(self.fps)))?;
            platform(sample.SetUINT32(&MFSampleExtension_CleanPoint, u32::from(sync)))?;
            let writer = self.writer.as_ref().unwrap();
            platform(writer.WriteSample(0, &sample))?;
            platform(writer.PlaceMarker(0, ptr::null()))?;
        }
        match self.events.recv_timeout(Duration::from_millis(250)) {
            Ok(callback::Event::Marker) => Ok(AppendOutcome::Written),
            _ => Err(RecordingError::Platform(
                "native sample confirmation failed or timed out".into(),
            )),
        }
    }

    pub fn finish(self) -> Result<FinalizedSegment, RecordingError> {
        if self.failed || self.last_pts_us.is_none() {
            return Err(RecordingError::InvalidInput(
                "cannot finalize failed or empty segment",
            ));
        }
        unsafe {
            platform(self.writer.as_ref().unwrap().Finalize())?;
        }
        match self.events.recv_timeout(Duration::from_secs(10)) {
            Ok(callback::Event::Finalized(result)) => platform(result.ok())?,
            _ => return Err(RecordingError::FinalizationTimeout),
        }
        Ok(FinalizedSegment {
            path: self.path.clone(),
        })
    }
}

impl Drop for WindowsSegment {
    fn drop(&mut self) {
        drop(self.writer.take());
        if let Some(sink) = self.sink.take() {
            unsafe {
                let _ = sink.Shutdown();
            }
        }
        drop(self.stream.take());
        drop(self.media.take());
    }
}

fn native(error: ::windows::core::Error) -> RecordingError {
    RecordingError::Platform(error.to_string())
}
fn platform<T>(result: ::windows::core::Result<T>) -> Result<T, RecordingError> {
    result.map_err(native)
}
