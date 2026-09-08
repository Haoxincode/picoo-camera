use super::status;
use crate::RecordingError;
use objc2_core_foundation::{CFData, CFDictionary, CFString, CFType};
use objc2_core_media::{
    kCMFormatDescriptionExtension_SampleDescriptionExtensionAtoms, CMSampleBuffer, CMTimeFlags,
};
use picoo_bitstream::{
    AccessUnit, Codec, CodecConfiguration, NalFormat, PictureKind, RandomAccessPoint,
};
use std::ptr::NonNull;

pub struct EncodedFrame {
    pub data: Vec<u8>,
    pub configuration: CodecConfiguration,
    pub pts_us: u64,
}

pub(super) fn validate(
    frame: &EncodedFrame,
    width: u32,
    height: u32,
    force_idr: bool,
) -> Result<(), RecordingError> {
    let invalid = || RecordingError::InvalidInput("hardware output differs from requested format");
    let configuration = &frame.configuration;
    configuration
        .validate_visible_size(width, height)
        .map_err(|_| invalid())?;
    let facts = configuration.source_facts().map_err(|_| invalid())?;
    let color = facts.color.ok_or_else(invalid)?;
    if (
        color.primaries,
        color.transfer,
        color.matrix,
        color.full_range,
    ) != (1, 1, 1, false)
        || configuration.profile_idc()
            != match configuration.codec() {
                Codec::Avc => 100,
                Codec::Hevc => 1,
            }
    {
        return Err(invalid());
    }
    let au = AccessUnit::parse(
        configuration.codec(),
        NalFormat::LengthPrefixed(configuration.nal_length_size()),
        &frame.data,
    )
    .map_err(|_| invalid())?;
    let idr = matches!(
        au.picture().kind,
        PictureKind::RandomAccess(RandomAccessPoint::AvcIdr | RandomAccessPoint::HevcIdr)
    );
    if force_idr && !idr {
        return Err(invalid());
    }
    Ok(())
}

/// Read-only callback data; all copies are bounded before allocation. No raw
/// CMSampleBuffer crosses the callback/thread boundary.
pub(super) unsafe fn copy(
    sample: *mut CMSampleBuffer,
    codec: Codec,
    expected_pts: u64,
) -> Result<EncodedFrame, RecordingError> {
    let invalid = || RecordingError::InvalidInput("invalid hardware encoder output");
    unsafe {
        let sample = sample.as_ref().ok_or_else(invalid)?;
        if sample.num_samples() != 1 {
            return Err(invalid());
        }
        let time = sample.presentation_time_stamp();
        if time.flags != CMTimeFlags::Valid
            || time.timescale <= 0
            || time.value < 0
            || time.epoch != 0
        {
            return Err(invalid());
        }
        let pts = i128::from(time.value) * 1_000_000 / i128::from(time.timescale);
        if pts != i128::from(expected_pts) {
            return Err(invalid());
        }
        let format = sample.format_description().ok_or_else(invalid)?;
        let atoms = format
            .extension(kCMFormatDescriptionExtension_SampleDescriptionExtensionAtoms)
            .ok_or_else(invalid)?
            .downcast::<CFDictionary>()
            .map_err(|_| invalid())?;
        let name = CFString::from_str(match codec {
            Codec::Avc => "avcC",
            Codec::Hevc => "hvcC",
        });
        let raw = atoms.value((name.as_ref() as *const CFString).cast());
        let record = raw
            .cast::<CFType>()
            .as_ref()
            .and_then(|value| value.downcast_ref::<CFData>())
            .ok_or_else(invalid)?;
        let record = record.as_bytes_unchecked();
        if record.is_empty() || record.len() > 64 * 1024 {
            return Err(invalid());
        }
        let configuration =
            CodecConfiguration::parse(codec, record.to_vec().into()).map_err(|_| invalid())?;
        let buffer = sample.data_buffer().ok_or_else(invalid)?;
        let length = buffer.data_length();
        if length == 0 || length > 2 * 1024 * 1024 {
            return Err(invalid());
        }
        let mut data = vec![0; length];
        status(
            buffer.copy_data_bytes(
                0,
                length,
                NonNull::new(data.as_mut_ptr().cast()).ok_or_else(invalid)?,
            ),
            "copy compressed output",
        )?;
        let au = AccessUnit::parse(
            codec,
            NalFormat::LengthPrefixed(configuration.nal_length_size()),
            &data,
        )
        .map_err(|_| invalid())?;
        configuration
            .validate_parameter_sets(&au)
            .map_err(|_| invalid())?;
        Ok(EncodedFrame {
            data,
            configuration,
            pts_us: expected_pts,
        })
    }
}
