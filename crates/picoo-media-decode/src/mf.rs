//! Media Foundation H.264 decoder — REQ-PICOO-MEDIA-005.
//!
//! CMSH264DecoderMFT IMFTransform pipeline: H.264 access unit → NV12.
//! When StreamConfig carries SPS/PPS, they are applied as
//! `MF_MT_MPEG_SEQUENCE_HEADER` and injected ahead of the first AU after
//! (re)configure — REQ-PICOO-PROTOCOL-005 / REQ-PICOO-SESSION-004.

use picoo_bitstream::{AccessUnit, AvcSpsFacts, PictureKind, RandomAccessPoint};
use picoo_protocol::control::StreamConfig;
use windows::core::{GUID, HRESULT};
use windows::Win32::Foundation::RPC_E_CHANGED_MODE;
use windows::Win32::Media::MediaFoundation::{
    CMSH264DecoderMFT, IMFSample, IMFTransform, MFCreateAlignedMemoryBuffer, MFCreateMediaType,
    MFCreateMemoryBuffer, MFCreateSample, MFMediaType_Video, MFNominalRange_16_235,
    MFVideoFormat_H264, MFVideoFormat_NV12, MFVideoInterlace_Progressive, MFVideoPrimaries_BT709,
    MFVideoTransFunc_709, MFVideoTransferMatrix_BT709, MFT_MESSAGE_COMMAND_FLUSH,
    MFT_MESSAGE_NOTIFY_BEGIN_STREAMING, MFT_MESSAGE_NOTIFY_START_OF_STREAM,
    MFT_OUTPUT_STREAM_CAN_PROVIDE_SAMPLES, MFT_OUTPUT_STREAM_PROVIDES_SAMPLES,
    MF_E_ATTRIBUTENOTFOUND, MF_E_NOTACCEPTING, MF_E_NO_MORE_TYPES, MF_E_TRANSFORM_NEED_MORE_INPUT,
    MF_E_TRANSFORM_STREAM_CHANGE, MF_LOW_LATENCY, MF_MT_FRAME_RATE, MF_MT_FRAME_SIZE,
    MF_MT_INTERLACE_MODE, MF_MT_MAJOR_TYPE, MF_MT_PIXEL_ASPECT_RATIO, MF_MT_SUBTYPE,
    MF_MT_TRANSFER_FUNCTION, MF_MT_VIDEO_NOMINAL_RANGE, MF_MT_VIDEO_PRIMARIES, MF_MT_YUV_MATRIX,
};
use windows::Win32::System::Com::{
    CoCreateInstance, CoInitializeEx, CoUninitialize, CLSCTX_INPROC_SERVER, COINIT_MULTITHREADED,
};

use crate::{AccessUnitDecoder, DecodeError, DecodeOutcome, DecodedFrame};

mod buffers;
mod device;
mod output;
use buffers::sample_to_frame;

const DEFAULT_FPS: u32 = 30;
/// MF_MT_MPEG_SEQUENCE_HEADER — H.264 SPS/PPS with Annex-B start codes.
const MF_MT_MPEG_SEQUENCE_HEADER: GUID = GUID::from_u128(0x05f4_6766_f1a9_44e5_b82a_e4df_c2ea_2873);

pub struct MfH264Decoder {
    transform: IMFTransform,
    configured: bool,
    geometry: Option<AvcSpsFacts>,
    fps: u32,
    next_sample_time_100ns: i64,
    sequence_header: Vec<u8>,
    inject_sequence_header: bool,
    device: device::DecoderDevice,
    // Declared last so the transform is released before MFShutdown/CoUninitialize.
    _runtime: MfRuntimeGuard,
}

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

        let gpu = std::sync::Arc::new(device::create_context()?);
        let transform = create_transform()?;
        let device = device::attach_manager(&transform, gpu)?;
        Ok(Self::initialized(transform, runtime, device))
    }

    #[cfg(any(test, feature = "test-codecs"))]
    pub(super) fn software_diagnostic() -> Result<Self, DecodeError> {
        let runtime = MfRuntimeGuard::start()?;
        Ok(Self::initialized(
            create_transform()?,
            runtime,
            device::DecoderDevice::SoftwareDiagnostic,
        ))
    }

    fn initialized(
        transform: IMFTransform,
        runtime: MfRuntimeGuard,
        device: device::DecoderDevice,
    ) -> Self {
        Self {
            transform,
            configured: false,
            geometry: None,
            fps: DEFAULT_FPS,
            next_sample_time_100ns: 0,
            sequence_header: Vec::new(),
            inject_sequence_header: false,
            device,
            _runtime: runtime,
        }
    }

    fn sequence_header_from_config(
        stream_config: Option<&StreamConfig>,
    ) -> Result<Vec<u8>, DecodeError> {
        stream_config
            .map(crate::configured_avc::sequence_header)
            .transpose()
            .map(Option::unwrap_or_default)
    }

    fn ensure_configured(
        &mut self,
        stream_config: Option<&StreamConfig>,
        picture: &AccessUnit<'_>,
    ) -> Result<(), DecodeError> {
        if self.device.gpu().is_some() && stream_config.is_none() {
            return Err(DecodeError::NotInitialized);
        }
        let fps = stream_config.map_or(DEFAULT_FPS, |cfg| cfg.fps.max(1));
        let (geometry, sequence_header) = if let Some(config) = stream_config {
            let record = crate::configured_avc::configuration(config)?;
            let geometry = AvcSpsFacts::parse(&record.sps()[0])
                .map_err(|e| DecodeError::Platform(e.to_string()))?;
            if (config.width, config.height) != (geometry.visible_width, geometry.visible_height) {
                return Err(DecodeError::ConfigurationMismatch);
            }
            (geometry, Self::sequence_header_from_config(stream_config)?)
        } else {
            let sps = picture
                .nals()
                .iter()
                .find(|nal| nal[0] & 0x1f == 7)
                .ok_or(DecodeError::NotInitialized)?;
            let geometry =
                AvcSpsFacts::parse(sps).map_err(|e| DecodeError::Platform(e.to_string()))?;
            let mut header = Vec::new();
            for nal in picture
                .nals()
                .iter()
                .filter(|nal| matches!(nal[0] & 0x1f, 7 | 8))
            {
                header.extend_from_slice(&[0, 0, 0, 1]);
                header.extend_from_slice(nal);
            }
            (geometry, header)
        };
        if self.configured
            && self.geometry == Some(geometry)
            && self.fps == fps
            && self.sequence_header == sequence_header
        {
            return Ok(());
        }

        if let Some(gpu) = self.device.gpu() {
            device::validate_configuration(gpu, &geometry, fps)?;
        }
        unsafe {
            configure_transform(
                &self.transform,
                geometry.coded_width,
                geometry.coded_height,
                fps,
                sequence_header.as_slice(),
            )?;
        }
        self.configured = true;
        self.geometry = Some(geometry);
        self.fps = fps;
        self.next_sample_time_100ns = 0;
        self.inject_sequence_header = !sequence_header.is_empty();
        self.sequence_header = sequence_header;
        Ok(())
    }

    fn decode_h264_au(
        &mut self,
        picture: AccessUnit<'_>,
        stream_config: Option<&StreamConfig>,
    ) -> Result<DecodeOutcome, DecodeError> {
        self.ensure_configured(stream_config, &picture)?;
        let geometry = self.geometry.as_ref().ok_or(DecodeError::NotInitialized)?;
        let annex = picture
            .to_annex_b()
            .map_err(|_| DecodeError::UnsupportedAccessUnit)?;
        let access_unit = annex.as_slice();
        let refresh_accepted =
            picture.picture().kind == PictureKind::RandomAccess(RandomAccessPoint::AvcIdr);
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
                geometry,
                self.device.gpu(),
            )?;
            self.next_sample_time_100ns += duration_100ns;
            let frame = match pending {
                Some(frame) => Some(frame),
                None => drain_output(&self.transform, geometry, self.device.gpu())?,
            };
            Ok(DecodeOutcome {
                frame,
                refresh_accepted,
            })
        }
    }
}

impl AccessUnitDecoder for MfH264Decoder {
    fn decode_access_unit(
        &mut self,
        access_unit: &[u8],
        stream_config: Option<&StreamConfig>,
    ) -> Result<DecodeOutcome, DecodeError> {
        let picture = crate::configured_avc::validate(access_unit, stream_config)?;
        self.decode_h264_au(picture, stream_config)
    }

    fn reset(&mut self) -> Result<(), DecodeError> {
        if self.configured {
            unsafe { reset_transform(&self.transform)? };
        }
        self.next_sample_time_100ns = 0;
        self.inject_sequence_header = !self.sequence_header.is_empty();
        Ok(())
    }
}

fn create_transform() -> Result<IMFTransform, DecodeError> {
    let transform: IMFTransform =
        unsafe { CoCreateInstance(&CMSH264DecoderMFT, None, CLSCTX_INPROC_SERVER) }
            .map_err(|e| DecodeError::Platform(format!("CoCreateInstance H264 MFT: {e}")))?;
    unsafe {
        transform
            .GetAttributes()
            .and_then(|attributes| attributes.SetUINT32(&MF_LOW_LATENCY, 1))
            .map_err(|e| DecodeError::Platform(format!("enable MF low latency: {e}")))?;
    }
    Ok(transform)
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
    out_type
        .SetUINT32(&MF_MT_YUV_MATRIX, MFVideoTransferMatrix_BT709.0 as u32)
        .map_err(|e| DecodeError::Platform(format!("output YUV matrix: {e}")))?;
    out_type
        .SetUINT32(&MF_MT_VIDEO_NOMINAL_RANGE, MFNominalRange_16_235.0 as u32)
        .map_err(|e| DecodeError::Platform(format!("output nominal range: {e}")))?;
    out_type
        .SetUINT32(&MF_MT_VIDEO_PRIMARIES, MFVideoPrimaries_BT709.0 as u32)
        .map_err(|e| DecodeError::Platform(format!("output primaries: {e}")))?;
    out_type
        .SetUINT32(&MF_MT_TRANSFER_FUNCTION, MFVideoTransFunc_709.0 as u32)
        .map_err(|e| DecodeError::Platform(format!("output transfer function: {e}")))?;
    transform
        .SetOutputType(0, &out_type, 0)
        .map_err(|e| DecodeError::Platform(format!("SetOutputType: {e}")))?;

    reset_transform(transform)?;

    Ok(())
}

unsafe fn reset_transform(transform: &IMFTransform) -> Result<(), DecodeError> {
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
    geometry: &AvcSpsFacts,
    gpu: Option<&std::sync::Arc<picoo_gpu::WindowsGpuContext>>,
) -> Result<Option<DecodedFrame>, DecodeError> {
    let sample = create_input_sample(access_unit, sample_time_100ns, duration_100ns)?;

    match transform.ProcessInput(0, &sample, 0) {
        Ok(()) => Ok(None),
        Err(e) if e.code() == MF_E_NOTACCEPTING => {
            // Drain pending output then retry once.
            let pending = drain_output(transform, geometry, gpu)?;
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
    require_dxgi: bool,
) -> Result<Option<IMFSample>, DecodeError> {
    let info = transform
        .GetOutputStreamInfo(0)
        .map_err(|e| DecodeError::Platform(format!("GetOutputStreamInfo: {e}")))?;
    let provides_samples = info.dwFlags & MFT_OUTPUT_STREAM_PROVIDES_SAMPLES.0 as u32 != 0;
    let can_provide_samples = info.dwFlags & MFT_OUTPUT_STREAM_CAN_PROVIDE_SAMPLES.0 as u32 != 0;
    if provides_samples || can_provide_samples {
        return Ok(None);
    }
    if require_dxgi {
        return Err(DecodeError::Platform(
            "hardware MFT must provide its own DXGI output samples".into(),
        ));
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

/// REQ-PICOO-MEDIA-031: renegotiate the native output without flushing or
/// resubmitting the AU. Only the declared NV12 geometry is accepted here.
unsafe fn renegotiate_output(
    transform: &IMFTransform,
    width: u32,
    height: u32,
) -> Result<(), DecodeError> {
    let mut offered = Vec::new();
    for index in 0..32 {
        let candidate = match transform.GetOutputAvailableType(0, index) {
            Ok(candidate) => candidate,
            Err(error) if error.code() == MF_E_NO_MORE_TYPES => break,
            Err(error) => {
                return Err(DecodeError::Platform(format!(
                    "GetOutputAvailableType: {error}"
                )))
            }
        };
        let subtype = candidate.GetGUID(&MF_MT_SUBTYPE).ok();
        let frame_size = candidate.GetUINT64(&MF_MT_FRAME_SIZE);
        offered.push((
            subtype,
            frame_size
                .as_ref()
                .ok()
                .map(|size| ((size >> 32) as u32, *size as u32)),
        ));
        if subtype != Some(MFVideoFormat_NV12) {
            continue;
        }
        match frame_size {
            Ok(size) if size != pack_frame_size(width, height) => continue,
            Ok(_) => {}
            // IMFTransform explicitly permits partial output media types.
            // Complete the requested output constraint; SetOutputType remains
            // the native acceptance gate, and known differing sizes are rejected.
            Err(error) if error.code() == MF_E_ATTRIBUTENOTFOUND => {
                candidate
                    .SetUINT64(&MF_MT_FRAME_SIZE, pack_frame_size(width, height))
                    .map_err(|error| {
                        DecodeError::Platform(format!("complete output frame size: {error}"))
                    })?;
            }
            Err(error) => return Err(DecodeError::Platform(format!("output frame size: {error}"))),
        }
        return transform
            .SetOutputType(0, &candidate, 0)
            .map_err(|error| DecodeError::Platform(format!("renegotiate output: {error}")));
    }
    Err(DecodeError::Platform(format!(
        "stream change offers no matching NV12 output for {width}x{height}; offered={offered:?}"
    )))
}

unsafe fn drain_output(
    transform: &IMFTransform,
    geometry: &AvcSpsFacts,
    gpu: Option<&std::sync::Arc<picoo_gpu::WindowsGpuContext>>,
) -> Result<Option<DecodedFrame>, DecodeError> {
    // One format-change retry is enough to consume the newly negotiated
    // output. Repeated stream changes fail explicitly instead of spinning.
    for attempt in 0..2 {
        let provided_sample = output_sample_for_transform(transform, gpu.is_some())?;
        let output = output::process(transform, gpu, provided_sample)?;
        let sample = output.sample;
        let result = output.result;
        match result {
            Ok(()) => {
                return sample
                    .as_ref()
                    .map(|sample| {
                        if let Some(gpu) = gpu {
                            device::validate_output_device(sample, gpu)?;
                        }
                        sample_to_frame(sample, transform, geometry)
                    })
                    .transpose()
            }
            Err(error) if error.code() == MF_E_TRANSFORM_NEED_MORE_INPUT => return Ok(None),
            Err(error) if error.code() == MF_E_TRANSFORM_STREAM_CHANGE && attempt == 0 => {
                drop(sample);
                renegotiate_output(transform, geometry.coded_width, geometry.coded_height)?;
            }
            Err(error) => return Err(DecodeError::Platform(format!("ProcessOutput: {error}"))),
        }
    }
    unreachable!("bounded output retry returns on its final attempt")
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
        let (sps, pps) =
            picoo_bitstream::avc::extract_sps_pps(picoo_testkit::H264_1280X720_RED_IDR).unwrap();
        let cfg = StreamConfig {
            codec: picoo_protocol::control::VideoCodec::Avc as i32,
            codec_configuration: picoo_bitstream::CodecConfiguration::from_avc_parameter_sets(
                &sps, &pps,
            )
            .unwrap()
            .record()
            .to_vec(),
            width: 1280,
            height: 720,
            ..Default::default()
        };
        let header = MfH264Decoder::sequence_header_from_config(Some(&cfg)).unwrap();
        assert_eq!(
            header,
            picoo_bitstream::avc::annex_b_parameter_sets(&sps, &pps)
        );
    }

    #[test]
    fn native_coded_allocation_preserves_visible_geometry_and_rejects_conflicting_config() {
        // REQ-PICOO-MEDIA-032: the real fixture has coded 192x96, visible 64x64.
        let annex = picoo_testkit::AVC_64X64_BT709_IDR;
        let (sps, pps) = picoo_bitstream::avc::extract_sps_pps(annex).unwrap();
        let wire = picoo_bitstream::canonical_access_unit(
            picoo_bitstream::Codec::Avc,
            picoo_bitstream::NalFormat::AnnexB,
            annex,
        )
        .unwrap();
        let mut config = StreamConfig {
            codec: picoo_protocol::control::VideoCodec::Avc as i32,
            width: 64,
            height: 64,
            fps: 30,
            codec_configuration: picoo_bitstream::CodecConfiguration::from_avc_parameter_sets(
                &sps, &pps,
            )
            .unwrap()
            .record()
            .to_vec(),
            ..Default::default()
        };
        let mut decoder = MfH264Decoder::software_diagnostic().unwrap();
        let submitted = decoder.decode_access_unit(&wire, Some(&config)).unwrap();
        let frame = if let Some(frame) = submitted.frame {
            frame
        } else {
            // A synchronous MFT can retain its last picture until EOS. Drain
            // the accepted input; never submit the same AU again to force output.
            unsafe {
                use windows::Win32::Media::MediaFoundation::{
                    MFT_MESSAGE_COMMAND_DRAIN, MFT_MESSAGE_NOTIFY_END_OF_STREAM,
                };
                decoder
                    .transform
                    .ProcessMessage(MFT_MESSAGE_NOTIFY_END_OF_STREAM, 0)
                    .unwrap();
                decoder
                    .transform
                    .ProcessMessage(MFT_MESSAGE_COMMAND_DRAIN, 0)
                    .unwrap();
                drain_output(&decoder.transform, decoder.geometry.as_ref().unwrap(), None)
                    .unwrap()
                    .expect("EOS drain releases the accepted picture")
            }
        };
        assert_eq!(
            (frame.description().width, frame.description().height),
            (64, 64)
        );
        let pixels = frame.cpu_nv12_bytes().unwrap();
        assert_eq!(pixels.len(), 64 * 64 * 3 / 2);
        assert!(pixels[0] > 16 && pixels[64 * 64 + 1] > pixels[64 * 64]);
        let geometry = decoder.geometry;
        let header = decoder.sequence_header.clone();
        config.width = 1280;
        assert!(matches!(
            decoder.decode_access_unit(&wire, Some(&config)),
            Err(DecodeError::ConfigurationMismatch)
        ));
        assert_eq!(decoder.geometry, geometry);
        assert_eq!(decoder.sequence_header, header);
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
        let decoder =
            MfH264Decoder::software_diagnostic().expect("create diagnostic MF decoder inside STA");
        drop(decoder);
        unsafe { CoUninitialize() };
    }
}
