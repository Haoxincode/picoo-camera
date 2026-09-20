//! CoreMedia compressed samples; the only copy is of bounded compressed bytes.
use crate::RecordingError;
use objc2_core_foundation::{
    CFBoolean, CFData, CFDictionary, CFMutableDictionary, CFRetained, CFString, CFType,
};
use objc2_core_media::{
    kCMFormatDescriptionExtension_SampleDescriptionExtensionAtoms, kCMSampleAttachmentKey_NotSync,
    kCMVideoCodecType_H264, kCMVideoCodecType_HEVC, CMBlockBuffer, CMFormatDescription,
    CMSampleBuffer, CMSampleTimingInfo, CMTime, CMTimeFlags, CMVideoFormatDescriptionCreate,
};
use picoo_bitstream::{Codec, CodecConfiguration};
use std::{
    ffi::c_void,
    ptr::{self, NonNull},
};

pub(super) fn time(pts_us: u64) -> Result<CMTime, RecordingError> {
    Ok(CMTime {
        value: i64::try_from(pts_us)
            .map_err(|_| RecordingError::InvalidInput("timestamp overflow"))?,
        timescale: 1_000_000,
        flags: CMTimeFlags::Valid,
        epoch: 0,
    })
}

fn checked(operation: &str, status: i32) -> Result<(), RecordingError> {
    if status == 0 {
        Ok(())
    } else {
        Err(RecordingError::Platform(format!(
            "{operation}: OSStatus {status}"
        )))
    }
}

pub(super) fn format(
    configuration: &CodecConfiguration,
) -> Result<CFRetained<CMFormatDescription>, RecordingError> {
    let facts = configuration
        .source_facts()
        .map_err(|error| RecordingError::Platform(error.to_string()))?;
    let width = i32::try_from(facts.visible_width)
        .map_err(|_| RecordingError::InvalidInput("width overflow"))?;
    let height = i32::try_from(facts.visible_height)
        .map_err(|_| RecordingError::InvalidInput("height overflow"))?;
    if width <= 0 || height <= 0 {
        return Err(RecordingError::InvalidInput("empty video dimensions"));
    }
    let atom = CFString::from_str(match configuration.codec() {
        Codec::Avc => "avcC",
        Codec::Hevc => "hvcC",
    });
    let record = CFData::from_bytes(configuration.record());
    let atoms = CFDictionary::<CFString, CFData>::from_slices(&[atom.as_ref()], &[record.as_ref()]);
    let extensions = CFDictionary::<CFString, CFType>::from_slices(
        &[unsafe { kCMFormatDescriptionExtension_SampleDescriptionExtensionAtoms }],
        &[atoms.as_ref()],
    );
    let mut raw = ptr::null();
    // SAFETY: Validated standard configuration atoms, positive dimensions, and
    // live CF dictionaries. CoreMedia copies the description into a +1 result.
    checked("create format", unsafe {
        CMVideoFormatDescriptionCreate(
            None,
            match configuration.codec() {
                Codec::Avc => kCMVideoCodecType_H264,
                Codec::Hevc => kCMVideoCodecType_HEVC,
            },
            width,
            height,
            Some(extensions.as_opaque()),
            NonNull::from(&mut raw),
        )
    })?;
    let raw = NonNull::new(raw.cast_mut())
        .ok_or_else(|| RecordingError::Platform("missing format".into()))?;
    Ok(unsafe { CFRetained::from_raw(raw) })
}

pub(super) fn create(
    data: &[u8],
    format: &CMFormatDescription,
    pts_us: u64,
    fps: u32,
    sync: bool,
) -> Result<CFRetained<CMSampleBuffer>, RecordingError> {
    let mut raw_block = ptr::null_mut();
    // SAFETY: A null data pointer asks CoreMedia to allocate the bounded AU length.
    checked("allocate compressed block", unsafe {
        CMBlockBuffer::create_with_memory_block(
            None,
            ptr::null_mut(),
            data.len(),
            None,
            ptr::null(),
            0,
            data.len(),
            0,
            NonNull::from(&mut raw_block),
        )
    })?;
    let raw_block =
        NonNull::new(raw_block).ok_or_else(|| RecordingError::Platform("missing block".into()))?;
    // SAFETY: Create returns +1 ownership.
    let block = unsafe { CFRetained::from_raw(raw_block) };
    let source = NonNull::new(data.as_ptr().cast_mut().cast::<c_void>())
        .ok_or(RecordingError::InvalidInput("empty AU"))?;
    // SAFETY: The source and destination both contain data.len() accessible bytes.
    checked("copy compressed block", unsafe {
        CMBlockBuffer::replace_data_bytes(source, &block, 0, data.len())
    })?;
    let timing = CMSampleTimingInfo {
        duration: CMTime {
            value: 1,
            timescale: fps as i32,
            flags: CMTimeFlags::Valid,
            epoch: 0,
        },
        presentationTimeStamp: time(pts_us)?,
        decodeTimeStamp: CMTime {
            value: 0,
            timescale: 0,
            flags: CMTimeFlags::empty(),
            epoch: 0,
        },
    };
    let size = data.len();
    let mut raw_sample = ptr::null_mut();
    // SAFETY: One compressed sample, one time entry, one size entry. All objects
    // and arrays outlive creation; the sample retains block and format references.
    checked("create compressed sample", unsafe {
        CMSampleBuffer::create_ready(
            None,
            Some(&block),
            Some(format),
            1,
            1,
            &timing,
            1,
            &size,
            NonNull::from(&mut raw_sample),
        )
    })?;
    let raw_sample = NonNull::new(raw_sample)
        .ok_or_else(|| RecordingError::Platform("missing sample".into()))?;
    let sample = unsafe { CFRetained::from_raw(raw_sample) };
    // SAFETY: CoreMedia documents an immutable array of mutable dictionaries,
    // one per sample. This new sample has exactly one entry and no other owners.
    unsafe {
        let attachments = sample
            .sample_attachments_array(true)
            .ok_or_else(|| RecordingError::Platform("missing sample attachments".into()))?;
        let dictionary = attachments
            .value_at_index(0)
            .cast::<CFMutableDictionary>()
            .as_ref()
            .ok_or_else(|| RecordingError::Platform("missing sample dictionary".into()))?;
        let not_sync = CFBoolean::new(!sync);
        CFMutableDictionary::set_value(
            Some(dictionary),
            (kCMSampleAttachmentKey_NotSync as *const CFString).cast(),
            (not_sync as *const CFBoolean).cast(),
        );
    }
    Ok(sample)
}
