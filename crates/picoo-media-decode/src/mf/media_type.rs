//! Native input/output media-type negotiation.
use super::*;

pub(super) fn pack_frame_size(width: u32, height: u32) -> u64 {
    ((width as u64) << 32) | height as u64
}

unsafe fn advertised_nv12_type(
    transform: &IMFTransform,
) -> Result<windows::Win32::Media::MediaFoundation::IMFMediaType, DecodeError> {
    for index in 0..64 {
        let candidate = match transform.GetOutputAvailableType(0, index) {
            Ok(candidate) => candidate,
            Err(error) if error.code() == MF_E_NO_MORE_TYPES => break,
            Err(error) => {
                return Err(DecodeError::Platform(format!(
                    "output type enumeration: {error}"
                )))
            }
        };
        if candidate.GetGUID(&MF_MT_SUBTYPE).ok() == Some(MFVideoFormat_NV12) {
            return Ok(candidate);
        }
    }
    Err(DecodeError::Platform(
        "Decoder offers no native NV12 type".into(),
    ))
}

pub(super) unsafe fn configure_transform(
    transform: &IMFTransform,
    codec: Codec,
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
        .SetGUID(
            &MF_MT_SUBTYPE,
            &match codec {
                Codec::Avc => MFVideoFormat_H264,
                Codec::Hevc => windows::Win32::Media::MediaFoundation::MFVideoFormat_HEVC,
            },
        )
        .map_err(|e| DecodeError::Platform(format!("input subtype: {e}")))?;
    if codec == Codec::Hevc {
        use windows::Win32::Media::MediaFoundation::{
            eAVEncH265VProfile_Main_420_8, MF_MT_MPEG2_PROFILE,
        };
        in_type
            .SetUINT32(&MF_MT_MPEG2_PROFILE, eAVEncH265VProfile_Main_420_8.0 as u32)
            .map_err(|error| DecodeError::Platform(format!("HEVC Main profile: {error}")))?;
    }
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

    // Preserve the MFT's native aperture/geometry rather than invent a bare
    // output type that discards the Decoder's display metadata.
    let out_type = advertised_nv12_type(transform)?;
    out_type
        .SetGUID(&MF_MT_MAJOR_TYPE, &MFMediaType_Video)
        .map_err(|e| DecodeError::Platform(format!("output major type: {e}")))?;
    out_type
        .SetGUID(&MF_MT_SUBTYPE, &MFVideoFormat_NV12)
        .map_err(|e| DecodeError::Platform(format!("output subtype: {e}")))?;
    match out_type.GetUINT64(&MF_MT_FRAME_SIZE) {
        Ok(_) => {}
        Err(error) if error.code() == MF_E_ATTRIBUTENOTFOUND => {
            out_type
                .SetUINT64(&MF_MT_FRAME_SIZE, pack_frame_size(width, height))
                .map_err(|e| DecodeError::Platform(format!("output frame size: {e}")))?;
        }
        Err(error) => return Err(DecodeError::Platform(format!("output frame size: {error}"))),
    }
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

pub(super) unsafe fn reset_transform(transform: &IMFTransform) -> Result<(), DecodeError> {
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
