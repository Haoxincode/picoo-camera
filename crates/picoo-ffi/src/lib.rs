//! C ABI entry points for mobile platforms — REQ-PICOO-STACK-003.

use picoo_session::ReceiverStatus;

/// Opaque handle placeholder for future session context.
pub struct PicooSessionHandle {
    pub status: ReceiverStatus,
}

/// Returns protocol version string for FFI smoke tests.
#[no_mangle]
pub extern "C" fn picoo_protocol_version() -> *const std::ffi::c_char {
    static VERSION: &[u8] = b"PCP/1\0";
    VERSION.as_ptr() as *const std::ffi::c_char
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn protocol_version_cstr() {
        let ptr = picoo_protocol_version();
        assert!(!ptr.is_null());
    }
}
