//! System synchronous codec discovery; hardware admission is a separate device contract.
use crate::DecodeError;
use picoo_bitstream::Codec;
use windows::Win32::Media::MediaFoundation::*;
use windows::Win32::System::Com::{CoCreateInstance, CoTaskMemFree, CLSCTX_INPROC_SERVER};

pub(super) fn create(codec: Codec) -> Result<IMFTransform, DecodeError> {
    let transform = match codec {
        Codec::Avc => unsafe { CoCreateInstance(&CMSH264DecoderMFT, None, CLSCTX_INPROC_SERVER) }
            .map_err(|error| DecodeError::Platform(format!("create AVC MFT: {error}")))?,
        Codec::Hevc => hevc()?,
    };
    unsafe {
        transform
            .GetAttributes()
            .and_then(|attributes| attributes.SetUINT32(&MF_LOW_LATENCY, 1))
            .map_err(|error| DecodeError::Platform(format!("enable MF low latency: {error}")))?;
    }
    Ok(transform)
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
        // MFTEnumEx returns count initialized COM interface slots in CoTaskMem.
        unsafe {
            for index in 0..self.count as usize {
                std::ptr::drop_in_place(self.pointer.add(index));
            }
            CoTaskMemFree(Some(self.pointer.cast()));
        }
    }
}

fn hevc() -> Result<IMFTransform, DecodeError> {
    let input = MFT_REGISTER_TYPE_INFO {
        guidMajorType: MFMediaType_Video,
        guidSubtype: MFVideoFormat_HEVC,
    };
    let output = MFT_REGISTER_TYPE_INFO {
        guidMajorType: MFMediaType_Video,
        guidSubtype: MFVideoFormat_NV12,
    };
    let mut activations = Activations {
        pointer: std::ptr::null_mut(),
        count: 0,
    };
    unsafe {
        MFTEnumEx(
            MFT_CATEGORY_VIDEO_DECODER,
            MFT_ENUM_FLAG_SYNCMFT | MFT_ENUM_FLAG_SORTANDFILTER,
            Some(&input),
            Some(&output),
            &mut activations.pointer,
            &mut activations.count,
        )
        .map_err(|error| DecodeError::Platform(format!("enumerate HEVC MFT: {error}")))?;
    }
    if activations.pointer.is_null() || activations.count == 0 {
        return Err(DecodeError::Platform(
            "system has no synchronous HEVC/NV12 decoder".into(),
        ));
    }
    // Windows sorts candidates by system preference. Bound attempted activations;
    // the owner still validates device binding and the committed media types.
    for index in 0..activations.count.min(32) as usize {
        let activation = unsafe { &*activations.pointer.add(index) };
        if let Some(activation) = activation {
            if let Ok(transform) = unsafe { activation.ActivateObject::<IMFTransform>() } {
                return Ok(transform);
            }
        }
    }
    Err(DecodeError::Platform(
        "no system HEVC decoder could be activated".into(),
    ))
}
