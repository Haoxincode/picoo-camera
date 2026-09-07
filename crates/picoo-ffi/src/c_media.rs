use picoo_bitstream::avc::extract_sps_pps;
use picoo_rate_control::BitrateLadder;
use std::slice;

/// Native Apple adapter supplies raw parameter NALs with explicit lengths.
pub(crate) fn configuration_from_raw_avc(
    sps: *const u8,
    sps_len: usize,
    pps: *const u8,
    pps_len: usize,
) -> Result<picoo_bitstream::CodecConfiguration, picoo_bitstream::BitstreamError> {
    if sps_len.saturating_add(pps_len).saturating_add(11) > 64 * 1024 {
        return Err(picoo_bitstream::BitstreamError::Limit);
    }
    if sps.is_null() || pps.is_null() || sps_len == 0 || pps_len == 0 {
        return Err(picoo_bitstream::BitstreamError::Malformed(
            "missing native AVC parameter sets",
        ));
    }
    // The FFI caller owns both buffers for this call; bounds are checked before
    // materialization. No Annex B guessing or configuration mutation occurs here.
    picoo_bitstream::CodecConfiguration::from_avc_parameter_sets(
        unsafe { slice::from_raw_parts(sps, sps_len) },
        unsafe { slice::from_raw_parts(pps, pps_len) },
    )
}

/// Extract SPS/PPS from Annex-B or AVCC bytes into caller buffers (REQ-PICOO-PROTOCOL-005).
///
/// Returns 0 on success, negative on error. On success writes lengths into `*_len` in/out.
#[no_mangle]
pub extern "C" fn picoo_h264_extract_sps_pps(
    data: *const u8,
    data_len: usize,
    sps_out: *mut u8,
    sps_len: *mut usize,
    pps_out: *mut u8,
    pps_len: *mut usize,
) -> i32 {
    if data.is_null() || data_len == 0 || sps_len.is_null() || pps_len.is_null() {
        return -1;
    }
    let slice = unsafe { slice::from_raw_parts(data, data_len) };
    let Some((sps, pps)) = extract_sps_pps(slice) else {
        return -2;
    };
    let sps_cap = unsafe { *sps_len };
    let pps_cap = unsafe { *pps_len };
    if sps.len() > sps_cap || pps.len() > pps_cap {
        unsafe {
            *sps_len = sps.len();
            *pps_len = pps.len();
        }
        return -3;
    }
    if !sps_out.is_null() {
        unsafe {
            std::ptr::copy_nonoverlapping(sps.as_ptr(), sps_out, sps.len());
        }
    }
    if !pps_out.is_null() {
        unsafe {
            std::ptr::copy_nonoverlapping(pps.as_ptr(), pps_out, pps.len());
        }
    }
    unsafe {
        *sps_len = sps.len();
        *pps_len = pps.len();
    }
    0
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

    #[test]
    fn native_raw_parameters_reject_missing_ambiguous_and_oversized_input() {
        let annex = include_bytes!("../../picoo-testkit/fixtures/avc-1280x720-bt709-idr.h264");
        let (sps, pps) = extract_sps_pps(annex).unwrap();
        assert!(
            configuration_from_raw_avc(sps.as_ptr(), sps.len(), pps.as_ptr(), pps.len()).is_ok()
        );
        assert!(
            configuration_from_raw_avc(annex.as_ptr(), annex.len(), std::ptr::null(), 0).is_err()
        );
        assert!(
            configuration_from_raw_avc(std::ptr::null(), usize::MAX, std::ptr::null(), 1).is_err()
        );
        let prefixed = [b"\0\0\0\x01".as_slice(), sps.as_slice()].concat();
        assert!(configuration_from_raw_avc(
            prefixed.as_ptr(),
            prefixed.len(),
            pps.as_ptr(),
            pps.len()
        )
        .is_err());
    }
}
