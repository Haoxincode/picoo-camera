use picoo_rate_control::BitrateLadder;
use std::slice;

/// Apple supplies the native avcC/hvcC atom with an explicit codec.
pub(crate) fn configuration_from_record(
    codec: picoo_bitstream::Codec,
    record: *const u8,
    length: usize,
) -> Result<picoo_bitstream::CodecConfiguration, picoo_bitstream::BitstreamError> {
    if length == 0 || length > 64 * 1024 {
        return Err(picoo_bitstream::BitstreamError::Limit);
    }
    if record.is_null() {
        return Err(picoo_bitstream::BitstreamError::Malformed(
            "missing native configuration record",
        ));
    }
    // FFI caller owns the bounded buffer throughout this synchronous copy.
    let bytes = unsafe { slice::from_raw_parts(record, length) };
    picoo_bitstream::CodecConfiguration::parse(codec, bytes.to_vec().into())
}

/// Return the initial bitrate for exact source height 720/1080, or 0 if unsupported.
#[no_mangle]
pub extern "C" fn picoo_bitrate_initial_for_height(height: u32) -> u32 {
    BitrateLadder::for_height(height).map_or(0, |bounds| bounds.initial_bps)
}

/// Clamp bitrate within exact source height 720/1080, or return 0 if unsupported.
#[no_mangle]
pub extern "C" fn picoo_bitrate_clamp_for_height(bitrate_bps: u32, height: u32) -> u32 {
    let Some(ladder) = BitrateLadder::for_height(height) else {
        return 0;
    };
    bitrate_bps.clamp(ladder.min_bps, ladder.max_bps)
}

#[no_mangle]
pub extern "C" fn picoo_stream_epoch_initial() -> u32 {
    picoo_sender::INITIAL_STREAM_EPOCH
}

#[cfg(test)]
mod configuration_tests {
    use super::*;
    use picoo_bitstream::Codec;
    #[test]
    fn native_records_reject_missing_wrong_codec_annex_b_and_oversized_input() {
        let record = include_bytes!("../../picoo-testkit/fixtures/hevc-64x64-bt709-config.bin");
        assert!(configuration_from_record(Codec::Hevc, record.as_ptr(), record.len()).is_ok());
        assert!(configuration_from_record(Codec::Avc, record.as_ptr(), record.len()).is_err());
        let annex = include_bytes!("../../picoo-testkit/fixtures/hevc-64x64-bt709-idr.h265");
        assert!(configuration_from_record(Codec::Hevc, annex.as_ptr(), annex.len()).is_err());
        assert!(configuration_from_record(Codec::Hevc, std::ptr::null(), 10).is_err());
        assert!(configuration_from_record(Codec::Hevc, std::ptr::null(), usize::MAX).is_err());
    }
}
