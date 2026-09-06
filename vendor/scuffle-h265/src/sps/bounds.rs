//! Checked crop arithmetic shared by SPS conformance and VUI display windows.
use std::io;

pub(super) fn remaining(size: u64, before: u64, after: u64, scale: u8) -> io::Result<u64> {
    before.checked_add(after)
        .and_then(|sum| sum.checked_mul(u64::from(scale)))
        .and_then(|crop| size.checked_sub(crop))
        .filter(|visible| *visible > 0)
        .ok_or_else(|| io::Error::new(io::ErrorKind::InvalidData, "invalid HEVC crop"))
}
