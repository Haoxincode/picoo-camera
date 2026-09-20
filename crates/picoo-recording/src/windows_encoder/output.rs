//! Bounded compressed output copy and bitstream/configuration verification.

use super::{platform, timestamp};
use crate::RecordingError;
use picoo_bitstream::{
    AccessUnit, Codec, CodecConfiguration, NalFormat, PictureKind, RandomAccessPoint,
};
use windows::Win32::Media::MediaFoundation::{IMFSample, IMFTransform, MF_MT_MPEG_SEQUENCE_HEADER};

pub struct EncodedFrame {
    pub data: Vec<u8>,
    pub configuration: CodecConfiguration,
    pub pts_us: u64,
}

pub(super) fn read(
    transform: &IMFTransform,
    sample: &IMFSample,
    codec: Codec,
    width: u32,
    height: u32,
    expected_pts_us: u64,
    force_idr: bool,
) -> Result<EncodedFrame, RecordingError> {
    unsafe {
        if platform(sample.GetSampleTime())? != timestamp(expected_pts_us)? {
            return Err(invalid());
        }
        let media = platform(transform.GetOutputCurrentType(0))?;
        let size = platform(media.GetBlobSize(&MF_MT_MPEG_SEQUENCE_HEADER))? as usize;
        if size == 0 || size > 64 * 1024 {
            return Err(invalid());
        }
        let mut sequence = vec![0; size];
        let mut written = 0;
        platform(media.GetBlob(
            &MF_MT_MPEG_SEQUENCE_HEADER,
            &mut sequence,
            Some(&mut written),
        ))?;
        if written as usize != sequence.len() {
            return Err(invalid());
        }
        let configuration = match codec {
            Codec::Avc => CodecConfiguration::from_avc_annex_b(&sequence),
            Codec::Hevc => CodecConfiguration::from_hevc_annex_b(&sequence),
        }
        .map_err(|_| invalid())?;
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
                != match codec {
                    Codec::Avc => 100,
                    Codec::Hevc => 1,
                }
        {
            return Err(invalid());
        }

        let buffer = platform(sample.ConvertToContiguousBuffer())?;
        let length = platform(buffer.GetCurrentLength())? as usize;
        if length == 0 || length > 2 * 1024 * 1024 {
            return Err(invalid());
        }
        let mut pointer = std::ptr::null_mut();
        let mut current = 0;
        platform(buffer.Lock(&mut pointer, None, Some(&mut current)))?;
        struct Unlock<'a>(&'a windows::Win32::Media::MediaFoundation::IMFMediaBuffer);
        impl Drop for Unlock<'_> {
            fn drop(&mut self) {
                unsafe {
                    let _ = self.0.Unlock();
                }
            }
        }
        let _unlock = Unlock(&buffer);
        if pointer.is_null() || current as usize != length {
            return Err(invalid());
        }
        let annex_b = std::slice::from_raw_parts(pointer, length);
        let access_unit =
            AccessUnit::parse(codec, NalFormat::AnnexB, annex_b).map_err(|_| invalid())?;
        configuration
            .validate_parameter_sets(&access_unit)
            .map_err(|_| invalid())?;
        let idr = matches!(
            access_unit.picture().kind,
            PictureKind::RandomAccess(RandomAccessPoint::AvcIdr | RandomAccessPoint::HevcIdr)
        );
        if force_idr && !idr {
            return Err(invalid());
        }
        let data = access_unit.to_length_prefixed().map_err(|_| invalid())?;
        Ok(EncodedFrame {
            data,
            configuration,
            pts_us: expected_pts_us,
        })
    }
}

fn invalid() -> RecordingError {
    RecordingError::InvalidInput("invalid hardware encoder output")
}
