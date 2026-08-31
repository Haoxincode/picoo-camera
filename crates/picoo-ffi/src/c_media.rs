use picoo_packet::extract_sps_pps;
use picoo_rate_control::BitrateLadder;
use std::slice;

pub(crate) fn copy_parameter_sets(
    sps: *const u8,
    sps_len: usize,
    pps: *const u8,
    pps_len: usize,
) -> (Vec<u8>, Vec<u8>) {
    let sps_slice = if !sps.is_null() && sps_len > 0 {
        unsafe { slice::from_raw_parts(sps, sps_len) }
    } else {
        &[]
    };
    let pps_slice = if !pps.is_null() && pps_len > 0 {
        unsafe { slice::from_raw_parts(pps, pps_len) }
    } else {
        &[]
    };
    if !pps_slice.is_empty() {
        return (sps_slice.to_vec(), pps_slice.to_vec());
    }
    if let Some((s, p)) = extract_sps_pps(sps_slice) {
        return (s, p);
    }
    (sps_slice.to_vec(), Vec::new())
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

#[no_mangle]
pub extern "C" fn picoo_bitrate_initial_for_height(height: u32) -> u32 {
    BitrateLadder::for_height(height).initial_bps
}

#[no_mangle]
pub extern "C" fn picoo_bitrate_clamp_for_height(bitrate_bps: u32, height: u32) -> u32 {
    let ladder = BitrateLadder::for_height(height);
    bitrate_bps.clamp(ladder.min_bps, ladder.max_bps)
}

#[no_mangle]
pub extern "C" fn picoo_stream_epoch_initial() -> u32 {
    picoo_sender::INITIAL_STREAM_EPOCH
}
