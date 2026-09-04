use crate::handles::{copy_str_to_buf, write_field, RecoverMutex, TrustedStoreInner};
use picoo_pairing::{DeviceIdentity, TrustedDeviceStore};
use std::ffi::CStr;
use std::sync::Mutex;

#[repr(C)]
#[derive(Clone, Copy)]
pub struct PicooTrustedDevice {
    pub device_id: [u8; 64],
    pub device_name: [u8; 64],
    pub certificate_fingerprint: [u8; 128],
    pub paired_at_ms: u64,
    pub last_connected_at_ms: u64,
}

/// Load trusted device store from JSON path.
#[no_mangle]
pub extern "C" fn picoo_trusted_store_load(path: *const std::ffi::c_char) -> *mut std::ffi::c_void {
    if path.is_null() {
        return std::ptr::null_mut();
    }
    let path = unsafe { CStr::from_ptr(path) }.to_string_lossy();
    match TrustedDeviceStore::load_from_path(path.as_ref()) {
        Ok(store) => Box::into_raw(Box::new(TrustedStoreInner {
            store: Mutex::new(store),
            path: Mutex::new(Some(path.into_owned())),
        })) as *mut std::ffi::c_void,
        Err(_) => std::ptr::null_mut(),
    }
}

#[no_mangle]
pub extern "C" fn picoo_trusted_store_destroy(handle: *mut std::ffi::c_void) {
    if handle.is_null() {
        return;
    }
    unsafe {
        drop(Box::from_raw(handle as *mut TrustedStoreInner));
    }
}

#[no_mangle]
pub extern "C" fn picoo_trusted_store_count(handle: *mut std::ffi::c_void) -> u32 {
    if handle.is_null() {
        return 0;
    }
    let inner = unsafe { &*(handle as *mut TrustedStoreInner) };
    inner.store.lock_or_recover().list().count() as u32
}

#[no_mangle]
pub extern "C" fn picoo_trusted_store_get(
    handle: *mut std::ffi::c_void,
    index: u32,
    out: *mut PicooTrustedDevice,
) -> i32 {
    if handle.is_null() || out.is_null() {
        return -1;
    }
    let inner = unsafe { &*(handle as *mut TrustedStoreInner) };
    let store = inner.store.lock_or_recover();
    let Some(device) = store.list().nth(index as usize) else {
        return -2;
    };
    let mut item = PicooTrustedDevice {
        device_id: [0; 64],
        device_name: [0; 64],
        certificate_fingerprint: [0; 128],
        paired_at_ms: device.paired_at_ms,
        last_connected_at_ms: device.last_connected_at_ms.unwrap_or(0),
    };
    write_field(&mut item.device_id, &device.device_id);
    write_field(&mut item.device_name, &device.device_name);
    write_field(
        &mut item.certificate_fingerprint,
        &device.certificate_fingerprint,
    );
    unsafe {
        *out = item;
    }
    0
}

/// Remove device by id. Returns 1 if removed, 0 if not found, negative on error.
#[no_mangle]
pub extern "C" fn picoo_trusted_store_remove(
    handle: *mut std::ffi::c_void,
    device_id: *const std::ffi::c_char,
) -> i32 {
    if handle.is_null() || device_id.is_null() {
        return -1;
    }
    let device_id = unsafe { CStr::from_ptr(device_id) }.to_string_lossy();
    let inner = unsafe { &*(handle as *mut TrustedStoreInner) };
    let mut store = inner.store.lock_or_recover();
    if store.remove(&device_id) {
        1
    } else {
        0
    }
}

/// Clear every trusted device. Returns the number removed (≥0), or negative on error.
#[no_mangle]
pub extern "C" fn picoo_trusted_store_clear(handle: *mut std::ffi::c_void) -> i32 {
    if handle.is_null() {
        return -1;
    }
    let inner = unsafe { &*(handle as *mut TrustedStoreInner) };
    let mut store = inner.store.lock_or_recover();
    store.clear() as i32
}

#[no_mangle]
pub extern "C" fn picoo_trusted_store_save(handle: *mut std::ffi::c_void) -> i32 {
    if handle.is_null() {
        return -1;
    }
    let inner = unsafe { &*(handle as *mut TrustedStoreInner) };
    let path = inner.path.lock_or_recover().clone();
    let Some(path) = path else {
        return -2;
    };
    match inner.store.lock_or_recover().save_to_path(&path) {
        Ok(()) => 0,
        Err(_) => -3,
    }
}

/// Load or create durable sender identity at `path` (REQ-PICOO-PAIRING-001).
#[no_mangle]
pub extern "C" fn picoo_identity_load_or_create(
    path: *const std::ffi::c_char,
    default_name: *const std::ffi::c_char,
) -> *mut std::ffi::c_void {
    if path.is_null() {
        return std::ptr::null_mut();
    }
    #[cfg(not(target_os = "ios"))]
    let path = unsafe { CStr::from_ptr(path) }.to_string_lossy();
    let default_name = if default_name.is_null() {
        "Picoo Phone".to_string()
    } else {
        unsafe { CStr::from_ptr(default_name) }
            .to_string_lossy()
            .into_owned()
    };
    #[cfg(target_os = "ios")]
    let identity = DeviceIdentity::load_or_create_system(
        "site.nebula-tech.picoo-camera",
        "sender-ed25519",
        &default_name,
    );
    #[cfg(not(target_os = "ios"))]
    let identity = DeviceIdentity::load_or_create(path.as_ref(), &default_name);
    match identity {
        Ok(identity) => Box::into_raw(Box::new(identity)) as *mut std::ffi::c_void,
        Err(_) => std::ptr::null_mut(),
    }
}

#[no_mangle]
pub extern "C" fn picoo_identity_destroy(handle: *mut std::ffi::c_void) {
    if handle.is_null() {
        return;
    }
    unsafe {
        drop(Box::from_raw(handle as *mut DeviceIdentity));
    }
}

#[no_mangle]
pub extern "C" fn picoo_identity_device_id(
    handle: *mut std::ffi::c_void,
    out: *mut std::ffi::c_char,
    out_len: usize,
) -> i32 {
    if handle.is_null() {
        return -1;
    }
    let identity = unsafe { &*(handle as *mut DeviceIdentity) };
    copy_str_to_buf(identity.device_id(), out, out_len)
}

#[no_mangle]
pub extern "C" fn picoo_identity_device_name(
    handle: *mut std::ffi::c_void,
    out: *mut std::ffi::c_char,
    out_len: usize,
) -> i32 {
    if handle.is_null() {
        return -1;
    }
    let identity = unsafe { &*(handle as *mut DeviceIdentity) };
    copy_str_to_buf(identity.device_name(), out, out_len)
}

/// Copy public key bytes into `out`. Returns length, or negative on error.
#[no_mangle]
pub extern "C" fn picoo_identity_public_key(
    handle: *mut std::ffi::c_void,
    out: *mut u8,
    out_len: usize,
) -> i32 {
    if handle.is_null() || out.is_null() {
        return -1;
    }
    let identity = unsafe { &*(handle as *mut DeviceIdentity) };
    let key = identity.public_key();
    if out_len < key.len() {
        return -2;
    }
    unsafe {
        std::ptr::copy_nonoverlapping(key.as_ptr(), out, key.len());
    }
    key.len() as i32
}

/// Persist identity after renaming display name.
#[no_mangle]
pub extern "C" fn picoo_identity_set_device_name(
    handle: *mut std::ffi::c_void,
    name: *const std::ffi::c_char,
    path: *const std::ffi::c_char,
) -> i32 {
    if handle.is_null() || name.is_null() || path.is_null() {
        return -1;
    }
    let name = unsafe { CStr::from_ptr(name) }.to_string_lossy();
    let path = unsafe { CStr::from_ptr(path) }.to_string_lossy();
    let identity = unsafe { &mut *(handle as *mut DeviceIdentity) };
    identity.set_device_name(&name);
    match identity.save_to_path(path.as_ref()) {
        Ok(()) => 0,
        Err(_) => -2,
    }
}
