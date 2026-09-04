use crate::handles::{write_field, BrowserInner, RecoverMutex};
use picoo_discovery::MdnsBrowser;
use std::ffi::CStr;
use std::sync::Mutex;
use std::time::Duration;

#[repr(C)]
#[derive(Clone, Copy)]
pub struct PicooDiscoveredReceiver {
    pub receiver_id: [u8; 64],
    pub display_name: [u8; 64],
    pub host: [u8; 64],
    pub quic_port: u16,
    /// TXT `platform` (`windows` / `macos`).
    pub platform: [u8; 16],
    /// TXT `pairing_state` (`open` / `paired_only`); empty if unknown.
    pub pairing_state: [u8; 32],
}

/// Create mDNS browser for receiver discovery (PUC-002).
#[no_mangle]
pub extern "C" fn picoo_discovery_browser_create() -> *mut std::ffi::c_void {
    discovery_browser_handle(MdnsBrowser::new())
}

/// Create a browser restricted to a platform-selected physical LAN interface.
#[no_mangle]
pub extern "C" fn picoo_discovery_browser_create_on_interface(
    interface_name: *const std::ffi::c_char,
) -> *mut std::ffi::c_void {
    if interface_name.is_null() {
        return std::ptr::null_mut();
    }
    let interface_name = unsafe { CStr::from_ptr(interface_name) }.to_string_lossy();
    discovery_browser_handle(MdnsBrowser::new_on_interface(&interface_name))
}

fn discovery_browser_handle(
    browser: Result<MdnsBrowser, picoo_discovery::BrowseError>,
) -> *mut std::ffi::c_void {
    match browser {
        Ok(browser) => Box::into_raw(Box::new(BrowserInner {
            browser: Mutex::new(browser),
            receivers: Mutex::new(Vec::new()),
        })) as *mut std::ffi::c_void,
        Err(_) => std::ptr::null_mut(),
    }
}

#[no_mangle]
pub extern "C" fn picoo_discovery_browser_destroy(handle: *mut std::ffi::c_void) {
    if handle.is_null() {
        return;
    }
    unsafe {
        drop(Box::from_raw(handle as *mut BrowserInner));
    }
}

/// Poll mDNS events; refreshes cached receiver list.
#[no_mangle]
pub extern "C" fn picoo_discovery_browser_poll(
    handle: *mut std::ffi::c_void,
    timeout_ms: u32,
) -> i32 {
    if handle.is_null() {
        return -1;
    }
    let inner = unsafe { &*(handle as *mut BrowserInner) };
    let mut browser = inner.browser.lock_or_recover();
    if browser
        .poll(Duration::from_millis(timeout_ms as u64))
        .is_err()
    {
        return -2;
    }
    let mut cached = inner.receivers.lock_or_recover();
    cached.clear();
    for entry in browser.list() {
        let mut item = PicooDiscoveredReceiver {
            receiver_id: [0; 64],
            display_name: [0; 64],
            host: [0; 64],
            quic_port: entry.advertisement.quic_port,
            platform: [0; 16],
            pairing_state: [0; 32],
        };
        write_field(&mut item.receiver_id, &entry.advertisement.receiver_id);
        write_field(&mut item.display_name, &entry.advertisement.display_name);
        write_field(&mut item.host, &entry.host);
        write_field(&mut item.platform, entry.advertisement.platform.as_str());
        write_field(
            &mut item.pairing_state,
            entry.advertisement.pairing_state.as_str(),
        );
        cached.push(item);
    }
    cached.len() as i32
}

#[no_mangle]
pub extern "C" fn picoo_discovery_browser_count(handle: *mut std::ffi::c_void) -> u32 {
    if handle.is_null() {
        return 0;
    }
    let inner = unsafe { &*(handle as *mut BrowserInner) };
    inner.receivers.lock_or_recover().len() as u32
}

#[no_mangle]
pub extern "C" fn picoo_discovery_browser_get(
    handle: *mut std::ffi::c_void,
    index: u32,
    out: *mut PicooDiscoveredReceiver,
) -> i32 {
    if handle.is_null() || out.is_null() {
        return -1;
    }
    let inner = unsafe { &*(handle as *mut BrowserInner) };
    let cached = inner.receivers.lock_or_recover();
    let Some(item) = cached.get(index as usize) else {
        return -2;
    };
    unsafe {
        *out = *item;
    }
    0
}
