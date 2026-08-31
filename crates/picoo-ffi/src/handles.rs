use picoo_discovery::MdnsBrowser;
use picoo_pairing::TrustedDeviceStore;
use picoo_sender::SenderSession;
use picoo_transport::QuicSenderTransport;
use std::sync::{Mutex, MutexGuard};

use crate::c_discovery::PicooDiscoveredReceiver;

/// Opaque handle placeholder for future session context.
pub struct PicooSessionHandle {
    pub status: picoo_session::ReceiverStatus,
}

pub(crate) struct BrowserInner {
    pub(crate) browser: Mutex<MdnsBrowser>,
    pub(crate) receivers: Mutex<Vec<PicooDiscoveredReceiver>>,
}

/// Never unwind through C/JNI after an earlier host callback poisoned a lock.
/// The next ABI call recovers ownership and can return its documented status.
pub(crate) trait RecoverMutex<T> {
    fn lock_or_recover(&self) -> MutexGuard<'_, T>;
}

impl<T> RecoverMutex<T> for Mutex<T> {
    fn lock_or_recover(&self) -> MutexGuard<'_, T> {
        self.lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
    }
}

pub(crate) fn copy_str_to_buf(value: &str, out: *mut std::ffi::c_char, out_len: usize) -> i32 {
    if out.is_null() || out_len == 0 {
        return -1;
    }
    let bytes = value.as_bytes();
    let copy_len = bytes.len().min(out_len.saturating_sub(1));
    unsafe {
        std::ptr::copy_nonoverlapping(bytes.as_ptr(), out as *mut u8, copy_len);
        *out.add(copy_len) = 0;
    }
    copy_len as i32
}

pub(crate) struct SenderInner {
    pub(crate) session: Mutex<SenderSession<QuicSenderTransport>>,
}

pub(crate) fn write_field(buf: &mut [u8], value: &str) {
    let bytes = value.as_bytes();
    let copy_len = bytes.len().min(buf.len().saturating_sub(1));
    buf[..copy_len].copy_from_slice(&bytes[..copy_len]);
    if copy_len < buf.len() {
        buf[copy_len] = 0;
    }
}

pub(crate) struct TrustedStoreInner {
    pub(crate) store: Mutex<TrustedDeviceStore>,
    pub(crate) path: Mutex<Option<String>>,
}
