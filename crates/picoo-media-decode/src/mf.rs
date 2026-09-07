//! Media Foundation AVC/HEVC decoder — REQ-PICOO-MEDIA-005.
//!
//! System synchronous IMFTransform pipeline: committed access unit → native NV12.
//! When StreamConfig carries SPS/PPS, they are applied as
//! `MF_MT_MPEG_SEQUENCE_HEADER` and injected ahead of the first AU after
//! (re)configure — REQ-PICOO-PROTOCOL-005 / REQ-PICOO-SESSION-004.

#[cfg(test)]
use crate::DecodeFixture as _;
use picoo_bitstream::{AccessUnit, Codec, PictureKind, RandomAccessPoint, VideoSpsFacts};
use picoo_protocol::control::StreamConfig;
use windows::core::GUID;
use windows::Win32::Media::MediaFoundation::{
    IMFSample, IMFTransform, MFCreateAlignedMemoryBuffer, MFCreateMediaType, MFCreateMemoryBuffer,
    MFCreateSample, MFMediaType_Video, MFNominalRange_16_235, MFVideoFormat_H264,
    MFVideoFormat_NV12, MFVideoInterlace_Progressive, MFVideoPrimaries_BT709, MFVideoTransFunc_709,
    MFVideoTransferMatrix_BT709, MFT_MESSAGE_COMMAND_FLUSH, MFT_MESSAGE_NOTIFY_BEGIN_STREAMING,
    MFT_MESSAGE_NOTIFY_START_OF_STREAM, MFT_OUTPUT_STREAM_CAN_PROVIDE_SAMPLES,
    MFT_OUTPUT_STREAM_PROVIDES_SAMPLES, MF_E_ATTRIBUTENOTFOUND, MF_E_NOTACCEPTING,
    MF_E_NO_MORE_TYPES, MF_E_TRANSFORM_NEED_MORE_INPUT, MF_E_TRANSFORM_STREAM_CHANGE,
    MF_MT_FRAME_RATE, MF_MT_FRAME_SIZE, MF_MT_INTERLACE_MODE, MF_MT_MAJOR_TYPE,
    MF_MT_PIXEL_ASPECT_RATIO, MF_MT_SUBTYPE, MF_MT_TRANSFER_FUNCTION, MF_MT_VIDEO_NOMINAL_RANGE,
    MF_MT_VIDEO_PRIMARIES, MF_MT_YUV_MATRIX,
};

use crate::{AccessUnitDecoder, DecodeError, DecodeOutcome, DecodedFrame, DecodedOutput};
use std::collections::BTreeMap;
use std::sync::Arc;

#[cfg(any(test, feature = "test-codecs"))]
mod buffers;
mod device;
mod factory;
mod media_type;
use media_type::{configure_transform, pack_frame_size, reset_transform};
mod native_output;
mod output;
use crate::windows_runtime as runtime;

#[cfg(test)]
use runtime::com_initialization_ownership;
use runtime::MfRuntimeGuard;
#[cfg(test)]
use windows::Win32::System::Com::{CoInitializeEx, CoUninitialize};
#[cfg(test)]
use windows::{core::HRESULT, Win32::Foundation::RPC_E_CHANGED_MODE};

const DEFAULT_FPS: u32 = 30;
const MAX_PENDING_SUBMISSIONS: usize = 16;
/// MF_MT_MPEG_SEQUENCE_HEADER — H.264 SPS/PPS with Annex-B start codes.
const MF_MT_MPEG_SEQUENCE_HEADER: GUID = GUID::from_u128(0x05f4_6766_f1a9_44e5_b82a_e4df_c2ea_2873);

pub struct MfVideoDecoder {
    codec: Codec,
    transform: IMFTransform,
    configured: bool,
    geometry: Option<VideoSpsFacts>,
    fps: u32,
    next_sample_time_100ns: i64,
    pending: BTreeMap<i64, Arc<crate::DecodeToken>>,
    sequence_header: Vec<u8>,
    inject_sequence_header: bool,
    device: device::DecoderDevice,
    // Declared last so the transform is released before MFShutdown/CoUninitialize.
    _runtime: MfRuntimeGuard,
}

impl MfVideoDecoder {
    pub fn new() -> Result<Self, DecodeError> {
        let runtime = MfRuntimeGuard::start()?;

        let gpu = std::sync::Arc::new(device::create_context()?);
        let transform = factory::create(Codec::Avc)?;
        let device = device::attach_manager(&transform, gpu)?;
        Ok(Self::initialized(transform, runtime, device))
    }

    #[cfg(any(test, feature = "test-codecs"))]
    pub(super) fn software_diagnostic() -> Result<Self, DecodeError> {
        let runtime = MfRuntimeGuard::start()?;
        Ok(Self::initialized(
            factory::create(Codec::Avc)?,
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
            codec: Codec::Avc,
            transform,
            configured: false,
            geometry: None,
            fps: DEFAULT_FPS,
            next_sample_time_100ns: 0,
            pending: BTreeMap::new(),
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
            .map(crate::configured_picture::sequence_header)
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
            let record = crate::configured_picture::configuration(config)?;
            let geometry = record
                .source_facts()
                .map_err(|error| DecodeError::Platform(error.to_string()))?;
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
                VideoSpsFacts::parse_avc(sps).map_err(|e| DecodeError::Platform(e.to_string()))?;
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
        if self.codec == picture.codec()
            && self.configured
            && self.geometry == Some(geometry)
            && self.fps == fps
            && self.sequence_header == sequence_header
        {
            return Ok(());
        }

        if let Some(gpu) = self.device.gpu() {
            crate::source_format::validate_source(&geometry)?;
            device::validate_configuration(gpu, picture.codec(), &geometry, fps)?;
        }
        let replacement = factory::create(picture.codec())?;
        let replacement_device = if let Some(gpu) = self.device.gpu() {
            device::attach_manager(&replacement, gpu.clone())?
        } else {
            #[cfg(any(test, feature = "test-codecs"))]
            {
                device::DecoderDevice::SoftwareDiagnostic
            }
            #[cfg(not(any(test, feature = "test-codecs")))]
            {
                return Err(DecodeError::NotInitialized);
            }
        };
        unsafe {
            configure_transform(
                &replacement,
                picture.codec(),
                geometry.coded_width,
                geometry.coded_height,
                fps,
                sequence_header.as_slice(),
            )?;
        }
        self.transform = replacement;
        self.device = replacement_device;
        self.codec = picture.codec();
        self.configured = true;
        self.geometry = Some(geometry);
        self.fps = fps;
        self.pending.clear();
        self.inject_sequence_header = !sequence_header.is_empty();
        self.sequence_header = sequence_header;
        Ok(())
    }

    fn decode_picture(
        &mut self,
        picture: AccessUnit<'_>,
        stream_config: Option<&StreamConfig>,
        token: Arc<crate::DecodeToken>,
    ) -> Result<DecodeOutcome, DecodeError> {
        if matches!(
            picture.picture().kind,
            PictureKind::RandomAccess(RandomAccessPoint::HevcCra)
                | PictureKind::HevcRasl
                | PictureKind::HevcRadl
        ) {
            return Err(DecodeError::UnsupportedAccessUnit);
        }
        self.ensure_configured(stream_config, &picture)?;
        let mut frames = self.drain_frames()?;
        if self.pending.len() >= MAX_PENDING_SUBMISSIONS {
            return Err(DecodeError::Platform(
                "MF pending submission limit reached".into(),
            ));
        }
        let annex = picture
            .to_annex_b()
            .map_err(|_| DecodeError::UnsupportedAccessUnit)?;
        let access_unit = annex.as_slice();
        let refresh_accepted = matches!(
            picture.picture().kind,
            PictureKind::RandomAccess(RandomAccessPoint::AvcIdr | RandomAccessPoint::HevcIdr)
        );
        let owned;
        let payload = if self.inject_sequence_header && !self.sequence_header.is_empty() {
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
            let stamp = self.next_sample_time_100ns;
            self.next_sample_time_100ns = stamp
                .checked_add(duration_100ns)
                .ok_or_else(|| DecodeError::Platform("MF submission clock exhausted".into()))?;
            let sample = create_input_sample(payload, stamp, duration_100ns)?;
            match self.transform.ProcessInput(0, &sample, 0) {
                Ok(()) => {}
                Err(error) if error.code() == MF_E_NOTACCEPTING => {
                    frames.extend(self.drain_frames()?);
                    self.transform
                        .ProcessInput(0, &sample, 0)
                        .map_err(|error| {
                            DecodeError::Platform(format!("ProcessInput retry: {error}"))
                        })?;
                }
                Err(error) => return Err(DecodeError::Platform(format!("ProcessInput: {error}"))),
            }
            // Only accepted submissions can be matched by GetSampleTime.
            self.pending.insert(stamp, token);
            self.inject_sequence_header = false;
            frames.extend(self.drain_frames()?);
            Ok(DecodeOutcome {
                frames,
                refresh_accepted,
            })
        }
    }
    fn drain_frames(&mut self) -> Result<Vec<DecodedOutput>, DecodeError> {
        let geometry = self.geometry.as_ref().ok_or(DecodeError::NotInitialized)?;
        let mut frames = Vec::new();
        for _ in 0..=MAX_PENDING_SUBMISSIONS {
            let output = unsafe {
                drain_output(&self.transform, geometry, self.device.gpu(), &self._runtime)?
            };
            let Some((stamp, frame)) = output else {
                return Ok(frames);
            };
            let token = self.pending.remove(&stamp).ok_or_else(|| {
                DecodeError::Platform("MF output has unknown or duplicate submission time".into())
            })?;
            frames.push(DecodedOutput { token, frame });
        }
        Err(DecodeError::Platform(
            "MF output drain limit exceeded".into(),
        ))
    }
}

impl AccessUnitDecoder for MfVideoDecoder {
    fn submit(
        &mut self,
        submission: crate::DecodeSubmission<'_>,
    ) -> Result<DecodeOutcome, DecodeError> {
        let access_unit = submission.access_unit;
        let stream_config = submission.token.stream_config.as_deref();

        let codec = stream_config
            .map(crate::configured_picture::configured_codec)
            .transpose()?
            .unwrap_or(Codec::Avc);
        let picture = crate::configured_picture::validate(codec, access_unit, stream_config)?;
        self.decode_picture(picture, stream_config, submission.token.clone())
    }

    fn reset(&mut self) -> Result<(), DecodeError> {
        if self.configured {
            unsafe { reset_transform(&self.transform)? };
        }
        self.pending.clear();
        self.inject_sequence_header = !self.sequence_header.is_empty();
        Ok(())
    }
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
/// resubmitting the AU. Native geometry is validated against the completed image
/// and its negotiated aperture, independently from the coded SPS dimensions.
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
            Ok(_) => {}
            // IMFTransform explicitly permits partial output media types.
            // Complete the requested output constraint; SetOutputType remains
            // the native acceptance gate. Complete images are validated by
            // native_output; diagnostic CPU layouts are checked by buffers.
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
    geometry: &VideoSpsFacts,
    gpu: Option<&std::sync::Arc<picoo_gpu::WindowsGpuContext>>,
    runtime: &MfRuntimeGuard,
) -> Result<Option<(i64, DecodedFrame)>, DecodeError> {
    // One format-change retry is enough to consume the newly negotiated
    // output. Repeated stream changes fail explicitly instead of spinning.
    for attempt in 0..2 {
        let provided_sample = output_sample_for_transform(transform, gpu.is_some())?;
        let output = output::process(transform, gpu, provided_sample, runtime._lifetime.clone())?;
        let sample = output.sample;
        let result = output.result;
        match result {
            Ok(()) => {
                return sample
                    .as_ref()
                    .map(|sample| {
                        let stamp = sample.GetSampleTime().map_err(|error| {
                            DecodeError::Platform(format!("MF output submission time: {error}"))
                        })?;
                        let frame = if let Some(gpu) = gpu {
                            native_output::sample_to_frame(
                                sample, transform, geometry, gpu, runtime,
                            )?
                        } else {
                            #[cfg(any(test, feature = "test-codecs"))]
                            {
                                buffers::sample_to_frame(sample, transform, geometry)?
                            }
                            #[cfg(not(any(test, feature = "test-codecs")))]
                            {
                                return Err(DecodeError::NotInitialized);
                            }
                        };
                        Ok((stamp, frame))
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
mod tests;

#[cfg(test)]
mod submission_tests;
