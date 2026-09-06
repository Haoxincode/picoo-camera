//! Explicit software-MF diagnostic readback followed by native fixture upload.
//! REQ-PICOO-MEDIA-032: excluded from the production Decoder feature graph.
use bytes::Bytes;
use picoo_bitstream::AvcSpsFacts;
use windows::core::Interface;
use windows::Win32::Foundation::E_NOINTERFACE;
use windows::Win32::Media::MediaFoundation::{
    IMF2DBuffer2, IMFMediaBuffer, IMFSample, IMFTransform, MF2DBuffer_LockFlags_Read,
    MFGetStrideForBitmapInfoHeader, MFVideoFormat_NV12, MF_E_ATTRIBUTENOTFOUND,
    MF_MT_DEFAULT_STRIDE, MF_MT_FRAME_SIZE,
};

use crate::{mf_nv12::copy_visible_nv12, now_timestamp_us, DecodeError, DecodedFrame};

fn platform(error: windows::core::Error) -> DecodeError {
    DecodeError::Platform(format!("MF output allocation: {error}"))
}

enum BufferLock<'a> {
    Surface(IMF2DBuffer2),
    Linear(&'a IMFMediaBuffer),
}

impl Drop for BufferLock<'_> {
    fn drop(&mut self) {
        unsafe {
            match self {
                Self::Surface(buffer) => {
                    let _ = buffer.Unlock2D();
                }
                Self::Linear(buffer) => {
                    let _ = buffer.Unlock();
                }
            }
        }
    }
}

pub(super) unsafe fn sample_to_frame(
    sample: &IMFSample,
    transform: &IMFTransform,
    facts: &AvcSpsFacts,
) -> Result<DecodedFrame, DecodeError> {
    let media_type = transform.GetOutputCurrentType(0).map_err(platform)?;
    let size = media_type.GetUINT64(&MF_MT_FRAME_SIZE).map_err(platform)?;
    if size != super::pack_frame_size(facts.coded_width, facts.coded_height) {
        return Err(DecodeError::ConfigurationMismatch);
    }
    if sample.GetBufferCount().map_err(platform)? != 1 {
        return Err(DecodeError::Platform(
            "NV12 output requires one native allocation".into(),
        ));
    }
    let buffer = sample.GetBufferByIndex(0).map_err(platform)?;
    let mut scanline = std::ptr::null_mut();
    let mut start = std::ptr::null_mut();
    let mut len = 0;
    let mut pitch = 0;
    let guard = match buffer.cast::<IMF2DBuffer2>() {
        Ok(surface) => {
            surface
                .Lock2DSize(
                    MF2DBuffer_LockFlags_Read,
                    &mut scanline,
                    &mut pitch,
                    &mut start,
                    &mut len,
                )
                .map_err(platform)?;
            BufferLock::Surface(surface)
        }
        Err(error) if error.code() == E_NOINTERFACE => {
            // IMFMediaBuffer::Lock exposes contiguous system memory. Its stride
            // is the media type's explicit default or the SDK's minimum stride.
            pitch = match media_type.GetUINT32(&MF_MT_DEFAULT_STRIDE) {
                Ok(stride) => stride as i32,
                Err(error) if error.code() == MF_E_ATTRIBUTENOTFOUND => {
                    MFGetStrideForBitmapInfoHeader(MFVideoFormat_NV12.data1, facts.coded_width)
                        .map_err(platform)?
                }
                Err(error) => return Err(platform(error)),
            };
            buffer
                .Lock(&mut start, None, Some(&mut len))
                .map_err(platform)?;
            scanline = start;
            BufferLock::Linear(&buffer)
        }
        Err(error) => return Err(platform(error)),
    };
    // Progressive NV12 is top-down. Reject unexpected layout rather than
    // interpreting bottom-up packed RGB rules as planar YUV rules.
    if start.is_null() || scanline.is_null() || pitch <= 0 {
        return Err(DecodeError::Platform(
            "invalid native NV12 pointers/pitch".into(),
        ));
    }
    let row_zero = (scanline as usize)
        .checked_sub(start as usize)
        .filter(|offset| *offset < len as usize)
        .ok_or_else(|| DecodeError::Platform("NV12 scanline outside locked allocation".into()))?;
    let source = std::slice::from_raw_parts(start, len as usize);
    let pixels = copy_visible_nv12(source, row_zero, pitch as usize, facts)?;
    drop(guard);
    DecodedFrame::fixture_nv12(
        facts.visible_width,
        facts.visible_height,
        facts.visible_width,
        0,
        now_timestamp_us(),
        Bytes::from(pixels),
    )
}
