//! Media Foundation H.264 decoder — REQ-PICOO-MEDIA-005.
//!
//! CMSH264DecoderMFT IMFTransform pipeline: H.264 access unit → NV12.
//! Falls back to [`StubDecoder`] for short test AUs or MF errors.

use std::mem::ManuallyDrop;

use bytes::Bytes;
use picoo_protocol::control::StreamConfig;
use windows::Win32::Media::MediaFoundation::{
    CMSH264DecoderMFT, IMFMediaBuffer, IMFSample, IMFTransform, MFCreateMediaType,
    MFCreateMemoryBuffer, MFCreateSample, MFMediaType_Video, MFVideoFormat_H264,
    MFVideoFormat_NV12, MFVideoInterlace_Progressive, MFT_MESSAGE_COMMAND_FLUSH,
    MFT_MESSAGE_NOTIFY_BEGIN_STREAMING, MFT_MESSAGE_NOTIFY_START_OF_STREAM, MFT_OUTPUT_DATA_BUFFER,
    MF_E_NOTACCEPTING, MF_E_TRANSFORM_NEED_MORE_INPUT, MF_MT_FRAME_SIZE, MF_MT_INTERLACE_MODE,
    MF_MT_MAJOR_TYPE, MF_MT_SUBTYPE,
};
use windows::Win32::System::Com::{
    CoCreateInstance, CoInitializeEx, CLSCTX_INPROC_SERVER, COINIT_MULTITHREADED,
};

use crate::{now_timestamp_us, AccessUnitDecoder, DecodeError, DecodedFrame, StubDecoder};

const DEFAULT_WIDTH: u32 = 1280;
const DEFAULT_HEIGHT: u32 = 720;
const MIN_H264_AU_BYTES: usize = 65;

pub struct MfH264Decoder {
    transform: IMFTransform,
    configured: bool,
    width: u32,
    height: u32,
    fallback: StubDecoder,
}

impl MfH264Decoder {
    pub fn new() -> Result<Self, DecodeError> {
        unsafe {
            CoInitializeEx(None, COINIT_MULTITHREADED)
                .ok()
                .map_err(|e| DecodeError::Platform(format!("CoInitializeEx: {e}")))?;
            windows::Win32::Media::MediaFoundation::MFStartup(
                windows::Win32::Media::MediaFoundation::MF_VERSION,
                Default::default(),
            )
            .map_err(|e| DecodeError::Platform(format!("MFStartup: {e}")))?;
        }

        let transform: IMFTransform =
            unsafe { CoCreateInstance(&CMSH264DecoderMFT, None, CLSCTX_INPROC_SERVER) }
                .map_err(|e| DecodeError::Platform(format!("CoCreateInstance H264 MFT: {e}")))?;

        Ok(Self {
            transform,
            configured: false,
            width: DEFAULT_WIDTH,
            height: DEFAULT_HEIGHT,
            fallback: StubDecoder::new(),
        })
    }

    fn dimensions(stream_config: Option<&StreamConfig>) -> (u32, u32) {
        stream_config
            .map(|cfg| (cfg.width, cfg.height))
            .filter(|(w, h)| *w > 0 && *h > 0)
            .unwrap_or((DEFAULT_WIDTH, DEFAULT_HEIGHT))
    }

    fn ensure_configured(
        &mut self,
        stream_config: Option<&StreamConfig>,
    ) -> Result<(), DecodeError> {
        let (width, height) = Self::dimensions(stream_config);
        if self.configured && self.width == width && self.height == height {
            return Ok(());
        }

        unsafe {
            configure_transform(&self.transform, width, height)?;
        }
        self.configured = true;
        self.width = width;
        self.height = height;
        Ok(())
    }

    fn decode_h264_au(
        &mut self,
        access_unit: &[u8],
        stream_config: Option<&StreamConfig>,
    ) -> Result<Option<DecodedFrame>, DecodeError> {
        self.ensure_configured(stream_config)?;
        unsafe {
            feed_access_unit(&self.transform, access_unit)?;
            drain_output(&self.transform, self.width, self.height)
        }
    }
}

impl Drop for MfH264Decoder {
    fn drop(&mut self) {
        unsafe {
            let _ = windows::Win32::Media::MediaFoundation::MFShutdown();
        }
    }
}

impl AccessUnitDecoder for MfH264Decoder {
    fn decode_access_unit(
        &mut self,
        access_unit: &[u8],
        stream_config: Option<&StreamConfig>,
    ) -> Result<Option<DecodedFrame>, DecodeError> {
        if access_unit.len() < MIN_H264_AU_BYTES {
            return self.fallback.decode_access_unit(access_unit, stream_config);
        }

        match self.decode_h264_au(access_unit, stream_config) {
            Ok(frame) => Ok(frame),
            Err(err) => {
                tracing::warn!("MF decode failed, using stub placeholder: {err}");
                self.fallback.decode_access_unit(access_unit, stream_config)
            }
        }
    }
}

fn pack_frame_size(width: u32, height: u32) -> u64 {
    ((width as u64) << 32) | height as u64
}

unsafe fn configure_transform(
    transform: &IMFTransform,
    width: u32,
    height: u32,
) -> Result<(), DecodeError> {
    let in_type = MFCreateMediaType()
        .map_err(|e| DecodeError::Platform(format!("MFCreateMediaType input: {e}")))?;
    in_type
        .SetGUID(&MF_MT_MAJOR_TYPE, &MFMediaType_Video)
        .map_err(|e| DecodeError::Platform(format!("input major type: {e}")))?;
    in_type
        .SetGUID(&MF_MT_SUBTYPE, &MFVideoFormat_H264)
        .map_err(|e| DecodeError::Platform(format!("input subtype: {e}")))?;
    transform
        .SetInputType(0, &in_type, 0)
        .map_err(|e| DecodeError::Platform(format!("SetInputType: {e}")))?;

    let out_type = MFCreateMediaType()
        .map_err(|e| DecodeError::Platform(format!("MFCreateMediaType output: {e}")))?;
    out_type
        .SetGUID(&MF_MT_MAJOR_TYPE, &MFMediaType_Video)
        .map_err(|e| DecodeError::Platform(format!("output major type: {e}")))?;
    out_type
        .SetGUID(&MF_MT_SUBTYPE, &MFVideoFormat_NV12)
        .map_err(|e| DecodeError::Platform(format!("output subtype: {e}")))?;
    out_type
        .SetUINT64(&MF_MT_FRAME_SIZE, pack_frame_size(width, height))
        .map_err(|e| DecodeError::Platform(format!("output frame size: {e}")))?;
    out_type
        .SetUINT32(&MF_MT_INTERLACE_MODE, MFVideoInterlace_Progressive.0 as u32)
        .map_err(|e| DecodeError::Platform(format!("output interlace: {e}")))?;
    transform
        .SetOutputType(0, &out_type, 0)
        .map_err(|e| DecodeError::Platform(format!("SetOutputType: {e}")))?;

    transform
        .ProcessMessage(MFT_MESSAGE_COMMAND_FLUSH, 0)
        .map_err(|e| DecodeError::Platform(format!("MFT flush: {e}")))?;
    transform
        .ProcessMessage(MFT_MESSAGE_NOTIFY_BEGIN_STREAMING, 0)
        .map_err(|e| DecodeError::Platform(format!("MFT begin streaming: {e}")))?;
    transform
        .ProcessMessage(MFT_MESSAGE_NOTIFY_START_OF_STREAM, 0)
        .map_err(|e| DecodeError::Platform(format!("MFT start of stream: {e}")))?;

    Ok(())
}

unsafe fn create_input_sample(data: &[u8]) -> Result<IMFSample, DecodeError> {
    let buffer = MFCreateMemoryBuffer(data.len() as u32)
        .map_err(|e| DecodeError::Platform(format!("MFCreateMemoryBuffer: {e}")))?;

    let mut dest: *mut u8 = std::ptr::null_mut();
    let mut max_len = 0u32;
    let mut current_len = 0u32;
    buffer
        .Lock(&mut dest, Some(&mut max_len), Some(&mut current_len))
        .map_err(|e| DecodeError::Platform(format!("buffer lock: {e}")))?;
    if dest.is_null() || max_len < data.len() as u32 {
        let _ = buffer.Unlock();
        return Err(DecodeError::Platform("input buffer too small".into()));
    }
    std::ptr::copy_nonoverlapping(data.as_ptr(), dest, data.len());
    buffer
        .Unlock()
        .map_err(|e| DecodeError::Platform(format!("buffer unlock: {e}")))?;
    buffer
        .SetCurrentLength(data.len() as u32)
        .map_err(|e| DecodeError::Platform(format!("SetCurrentLength: {e}")))?;

    let sample =
        MFCreateSample().map_err(|e| DecodeError::Platform(format!("MFCreateSample: {e}")))?;
    sample
        .AddBuffer(&buffer)
        .map_err(|e| DecodeError::Platform(format!("AddBuffer: {e}")))?;
    Ok(sample)
}

unsafe fn feed_access_unit(
    transform: &IMFTransform,
    access_unit: &[u8],
) -> Result<(), DecodeError> {
    let sample = create_input_sample(access_unit)?;

    match transform.ProcessInput(0, &sample, 0) {
        Ok(()) => Ok(()),
        Err(e) if e.code() == MF_E_NOTACCEPTING => {
            // Drain pending output then retry once.
            let _ = drain_output(transform, DEFAULT_WIDTH, DEFAULT_HEIGHT)?;
            transform
                .ProcessInput(0, &sample, 0)
                .map_err(|e| DecodeError::Platform(format!("ProcessInput retry: {e}")))?;
            Ok(())
        }
        Err(e) => Err(DecodeError::Platform(format!("ProcessInput: {e}"))),
    }
}

unsafe fn copy_buffer_to_bytes(buffer: &IMFMediaBuffer) -> Result<Vec<u8>, DecodeError> {
    let mut dest: *mut u8 = std::ptr::null_mut();
    let mut max_len = 0u32;
    let mut current_len = 0u32;
    buffer
        .Lock(&mut dest, Some(&mut max_len), Some(&mut current_len))
        .map_err(|e| DecodeError::Platform(format!("output lock: {e}")))?;
    if dest.is_null() {
        let _ = buffer.Unlock();
        return Err(DecodeError::Platform("null output buffer".into()));
    }
    let slice = std::slice::from_raw_parts(dest, current_len as usize);
    let out = slice.to_vec();
    buffer
        .Unlock()
        .map_err(|e| DecodeError::Platform(format!("output unlock: {e}")))?;
    Ok(out)
}

unsafe fn sample_to_frame(
    sample: &IMFSample,
    width: u32,
    height: u32,
) -> Result<DecodedFrame, DecodeError> {
    let buffer = sample
        .ConvertToContiguousBuffer()
        .map_err(|e| DecodeError::Platform(format!("ConvertToContiguousBuffer: {e}")))?;
    let nv12 = copy_buffer_to_bytes(&buffer)?;
    if nv12.is_empty() {
        return Err(DecodeError::UnsupportedAccessUnit);
    }

    Ok(DecodedFrame {
        width,
        height,
        stride: width,
        rotation: 0,
        timestamp_us: now_timestamp_us(),
        nv12: Bytes::from(nv12),
    })
}

unsafe fn drain_output(
    transform: &IMFTransform,
    width: u32,
    height: u32,
) -> Result<Option<DecodedFrame>, DecodeError> {
    loop {
        let mut output_buffer = MFT_OUTPUT_DATA_BUFFER {
            dwStreamID: 0,
            pSample: ManuallyDrop::new(None),
            dwStatus: 0,
            pEvents: ManuallyDrop::new(None),
        };

        let mut status = 0u32;
        match transform.ProcessOutput(0, std::slice::from_mut(&mut output_buffer), &mut status) {
            Ok(()) => {
                let sample = ManuallyDrop::take(&mut output_buffer.pSample);
                if let Some(sample) = sample {
                    return sample_to_frame(&sample, width, height).map(Some);
                }
                return Ok(None);
            }
            Err(e) if e.code() == MF_E_TRANSFORM_NEED_MORE_INPUT => return Ok(None),
            Err(e) => {
                return Err(DecodeError::Platform(format!("ProcessOutput: {e}")));
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn pack_frame_size_matches_mf_convention() {
        assert_eq!(pack_frame_size(1280, 720), (1280u64 << 32) | 720);
    }
}
