//! C ABI entry points for mobile platforms — REQ-PICOO-STACK-003.

#![allow(clippy::not_unsafe_ptr_arg_deref)]

use picoo_sender::{SenderSession, SessionStats};
use picoo_session::ReceiverStatus;
use picoo_transport::{Endpoint, QuicSenderTransport};
use std::ffi::CStr;
use std::sync::Mutex;

/// Opaque handle placeholder for future session context.
pub struct PicooSessionHandle {
    pub status: ReceiverStatus,
}

struct SenderInner {
    session: Mutex<SenderSession<QuicSenderTransport>>,
}

/// Returns protocol version string for FFI smoke tests.
#[no_mangle]
pub extern "C" fn picoo_protocol_version() -> *const std::ffi::c_char {
    static VERSION: &[u8] = b"PCP/1\0";
    VERSION.as_ptr() as *const std::ffi::c_char
}

/// Create a sender session (packetization + QUIC transport).
#[no_mangle]
pub extern "C" fn picoo_sender_create() -> *mut std::ffi::c_void {
    Box::into_raw(Box::new(SenderInner {
        session: Mutex::new(SenderSession::new(QuicSenderTransport::new())),
    })) as *mut std::ffi::c_void
}

/// Destroy a sender session created by [`picoo_sender_create`].
#[no_mangle]
pub extern "C" fn picoo_sender_destroy(handle: *mut std::ffi::c_void) {
    if handle.is_null() {
        return;
    }
    unsafe {
        drop(Box::from_raw(handle as *mut SenderInner));
    }
}

/// Connect QUIC session to host:port (PCP/1 ALPN `picoocam/1`).
#[no_mangle]
pub extern "C" fn picoo_sender_connect(
    handle: *mut std::ffi::c_void,
    host: *const std::ffi::c_char,
    port: u16,
) -> i32 {
    if handle.is_null() || host.is_null() {
        return -1;
    }
    let host = unsafe { CStr::from_ptr(host) }.to_string_lossy();
    let inner = unsafe { &*(handle as *mut SenderInner) };
    let mut session = inner.session.lock().expect("sender lock");
    match session.connect(Endpoint {
        host: host.into_owned(),
        port,
    }) {
        Ok(_) => 0,
        Err(_) => -2,
    }
}

/// Drive QUIC I/O (call periodically from platform thread).
#[no_mangle]
pub extern "C" fn picoo_sender_pump(handle: *mut std::ffi::c_void) -> i32 {
    if handle.is_null() {
        return -1;
    }
    let inner = unsafe { &*(handle as *mut SenderInner) };
    match inner.session.lock().expect("sender lock").pump() {
        Ok(()) => 0,
        Err(_) => -2,
    }
}

/// Ingest one H.264 access unit. Returns 0 on success, negative on error.
#[no_mangle]
pub extern "C" fn picoo_sender_ingest_access_unit(
    handle: *mut std::ffi::c_void,
    data: *const u8,
    len: usize,
    is_keyframe: u8,
    pts_us: u64,
    stream_epoch: u32,
    out_packets: *mut u32,
) -> i32 {
    if handle.is_null() || data.is_null() || len == 0 {
        return -1;
    }

    let inner = unsafe { &*(handle as *mut SenderInner) };
    let slice = unsafe { std::slice::from_raw_parts(data, len) };
    let mut session = inner.session.lock().expect("sender lock");

    match session.ingest_access_unit(slice, is_keyframe != 0, pts_us, stream_epoch) {
        Ok(count) => {
            if !out_packets.is_null() {
                unsafe {
                    *out_packets = count as u32;
                }
            }
            0
        }
        Err(_) => -2,
    }
}

/// Flush pending VideoPackets over QUIC datagrams.
#[no_mangle]
pub extern "C" fn picoo_sender_flush(handle: *mut std::ffi::c_void, out_sent: *mut u32) -> i32 {
    if handle.is_null() {
        return -1;
    }
    let inner = unsafe { &*(handle as *mut SenderInner) };
    let mut session = inner.session.lock().expect("sender lock");
    match session.flush_pending() {
        Ok(sent) => {
            if !out_sent.is_null() {
                unsafe {
                    *out_sent = sent as u32;
                }
            }
            0
        }
        Err(_) => -2,
    }
}

#[repr(C)]
pub struct PicooSenderStats {
    pub access_units: u64,
    pub packets: u64,
    pub bytes: u64,
    pub sent_datagrams: u64,
}

/// Read cumulative sender stats.
#[no_mangle]
pub extern "C" fn picoo_sender_stats(
    handle: *mut std::ffi::c_void,
    out: *mut PicooSenderStats,
) -> i32 {
    if handle.is_null() || out.is_null() {
        return -1;
    }
    let inner = unsafe { &*(handle as *mut SenderInner) };
    let stats: SessionStats = inner.session.lock().expect("sender lock").stats();
    unsafe {
        (*out).access_units = stats.pipeline.access_units;
        (*out).packets = stats.pipeline.packets;
        (*out).bytes = stats.pipeline.bytes;
        (*out).sent_datagrams = stats.sent_datagrams;
    }
    0
}

/// Number of VideoPackets waiting for transport.
#[no_mangle]
pub extern "C" fn picoo_sender_pending_packets(handle: *mut std::ffi::c_void) -> u64 {
    if handle.is_null() {
        return 0;
    }
    let inner = unsafe { &*(handle as *mut SenderInner) };
    inner.session.lock().expect("sender lock").pending_packets() as u64
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn protocol_version_cstr() {
        let ptr = picoo_protocol_version();
        assert!(!ptr.is_null());
    }

    #[test]
    fn sender_ingest_via_ffi() {
        let handle = picoo_sender_create();
        assert!(!handle.is_null());
        let data = b"test-nalu";
        let mut out = 0u32;
        assert_eq!(
            picoo_sender_ingest_access_unit(handle, data.as_ptr(), data.len(), 1, 42, 1, &mut out),
            0
        );
        assert_eq!(out, 1);
        picoo_sender_destroy(handle);
    }
}
