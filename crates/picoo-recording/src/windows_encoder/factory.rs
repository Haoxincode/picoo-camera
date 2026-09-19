//! Hardware-only MFT discovery, same-device binding and immutable media types.

use super::{native, platform, set_codec_value};
use crate::RecordingError;
use picoo_bitstream::Codec;
use windows::{
    core::Interface,
    Win32::{
        Graphics::Direct3D11::ID3D11Device,
        Media::MediaFoundation::*,
        System::{Com::CoTaskMemFree, Variant::VARIANT},
    },
};

#[derive(Clone, Copy)]
pub(super) struct EncoderFormat {
    pub codec: Codec,
    pub width: u32,
    pub height: u32,
    pub fps: u32,
    pub bitrate: u32,
    profile: u32,
    level: u32,
}

impl EncoderFormat {
    pub(super) fn new(
        codec: Codec,
        width: u32,
        height: u32,
        fps: u32,
        bitrate: u32,
    ) -> Result<Self, RecordingError> {
        if !matches!((width, height), (1280, 720) | (1920, 1080))
            || !matches!(fps, 30 | 60)
            || bitrate == 0
            || bitrate > 100_000_000
        {
            return Err(RecordingError::InvalidInput("unsupported encoder format"));
        }
        let (profile, level) = match (codec, height, fps) {
            (Codec::Avc, 720, 30) => (eAVEncH264VProfile_High.0 as u32, 31),
            (Codec::Avc, 720, 60) => (eAVEncH264VProfile_High.0 as u32, 32),
            (Codec::Avc, 1080, 30) => (eAVEncH264VProfile_High.0 as u32, 40),
            (Codec::Avc, 1080, 60) => (eAVEncH264VProfile_High.0 as u32, 42),
            (Codec::Hevc, 720, 30) => (eAVEncH265VProfile_Main_420_8.0 as u32, 93),
            (Codec::Hevc, 720, 60) | (Codec::Hevc, 1080, 30) => {
                (eAVEncH265VProfile_Main_420_8.0 as u32, 120)
            }
            (Codec::Hevc, 1080, 60) => (eAVEncH265VProfile_Main_420_8.0 as u32, 123),
            _ => unreachable!("validated dimensions and frame rate"),
        };
        Ok(Self {
            codec,
            width,
            height,
            fps,
            bitrate,
            profile,
            level,
        })
    }

    fn subtype(self) -> windows::core::GUID {
        match self.codec {
            Codec::Avc => MFVideoFormat_H264,
            Codec::Hevc => MFVideoFormat_HEVC,
        }
    }
}

pub(super) struct Selected {
    pub activation: IMFActivate,
    pub transform: IMFTransform,
    pub events: IMFMediaEventGenerator,
    pub codec_api: ICodecAPI,
    pub manager: IMFDXGIDeviceManager,
}

struct Activations {
    pointer: *mut Option<IMFActivate>,
    count: u32,
}

impl Drop for Activations {
    fn drop(&mut self) {
        if self.pointer.is_null() {
            return;
        }
        unsafe {
            for index in 0..self.count as usize {
                std::ptr::drop_in_place(self.pointer.add(index));
            }
            CoTaskMemFree(Some(self.pointer.cast()));
        }
    }
}

pub(super) fn create(
    device: &ID3D11Device,
    format: EncoderFormat,
) -> Result<Selected, RecordingError> {
    let input = MFT_REGISTER_TYPE_INFO {
        guidMajorType: MFMediaType_Video,
        guidSubtype: MFVideoFormat_NV12,
    };
    let output = MFT_REGISTER_TYPE_INFO {
        guidMajorType: MFMediaType_Video,
        guidSubtype: format.subtype(),
    };
    let mut activations = Activations {
        pointer: std::ptr::null_mut(),
        count: 0,
    };
    unsafe {
        platform(MFTEnumEx(
            MFT_CATEGORY_VIDEO_ENCODER,
            MFT_ENUM_FLAG_HARDWARE | MFT_ENUM_FLAG_SORTANDFILTER,
            Some(&input),
            Some(&output),
            &mut activations.pointer,
            &mut activations.count,
        ))?;
    }
    if activations.pointer.is_null() || activations.count == 0 {
        return Err(RecordingError::Platform(
            "system has no hardware AVC/HEVC encoder for NV12".into(),
        ));
    }
    let manager = create_manager(device)?;
    let mut last_error = None;
    for index in 0..activations.count.min(32) as usize {
        let Some(activation) = (unsafe { &*activations.pointer.add(index) }).clone() else {
            continue;
        };
        let transform = match unsafe { activation.ActivateObject::<IMFTransform>() } {
            Ok(transform) => transform,
            Err(error) => {
                last_error = Some(native(error));
                continue;
            }
        };
        match configure(&transform, &manager, format) {
            Ok((events, codec_api)) => {
                return Ok(Selected {
                    activation,
                    transform,
                    events,
                    codec_api,
                    manager,
                });
            }
            Err(error) => {
                last_error = Some(error);
                drop(transform);
                unsafe {
                    let _ = activation.ShutdownObject();
                }
            }
        }
    }
    Err(last_error.unwrap_or_else(|| {
        RecordingError::Platform("no hardware encoder passed D3D11 admission".into())
    }))
}

fn create_manager(device: &ID3D11Device) -> Result<IMFDXGIDeviceManager, RecordingError> {
    let mut token = 0;
    let mut manager = None;
    unsafe {
        platform(MFCreateDXGIDeviceManager(&mut token, &mut manager))?;
    }
    let manager = manager.ok_or(RecordingError::InvalidInput("missing encoder DXGI manager"))?;
    unsafe {
        platform(manager.ResetDevice(device, token))?;
    }
    Ok(manager)
}

fn configure(
    transform: &IMFTransform,
    manager: &IMFDXGIDeviceManager,
    format: EncoderFormat,
) -> Result<(IMFMediaEventGenerator, ICodecAPI), RecordingError> {
    unsafe {
        let attributes = platform(transform.GetAttributes())?;
        if platform(attributes.GetUINT32(&MF_TRANSFORM_ASYNC))? != 1
            || platform(attributes.GetUINT32(&MF_SA_D3D11_AWARE))? != 1
        {
            return Err(RecordingError::InvalidInput(
                "hardware encoder is not asynchronous D3D11 aware",
            ));
        }
        platform(attributes.SetUINT32(&MF_TRANSFORM_ASYNC_UNLOCK, 1))?;
        platform(transform.ProcessMessage(MFT_MESSAGE_SET_D3D_MANAGER, manager.as_raw() as usize))?;

        let codec_api: ICodecAPI = transform.cast().map_err(native)?;
        // AVC requires this static property before SetOutputType. HEVC does
        // not expose it; its one-in/one-out ordering comes from low latency.
        if format.codec == Codec::Avc {
            set_codec_value(
                &codec_api,
                &CODECAPI_AVEncMPVDefaultBPictureCount,
                VARIANT::from(0_u32),
            )?;
        }

        // Both Microsoft AVC and HEVC encoder contracts require output first.
        let output = media_type(format.subtype(), format, true)?;
        platform(transform.SetOutputType(0, &output, 0))?;
        let input = media_type(MFVideoFormat_NV12, format, false)?;
        platform(transform.SetInputType(0, &input, 0))?;
        validate_current_types(transform, format)?;

        for (key, value) in [
            (&CODECAPI_AVLowLatencyMode, VARIANT::from(true)),
            (
                &CODECAPI_AVEncCommonMeanBitRate,
                VARIANT::from(format.bitrate),
            ),
            (
                &CODECAPI_AVEncMPVGOPSize,
                VARIANT::from(format.fps.saturating_mul(2)),
            ),
        ] {
            set_codec_value(&codec_api, key, value)?;
        }
        platform(codec_api.IsSupported(&CODECAPI_AVEncVideoForceKeyFrame))?;
        let mut input_info = MFT_INPUT_STREAM_INFO::default();
        platform(transform.GetInputStreamInfo(0, &mut input_info))?;
        if input_info.dwFlags & MFT_INPUT_STREAM_HOLDS_BUFFERS.0 as u32 != 0 {
            return Err(RecordingError::InvalidInput(
                "hardware encoder retains input beyond its corresponding output",
            ));
        }
        let events: IMFMediaEventGenerator = transform.cast().map_err(native)?;
        platform(transform.ProcessMessage(MFT_MESSAGE_NOTIFY_BEGIN_STREAMING, 0))?;
        platform(transform.ProcessMessage(MFT_MESSAGE_NOTIFY_START_OF_STREAM, 0))?;
        Ok((events, codec_api))
    }
}

unsafe fn media_type(
    subtype: windows::core::GUID,
    format: EncoderFormat,
    compressed: bool,
) -> Result<IMFMediaType, RecordingError> {
    let media = platform(MFCreateMediaType())?;
    platform(media.SetGUID(&MF_MT_MAJOR_TYPE, &MFMediaType_Video))?;
    platform(media.SetGUID(&MF_MT_SUBTYPE, &subtype))?;
    platform(media.SetUINT64(
        &MF_MT_FRAME_SIZE,
        (u64::from(format.width) << 32) | u64::from(format.height),
    ))?;
    platform(media.SetUINT64(&MF_MT_FRAME_RATE, (u64::from(format.fps) << 32) | 1))?;
    platform(media.SetUINT64(&MF_MT_PIXEL_ASPECT_RATIO, (1_u64 << 32) | 1))?;
    platform(media.SetUINT32(&MF_MT_INTERLACE_MODE, MFVideoInterlace_Progressive.0 as u32))?;
    platform(media.SetUINT32(&MF_MT_VIDEO_PRIMARIES, MFVideoPrimaries_BT709.0 as u32))?;
    platform(media.SetUINT32(&MF_MT_TRANSFER_FUNCTION, MFVideoTransFunc_709.0 as u32))?;
    platform(media.SetUINT32(&MF_MT_YUV_MATRIX, MFVideoTransferMatrix_BT709.0 as u32))?;
    platform(media.SetUINT32(&MF_MT_VIDEO_NOMINAL_RANGE, MFNominalRange_16_235.0 as u32))?;
    if compressed {
        platform(media.SetUINT32(&MF_MT_AVG_BITRATE, format.bitrate))?;
        platform(media.SetUINT32(&MF_MT_VIDEO_PROFILE, format.profile))?;
        platform(media.SetUINT32(&MF_MT_MPEG2_LEVEL, format.level))?;
    }
    Ok(media)
}

unsafe fn validate_current_types(
    transform: &IMFTransform,
    format: EncoderFormat,
) -> Result<(), RecordingError> {
    let input = platform(transform.GetInputCurrentType(0))?;
    let output = platform(transform.GetOutputCurrentType(0))?;
    validate_type(&input, MFVideoFormat_NV12, format, false)?;
    validate_type(&output, format.subtype(), format, true)
}

unsafe fn validate_type(
    media: &IMFMediaType,
    subtype: windows::core::GUID,
    format: EncoderFormat,
    compressed: bool,
) -> Result<(), RecordingError> {
    let expected_size = (u64::from(format.width) << 32) | u64::from(format.height);
    let expected_rate = (u64::from(format.fps) << 32) | 1;
    let expected_aspect = (1_u64 << 32) | 1;
    let valid = platform(media.GetGUID(&MF_MT_MAJOR_TYPE))? == MFMediaType_Video
        && platform(media.GetGUID(&MF_MT_SUBTYPE))? == subtype
        && platform(media.GetUINT64(&MF_MT_FRAME_SIZE))? == expected_size
        && platform(media.GetUINT64(&MF_MT_FRAME_RATE))? == expected_rate
        && platform(media.GetUINT64(&MF_MT_PIXEL_ASPECT_RATIO))? == expected_aspect
        && platform(media.GetUINT32(&MF_MT_INTERLACE_MODE))?
            == MFVideoInterlace_Progressive.0 as u32
        && platform(media.GetUINT32(&MF_MT_VIDEO_PRIMARIES))? == MFVideoPrimaries_BT709.0 as u32
        && platform(media.GetUINT32(&MF_MT_TRANSFER_FUNCTION))? == MFVideoTransFunc_709.0 as u32
        && platform(media.GetUINT32(&MF_MT_YUV_MATRIX))? == MFVideoTransferMatrix_BT709.0 as u32
        && platform(media.GetUINT32(&MF_MT_VIDEO_NOMINAL_RANGE))? == MFNominalRange_16_235.0 as u32;
    let compressed_valid = !compressed
        || (platform(media.GetUINT32(&MF_MT_VIDEO_PROFILE))? == format.profile
            && platform(media.GetUINT32(&MF_MT_MPEG2_LEVEL))? == format.level
            && platform(media.GetUINT32(&MF_MT_AVG_BITRATE))? == format.bitrate);
    if !valid || !compressed_valid {
        return Err(RecordingError::InvalidInput(
            "hardware encoder changed the requested media type",
        ));
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn official_profile_and_level_matrix_is_complete() {
        for (codec, height, fps, profile, level) in [
            (Codec::Avc, 720, 30, 100, 31),
            (Codec::Avc, 720, 60, 100, 32),
            (Codec::Avc, 1080, 30, 100, 40),
            (Codec::Avc, 1080, 60, 100, 42),
            (Codec::Hevc, 720, 30, 1, 93),
            (Codec::Hevc, 720, 60, 1, 120),
            (Codec::Hevc, 1080, 30, 1, 120),
            (Codec::Hevc, 1080, 60, 1, 123),
        ] {
            let width = if height == 720 { 1280 } else { 1920 };
            let format = EncoderFormat::new(codec, width, height, fps, 8_000_000).unwrap();
            assert_eq!((format.profile, format.level), (profile, level));
        }
        assert!(EncoderFormat::new(Codec::Avc, 640, 480, 30, 8_000_000).is_err());
        assert!(EncoderFormat::new(Codec::Hevc, 1280, 720, 25, 8_000_000).is_err());
        assert!(EncoderFormat::new(Codec::Hevc, 1280, 720, 30, 0).is_err());
    }
}
