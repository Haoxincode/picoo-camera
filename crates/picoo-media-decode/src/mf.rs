//! Media Foundation H.264 decoder — REQ-PICOO-MEDIA-005.
//!
//! CMSH264DecoderMFT IMFTransform pipeline: H.264 access unit → NV12.
//! When StreamConfig carries SPS/PPS, they are applied as
//! `MF_MT_MPEG_SEQUENCE_HEADER` and injected ahead of the first AU after
//! (re)configure — REQ-PICOO-PROTOCOL-005 / REQ-PICOO-SESSION-004.

use std::mem::ManuallyDrop;

use bytes::Bytes;
use picoo_packet::{access_unit_to_annex_b, annex_b_parameter_sets};
use picoo_protocol::control::StreamConfig;
use windows::core::{GUID, HRESULT};
use windows::Win32::Foundation::RPC_E_CHANGED_MODE;
use windows::Win32::Media::MediaFoundation::{
    CMSH264DecoderMFT, IMFMediaBuffer, IMFSample, IMFTransform, MFCreateAlignedMemoryBuffer,
    MFCreateMediaType, MFCreateMemoryBuffer, MFCreateSample, MFMediaType_Video, MFVideoFormat_H264,
    MFVideoFormat_NV12, MFVideoInterlace_Progressive, MFT_MESSAGE_COMMAND_FLUSH,
    MFT_MESSAGE_NOTIFY_BEGIN_STREAMING, MFT_MESSAGE_NOTIFY_START_OF_STREAM, MFT_OUTPUT_DATA_BUFFER,
    MFT_OUTPUT_STREAM_CAN_PROVIDE_SAMPLES, MFT_OUTPUT_STREAM_PROVIDES_SAMPLES, MF_E_NOTACCEPTING,
    MF_E_TRANSFORM_NEED_MORE_INPUT, MF_LOW_LATENCY, MF_MT_FRAME_RATE, MF_MT_FRAME_SIZE,
    MF_MT_INTERLACE_MODE, MF_MT_MAJOR_TYPE, MF_MT_PIXEL_ASPECT_RATIO, MF_MT_SUBTYPE,
};
use windows::Win32::System::Com::{
    CoCreateInstance, CoInitializeEx, CoUninitialize, CLSCTX_INPROC_SERVER, COINIT_MULTITHREADED,
};

use crate::{now_timestamp_us, AccessUnitDecoder, DecodeError, DecodedFrame};

const DEFAULT_WIDTH: u32 = 1280;
const DEFAULT_HEIGHT: u32 = 720;
const DEFAULT_FPS: u32 = 30;
/// MF_MT_MPEG_SEQUENCE_HEADER — H.264 SPS/PPS with Annex-B start codes.
const MF_MT_MPEG_SEQUENCE_HEADER: GUID = GUID::from_u128(0x05f4_6766_f1a9_44e5_b82a_e4df_c2ea_2873);

pub struct MfH264Decoder {
    transform: IMFTransform,
    configured: bool,
    width: u32,
    height: u32,
    fps: u32,
    next_sample_time_100ns: i64,
    sequence_header: Vec<u8>,
    inject_sequence_header: bool,
    // Declared last so the transform is released before MFShutdown/CoUninitialize.
    _runtime: MfRuntimeGuard,
}

// IMFTransform is not automatically Send in windows-rs; receiver owns the decoder on one thread.
unsafe impl Send for MfH264Decoder {}

struct MfRuntimeGuard {
    owns_com_apartment: bool,
}

impl MfRuntimeGuard {
    fn start() -> Result<Self, DecodeError> {
        let com_result = unsafe { CoInitializeEx(None, COINIT_MULTITHREADED) };
        let owns_com_apartment = com_initialization_ownership(com_result)?;
        if let Err(error) = unsafe {
            windows::Win32::Media::MediaFoundation::MFStartup(
                windows::Win32::Media::MediaFoundation::MF_VERSION,
                Default::default(),
            )
        } {
            if owns_com_apartment {
                unsafe { CoUninitialize() };
            }
            return Err(DecodeError::Platform(format!("MFStartup: {error}")));
        }
        Ok(Self { owns_com_apartment })
    }
}

impl Drop for MfRuntimeGuard {
    fn drop(&mut self) {
        unsafe {
            let _ = windows::Win32::Media::MediaFoundation::MFShutdown();
            if self.owns_com_apartment {
                CoUninitialize();
            }
        }
    }
}

fn com_initialization_ownership(result: HRESULT) -> Result<bool, DecodeError> {
    if result.is_ok() {
        // S_OK and S_FALSE both require a matching CoUninitialize.
        Ok(true)
    } else if result == RPC_E_CHANGED_MODE {
        // GPUI initializes OLE/STA on its UI thread. MF's synchronous decoder
        // can use that existing apartment; do not replace or uninitialize it.
        Ok(false)
    } else {
        Err(DecodeError::Platform(format!(
            "CoInitializeEx: {}",
            result.message()
        )))
    }
}

impl MfH264Decoder {
    pub fn new() -> Result<Self, DecodeError> {
        let runtime = MfRuntimeGuard::start()?;

        let transform: IMFTransform =
            unsafe { CoCreateInstance(&CMSH264DecoderMFT, None, CLSCTX_INPROC_SERVER) }
                .map_err(|e| DecodeError::Platform(format!("CoCreateInstance H264 MFT: {e}")))?;
        unsafe {
            transform
                .GetAttributes()
                .and_then(|attributes| attributes.SetUINT32(&MF_LOW_LATENCY, 1))
                .map_err(|e| DecodeError::Platform(format!("enable MF low latency: {e}")))?;
        }

        Ok(Self {
            transform,
            configured: false,
            width: DEFAULT_WIDTH,
            height: DEFAULT_HEIGHT,
            fps: DEFAULT_FPS,
            next_sample_time_100ns: 0,
            sequence_header: Vec::new(),
            inject_sequence_header: false,
            _runtime: runtime,
        })
    }

    fn stream_shape(stream_config: Option<&StreamConfig>) -> (u32, u32, u32) {
        stream_config
            .map(|cfg| (cfg.width, cfg.height, cfg.fps))
            .filter(|(w, h, _)| *w > 0 && *h > 0)
            .map(|(w, h, fps)| (w, h, fps.max(1)))
            .unwrap_or((DEFAULT_WIDTH, DEFAULT_HEIGHT, DEFAULT_FPS))
    }

    fn sequence_header_from_config(stream_config: Option<&StreamConfig>) -> Vec<u8> {
        stream_config
            .filter(|cfg| !cfg.sps.is_empty() && !cfg.pps.is_empty())
            .map(|cfg| annex_b_parameter_sets(&cfg.sps, &cfg.pps))
            .unwrap_or_default()
    }

    fn ensure_configured(
        &mut self,
        stream_config: Option<&StreamConfig>,
    ) -> Result<(), DecodeError> {
        let (width, height, fps) = Self::stream_shape(stream_config);
        let sequence_header = Self::sequence_header_from_config(stream_config);
        if self.configured
            && self.width == width
            && self.height == height
            && self.fps == fps
            && self.sequence_header == sequence_header
        {
            return Ok(());
        }

        unsafe {
            configure_transform(
                &self.transform,
                width,
                height,
                fps,
                sequence_header.as_slice(),
            )?;
        }
        self.configured = true;
        self.width = width;
        self.height = height;
        self.fps = fps;
        self.next_sample_time_100ns = 0;
        self.inject_sequence_header = !sequence_header.is_empty();
        self.sequence_header = sequence_header;
        Ok(())
    }

    fn decode_h264_au(
        &mut self,
        access_unit: &[u8],
        stream_config: Option<&StreamConfig>,
    ) -> Result<Option<DecodedFrame>, DecodeError> {
        self.ensure_configured(stream_config)?;
        // Android MediaCodec commonly emits length-prefixed AUs; MF expects Annex-B.
        let annex = access_unit_to_annex_b(access_unit);
        let access_unit = annex.as_ref();
        let owned;
        let payload = if self.inject_sequence_header && !self.sequence_header.is_empty() {
            self.inject_sequence_header = false;
            let mut combined = Vec::with_capacity(self.sequence_header.len() + access_unit.len());
            combined.extend_from_slice(&self.sequence_header);
            combined.extend_from_slice(access_unit);
            owned = combined;
            owned.as_slice()
        } else {
            access_unit
        };
        unsafe {
            let duration_100ns = 10_000_000i64 / i64::from(self.fps.max(1));
            let pending = feed_access_unit(
                &self.transform,
                payload,
                self.next_sample_time_100ns,
                duration_100ns,
                self.width,
                self.height,
            )?;
            self.next_sample_time_100ns += duration_100ns;
            match pending {
                Some(frame) => Ok(Some(frame)),
                None => drain_output(&self.transform, self.width, self.height),
            }
        }
    }
}

impl AccessUnitDecoder for MfH264Decoder {
    fn decode_access_unit(
        &mut self,
        access_unit: &[u8],
        stream_config: Option<&StreamConfig>,
    ) -> Result<Option<DecodedFrame>, DecodeError> {
        self.decode_h264_au(access_unit, stream_config)
    }
}

fn pack_frame_size(width: u32, height: u32) -> u64 {
    ((width as u64) << 32) | height as u64
}

unsafe fn configure_transform(
    transform: &IMFTransform,
    width: u32,
    height: u32,
    fps: u32,
    sequence_header: &[u8],
) -> Result<(), DecodeError> {
    let in_type = MFCreateMediaType()
        .map_err(|e| DecodeError::Platform(format!("MFCreateMediaType input: {e}")))?;
    in_type
        .SetGUID(&MF_MT_MAJOR_TYPE, &MFMediaType_Video)
        .map_err(|e| DecodeError::Platform(format!("input major type: {e}")))?;
    in_type
        .SetGUID(&MF_MT_SUBTYPE, &MFVideoFormat_H264)
        .map_err(|e| DecodeError::Platform(format!("input subtype: {e}")))?;
    in_type
        .SetUINT64(&MF_MT_FRAME_SIZE, pack_frame_size(width, height))
        .map_err(|e| DecodeError::Platform(format!("input frame size: {e}")))?;
    in_type
        .SetUINT64(&MF_MT_FRAME_RATE, pack_frame_size(fps, 1))
        .map_err(|e| DecodeError::Platform(format!("input frame rate: {e}")))?;
    in_type
        .SetUINT32(&MF_MT_INTERLACE_MODE, MFVideoInterlace_Progressive.0 as u32)
        .map_err(|e| DecodeError::Platform(format!("input interlace: {e}")))?;
    in_type
        .SetUINT64(&MF_MT_PIXEL_ASPECT_RATIO, pack_frame_size(1, 1))
        .map_err(|e| DecodeError::Platform(format!("input pixel aspect: {e}")))?;
    if !sequence_header.is_empty() {
        in_type
            .SetBlob(&MF_MT_MPEG_SEQUENCE_HEADER, sequence_header)
            .map_err(|e| DecodeError::Platform(format!("sequence header blob: {e}")))?;
    }
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
        .SetUINT64(&MF_MT_FRAME_RATE, pack_frame_size(fps, 1))
        .map_err(|e| DecodeError::Platform(format!("output frame rate: {e}")))?;
    out_type
        .SetUINT32(&MF_MT_INTERLACE_MODE, MFVideoInterlace_Progressive.0 as u32)
        .map_err(|e| DecodeError::Platform(format!("output interlace: {e}")))?;
    out_type
        .SetUINT64(&MF_MT_PIXEL_ASPECT_RATIO, pack_frame_size(1, 1))
        .map_err(|e| DecodeError::Platform(format!("output pixel aspect: {e}")))?;
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

unsafe fn create_input_sample(
    data: &[u8],
    sample_time_100ns: i64,
    duration_100ns: i64,
) -> Result<IMFSample, DecodeError> {
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
    sample
        .SetSampleTime(sample_time_100ns)
        .map_err(|e| DecodeError::Platform(format!("SetSampleTime: {e}")))?;
    sample
        .SetSampleDuration(duration_100ns)
        .map_err(|e| DecodeError::Platform(format!("SetSampleDuration: {e}")))?;
    Ok(sample)
}

unsafe fn feed_access_unit(
    transform: &IMFTransform,
    access_unit: &[u8],
    sample_time_100ns: i64,
    duration_100ns: i64,
    width: u32,
    height: u32,
) -> Result<Option<DecodedFrame>, DecodeError> {
    let sample = create_input_sample(access_unit, sample_time_100ns, duration_100ns)?;

    match transform.ProcessInput(0, &sample, 0) {
        Ok(()) => Ok(None),
        Err(e) if e.code() == MF_E_NOTACCEPTING => {
            // Drain pending output then retry once.
            let pending = drain_output(transform, width, height)?;
            transform
                .ProcessInput(0, &sample, 0)
                .map_err(|e| DecodeError::Platform(format!("ProcessInput retry: {e}")))?;
            Ok(pending)
        }
        Err(e) => Err(DecodeError::Platform(format!("ProcessInput: {e}"))),
    }
}

unsafe fn output_sample_for_transform(
    transform: &IMFTransform,
) -> Result<Option<IMFSample>, DecodeError> {
    let info = transform
        .GetOutputStreamInfo(0)
        .map_err(|e| DecodeError::Platform(format!("GetOutputStreamInfo: {e}")))?;
    let provides_samples = info.dwFlags & MFT_OUTPUT_STREAM_PROVIDES_SAMPLES.0 as u32 != 0;
    let can_provide_samples = info.dwFlags & MFT_OUTPUT_STREAM_CAN_PROVIDE_SAMPLES.0 as u32 != 0;
    if provides_samples || can_provide_samples {
        return Ok(None);
    }
    if info.cbSize == 0 {
        return Err(DecodeError::Platform(
            "output stream requires a caller sample but reported cbSize=0".into(),
        ));
    }

    let buffer = MFCreateAlignedMemoryBuffer(info.cbSize, info.cbAlignment)
        .map_err(|e| DecodeError::Platform(format!("MFCreateAlignedMemoryBuffer: {e}")))?;
    let sample =
        MFCreateSample().map_err(|e| DecodeError::Platform(format!("MFCreateSample: {e}")))?;
    sample
        .AddBuffer(&buffer)
        .map_err(|e| DecodeError::Platform(format!("Add output buffer: {e}")))?;
    Ok(Some(sample))
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
    let minimum_stride = width as usize;
    let rows_times_two = height as usize * 3;
    let doubled_len = nv12.len().saturating_mul(2);
    let stride = if rows_times_two > 0 && doubled_len % rows_times_two == 0 {
        doubled_len / rows_times_two
    } else {
        0
    };
    if stride < minimum_stride {
        return Err(DecodeError::Platform(format!(
            "short NV12 output: {} bytes for {width}x{height}",
            nv12.len()
        )));
    }

    Ok(DecodedFrame {
        width,
        height,
        stride: stride as u32,
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
        let provided_sample = output_sample_for_transform(transform)?;
        let mut output_buffer = MFT_OUTPUT_DATA_BUFFER {
            dwStreamID: 0,
            pSample: ManuallyDrop::new(provided_sample),
            dwStatus: 0,
            pEvents: ManuallyDrop::new(None),
        };

        let mut status = 0u32;
        let result =
            transform.ProcessOutput(0, std::slice::from_mut(&mut output_buffer), &mut status);
        let sample = ManuallyDrop::take(&mut output_buffer.pSample);
        let _events = ManuallyDrop::take(&mut output_buffer.pEvents);
        match result {
            Ok(()) => {
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

    #[test]
    fn sequence_header_from_config_builds_annex_b() {
        let cfg = StreamConfig {
            sps: vec![0x67, 0x42],
            pps: vec![0x68, 0xce],
            width: 1280,
            height: 720,
            ..Default::default()
        };
        let header = MfH264Decoder::sequence_header_from_config(Some(&cfg));
        assert_eq!(header, vec![0, 0, 0, 1, 0x67, 0x42, 0, 0, 0, 1, 0x68, 0xce]);
    }

    #[test]
    fn existing_sta_apartment_is_borrowed_not_replaced() {
        assert!(!com_initialization_ownership(RPC_E_CHANGED_MODE).expect("borrow STA"));
        assert!(com_initialization_ownership(HRESULT(0)).expect("own successful init"));
    }

    #[test]
    fn decoder_starts_inside_existing_sta_apartment() {
        let initialized =
            unsafe { CoInitializeEx(None, windows::Win32::System::Com::COINIT_APARTMENTTHREADED) };
        initialized.ok().expect("initialize fixture STA");
        let decoder = MfH264Decoder::new().expect("create MF decoder inside GPUI-like STA");
        drop(decoder);
        unsafe { CoUninitialize() };
    }
}
