//! Windows COM server boundary for `PicooVirtualCameraSource.dll`.

mod activator;
mod class_factory;
mod media_source;
mod media_stream;

use std::ffi::c_void;
use std::panic::{catch_unwind, AssertUnwindSafe};
use std::sync::atomic::{AtomicU32, Ordering};
use std::sync::{Mutex, MutexGuard};

use windows::core::{Error, Interface, Result, GUID, HRESULT};
use windows::Win32::Foundation::{E_POINTER, S_FALSE, S_OK};
use windows::Win32::System::Diagnostics::Debug::OutputDebugStringW;

use self::class_factory::ClassFactory;

pub const PICOO_VCAM_CLSID: GUID = GUID::from_u128(0xa7c4e2f1_8b3d_4c6a_9e5f_1d2c3b4a5e6f);

static ACTIVE_OBJECTS: AtomicU32 = AtomicU32::new(0);
static SERVER_LOCKS: AtomicU32 = AtomicU32::new(0);

// Keep the product name as an explicit UTF-16 PE payload for installer/bundle validation.
#[used]
static FRIENDLY_NAME_UTF16: [u16; 13] = [
    b'P' as u16,
    b'i' as u16,
    b'c' as u16,
    b'o' as u16,
    b'o' as u16,
    b' ' as u16,
    b'C' as u16,
    b'a' as u16,
    b'm' as u16,
    b'e' as u16,
    b'r' as u16,
    b'a' as u16,
    0,
];

pub(super) struct ObjectTracker;

impl ObjectTracker {
    pub fn new() -> Self {
        ACTIVE_OBJECTS.fetch_add(1, Ordering::AcqRel);
        Self
    }
}

impl Drop for ObjectTracker {
    fn drop(&mut self) {
        ACTIVE_OBJECTS.fetch_sub(1, Ordering::AcqRel);
    }
}

pub(super) fn set_server_lock(locked: bool) {
    if locked {
        SERVER_LOCKS.fetch_add(1, Ordering::AcqRel);
    } else {
        let _ = SERVER_LOCKS.fetch_update(Ordering::AcqRel, Ordering::Acquire, |count| {
            count.checked_sub(1)
        });
    }
}

pub(super) fn lock<T>(mutex: &Mutex<T>) -> Result<MutexGuard<'_, T>> {
    mutex
        .lock()
        .map_err(|_| Error::from(windows::Win32::Foundation::E_UNEXPECTED))
}

pub(super) fn emit_debug_message(message: &str) {
    let wide = message
        .encode_utf16()
        .chain(std::iter::once(0))
        .collect::<Vec<_>>();
    unsafe {
        OutputDebugStringW(windows::core::PCWSTR(wide.as_ptr()));
    }
}

pub(super) unsafe fn query_interface<T: Interface>(
    interface: &T,
    riid: *const GUID,
    output: *mut *mut c_void,
) -> Result<()> {
    if riid.is_null() || output.is_null() {
        return Err(Error::from(E_POINTER));
    }
    output.write(std::ptr::null_mut());
    interface.query(riid, output).ok()
}

fn guarded_hresult(operation: impl FnOnce() -> Result<()>) -> HRESULT {
    match catch_unwind(AssertUnwindSafe(operation)) {
        Ok(Ok(())) => S_OK,
        Ok(Err(error)) => error.code(),
        Err(_) => windows::Win32::Foundation::E_UNEXPECTED,
    }
}

/// Standard in-process COM server entry point.
///
/// # Safety
///
/// COM must pass valid readable `clsid`/`riid` pointers and a writable `output` pointer.
/// The function validates null pointers before dereferencing them.
#[no_mangle]
pub unsafe extern "system" fn DllGetClassObject(
    clsid: *const GUID,
    riid: *const GUID,
    output: *mut *mut c_void,
) -> HRESULT {
    guarded_hresult(|| {
        if clsid.is_null() || riid.is_null() || output.is_null() {
            return Err(Error::from(E_POINTER));
        }
        output.write(std::ptr::null_mut());
        if clsid.read() != PICOO_VCAM_CLSID {
            return Err(Error::from(
                windows::Win32::Foundation::CLASS_E_CLASSNOTAVAILABLE,
            ));
        }
        let factory = ClassFactory::create();
        query_interface(&factory, riid, output)
    })
}

/// COM may unload the DLL only after all objects and explicit server locks are gone.
#[no_mangle]
pub extern "system" fn DllCanUnloadNow() -> HRESULT {
    if ACTIVE_OBJECTS.load(Ordering::Acquire) == 0 && SERVER_LOCKS.load(Ordering::Acquire) == 0 {
        S_OK
    } else {
        S_FALSE
    }
}
