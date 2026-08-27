//! C ABI entry points for mobile platforms — REQ-PICOO-STACK-003.

#![allow(clippy::not_unsafe_ptr_arg_deref)]

use picoo_diagnostics::{build_report, export_json, DiagnosticInput};
use picoo_discovery::MdnsBrowser;
use picoo_pairing::TrustedDeviceStore;
use picoo_sender::{SenderSession, SessionStats, StreamConfigParams};
use picoo_session::SenderStatus;
use picoo_transport::{Endpoint, QuicSenderTransport};
use std::ffi::CStr;
use std::sync::Mutex;
use std::time::Duration;

/// Opaque handle placeholder for future session context.
pub struct PicooSessionHandle {
    pub status: picoo_session::ReceiverStatus,
}

struct BrowserInner {
    browser: Mutex<MdnsBrowser>,
    receivers: Mutex<Vec<PicooDiscoveredReceiver>>,
}

fn sender_status_code(status: SenderStatus) -> i32 {
    match status {
        SenderStatus::Disconnected => 0,
        SenderStatus::Discovering => 1,
        SenderStatus::Pairing => 2,
        SenderStatus::Connecting => 3,
        SenderStatus::Negotiating => 4,
        SenderStatus::Streaming => 5,
        SenderStatus::Reconnecting => 6,
        SenderStatus::PermissionRequired => 7,
        SenderStatus::NetworkUnstable => 8,
    }
}

fn copy_str_to_buf(value: &str, out: *mut std::ffi::c_char, out_len: usize) -> i32 {
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

/// Current sender session status (see `PicooSenderStatus` values).
#[no_mangle]
pub extern "C" fn picoo_sender_status(handle: *mut std::ffi::c_void) -> i32 {
    if handle.is_null() {
        return sender_status_code(SenderStatus::Disconnected);
    }
    let inner = unsafe { &*(handle as *mut SenderInner) };
    sender_status_code(inner.session.lock().expect("sender lock").status())
}

/// Send ClientHello after QUIC connect (PUC-001).
#[no_mangle]
pub extern "C" fn picoo_sender_send_client_hello(
    handle: *mut std::ffi::c_void,
    sender_id: *const std::ffi::c_char,
    device_name: *const std::ffi::c_char,
    public_key: *const u8,
    public_key_len: usize,
) -> i32 {
    if handle.is_null() || sender_id.is_null() || device_name.is_null() {
        return -1;
    }
    let sender_id = unsafe { CStr::from_ptr(sender_id) }.to_string_lossy();
    let device_name = unsafe { CStr::from_ptr(device_name) }.to_string_lossy();
    let key = if public_key.is_null() || public_key_len == 0 {
        &[][..]
    } else {
        unsafe { std::slice::from_raw_parts(public_key, public_key_len) }
    };
    let inner = unsafe { &*(handle as *mut SenderInner) };
    match inner
        .session
        .lock()
        .expect("sender lock")
        .send_client_hello(&sender_id, &device_name, key)
    {
        Ok(()) => 0,
        Err(_) => -2,
    }
}

/// Send PairingConfirm after desktop confirms six-digit code.
#[no_mangle]
pub extern "C" fn picoo_sender_send_pairing_confirm(
    handle: *mut std::ffi::c_void,
    receiver_id: *const std::ffi::c_char,
) -> i32 {
    if handle.is_null() || receiver_id.is_null() {
        return -1;
    }
    let receiver_id = unsafe { CStr::from_ptr(receiver_id) }.to_string_lossy();
    let inner = unsafe { &*(handle as *mut SenderInner) };
    match inner
        .session
        .lock()
        .expect("sender lock")
        .send_pairing_confirm(&receiver_id)
    {
        Ok(()) => 0,
        Err(_) => -2,
    }
}

/// Copy pairing short code into `out` buffer. Returns length, 0 if none, negative on error.
#[no_mangle]
pub extern "C" fn picoo_sender_pairing_short_code(
    handle: *mut std::ffi::c_void,
    out: *mut std::ffi::c_char,
    out_len: usize,
) -> i32 {
    if handle.is_null() {
        return -1;
    }
    let inner = unsafe { &*(handle as *mut SenderInner) };
    let session = inner.session.lock().expect("sender lock");
    match session.pairing_short_code() {
        Some(code) => copy_str_to_buf(code, out, out_len),
        None => 0,
    }
}

/// Configure stream parameters before/at streaming (PUC-005).
#[no_mangle]
pub extern "C" fn picoo_sender_set_stream_config(
    handle: *mut std::ffi::c_void,
    width: u32,
    height: u32,
    fps: u32,
    bitrate_bps: u32,
    stream_epoch: u32,
    mirrored: u8,
) -> i32 {
    if handle.is_null() {
        return -1;
    }
    let inner = unsafe { &*(handle as *mut SenderInner) };
    inner
        .session
        .lock()
        .expect("sender lock")
        .set_stream_config(StreamConfigParams {
            width,
            height,
            fps,
            bitrate_bps,
            stream_epoch,
            mirrored: mirrored != 0,
            ..StreamConfigParams::default()
        });
    0
}

/// Current adaptive bitrate in bps.
#[no_mangle]
pub extern "C" fn picoo_sender_current_bitrate_bps(handle: *mut std::ffi::c_void) -> u32 {
    if handle.is_null() {
        return 0;
    }
    let inner = unsafe { &*(handle as *mut SenderInner) };
    inner
        .session
        .lock()
        .expect("sender lock")
        .current_bitrate_bps()
}

/// Attach trusted device store path to sender (load + auto-save on pairing).
#[no_mangle]
pub extern "C" fn picoo_sender_attach_trusted_store(
    handle: *mut std::ffi::c_void,
    path: *const std::ffi::c_char,
) -> i32 {
    if handle.is_null() || path.is_null() {
        return -1;
    }
    let path = unsafe { CStr::from_ptr(path) }.to_string_lossy();
    let inner = unsafe { &*(handle as *mut SenderInner) };
    match inner
        .session
        .lock()
        .expect("sender lock")
        .attach_trusted_store(path.as_ref())
    {
        Ok(()) => 0,
        Err(_) => -2,
    }
}

/// Connected receiver id from ServerHello / pairing state.
#[no_mangle]
pub extern "C" fn picoo_sender_connected_receiver_id(
    handle: *mut std::ffi::c_void,
    out: *mut std::ffi::c_char,
    out_len: usize,
) -> i32 {
    if handle.is_null() {
        return -1;
    }
    let inner = unsafe { &*(handle as *mut SenderInner) };
    let session = inner.session.lock().expect("sender lock");
    match session.connected_receiver_id() {
        Some(id) => copy_str_to_buf(id, out, out_len),
        None => 0,
    }
}

#[repr(C)]
#[derive(Clone, Copy)]
pub struct PicooDiscoveredReceiver {
    pub receiver_id: [u8; 64],
    pub display_name: [u8; 64],
    pub host: [u8; 64],
    pub quic_port: u16,
}

fn write_field(buf: &mut [u8], value: &str) {
    let bytes = value.as_bytes();
    let copy_len = bytes.len().min(buf.len().saturating_sub(1));
    buf[..copy_len].copy_from_slice(&bytes[..copy_len]);
    if copy_len < buf.len() {
        buf[copy_len] = 0;
    }
}

struct TrustedStoreInner {
    store: Mutex<TrustedDeviceStore>,
    path: Mutex<Option<String>>,
}

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
    inner.store.lock().expect("store lock").list().count() as u32
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
    let store = inner.store.lock().expect("store lock");
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
    let mut store = inner.store.lock().expect("store lock");
    if store.remove(&device_id) {
        1
    } else {
        0
    }
}

#[no_mangle]
pub extern "C" fn picoo_trusted_store_save(handle: *mut std::ffi::c_void) -> i32 {
    if handle.is_null() {
        return -1;
    }
    let inner = unsafe { &*(handle as *mut TrustedStoreInner) };
    let store = inner.store.lock().expect("store lock");
    let path = inner.path.lock().expect("path lock");
    let Some(path) = path.as_deref() else {
        return -2;
    };
    match store.save_to_path(path) {
        Ok(()) => 0,
        Err(_) => -3,
    }
}

/// Create mDNS browser for receiver discovery (PUC-002).
#[no_mangle]
pub extern "C" fn picoo_discovery_browser_create() -> *mut std::ffi::c_void {
    match MdnsBrowser::new() {
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
    let mut browser = inner.browser.lock().expect("browser lock");
    if browser
        .poll(Duration::from_millis(timeout_ms as u64))
        .is_err()
    {
        return -2;
    }
    let mut cached = inner.receivers.lock().expect("cache lock");
    cached.clear();
    for entry in browser.list() {
        let mut item = PicooDiscoveredReceiver {
            receiver_id: [0; 64],
            display_name: [0; 64],
            host: [0; 64],
            quic_port: entry.advertisement.quic_port,
        };
        write_field(&mut item.receiver_id, &entry.advertisement.receiver_id);
        write_field(&mut item.display_name, &entry.advertisement.display_name);
        write_field(&mut item.host, &entry.host);
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
    inner.receivers.lock().expect("cache lock").len() as u32
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
    let cached = inner.receivers.lock().expect("cache lock");
    let Some(item) = cached.get(index as usize) else {
        return -2;
    };
    unsafe {
        *out = *item;
    }
    0
}

fn export_diagnostics_from_trusted_path(
    trusted_store_path: &str,
    platform: &str,
    app_version: &str,
) -> Result<String, i32> {
    let store = TrustedDeviceStore::load_from_path(trusted_store_path).map_err(|_| -2)?;
    let now_ms = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_millis() as u64)
        .unwrap_or(0);
    let report = build_report(DiagnosticInput {
        platform: platform.into(),
        app_version: app_version.into(),
        exported_at_ms: now_ms,
        trusted_devices: store.list().cloned().collect(),
        ..Default::default()
    });
    export_json(&report).map_err(|_| -3)
}

/// Export redacted diagnostics JSON to file — REQ-PICOO-PRIVACY-003.
#[no_mangle]
pub extern "C" fn picoo_export_diagnostics_to_path(
    trusted_store_path: *const std::ffi::c_char,
    platform: *const std::ffi::c_char,
    app_version: *const std::ffi::c_char,
    out_path: *const std::ffi::c_char,
) -> i32 {
    if trusted_store_path.is_null()
        || platform.is_null()
        || app_version.is_null()
        || out_path.is_null()
    {
        return -1;
    }
    let trusted_store_path = unsafe { CStr::from_ptr(trusted_store_path) }.to_string_lossy();
    let platform = unsafe { CStr::from_ptr(platform) }.to_string_lossy();
    let app_version = unsafe { CStr::from_ptr(app_version) }.to_string_lossy();
    let out_path = unsafe { CStr::from_ptr(out_path) }.to_string_lossy();
    match export_diagnostics_from_trusted_path(
        trusted_store_path.as_ref(),
        platform.as_ref(),
        app_version.as_ref(),
    ) {
        Ok(json) => match std::fs::write(out_path.as_ref(), json) {
            Ok(()) => 0,
            Err(_) => -4,
        },
        Err(code) => code,
    }
}

/// Copy redacted diagnostics JSON into `out` buffer. Returns byte length, negative on error.
#[no_mangle]
pub extern "C" fn picoo_export_diagnostics_json(
    trusted_store_path: *const std::ffi::c_char,
    platform: *const std::ffi::c_char,
    app_version: *const std::ffi::c_char,
    out: *mut std::ffi::c_char,
    out_len: usize,
) -> i32 {
    if trusted_store_path.is_null() || platform.is_null() || app_version.is_null() {
        return -1;
    }
    let trusted_store_path = unsafe { CStr::from_ptr(trusted_store_path) }.to_string_lossy();
    let platform = unsafe { CStr::from_ptr(platform) }.to_string_lossy();
    let app_version = unsafe { CStr::from_ptr(app_version) }.to_string_lossy();
    let json = match export_diagnostics_from_trusted_path(
        trusted_store_path.as_ref(),
        platform.as_ref(),
        app_version.as_ref(),
    ) {
        Ok(json) => json,
        Err(code) => return code,
    };
    if out.is_null() || out_len == 0 {
        return json.len() as i32;
    }
    copy_str_to_buf(&json, out, out_len)
}

/// QR JSON connect payload parse helper — returns host/port/receiver_id or negative on error.
#[no_mangle]
pub extern "C" fn picoo_qr_connect_parse(
    json: *const std::ffi::c_char,
    out_host: *mut std::ffi::c_char,
    out_host_len: usize,
    out_port: *mut u16,
    out_receiver_id: *mut std::ffi::c_char,
    out_receiver_id_len: usize,
) -> i32 {
    if json.is_null() {
        return -1;
    }
    let json = unsafe { CStr::from_ptr(json) }.to_string_lossy();
    let payload = match picoo_discovery::QrConnectPayload::decode_json(&json) {
        Ok(payload) => payload,
        Err(_) => return -2,
    };
    if !out_port.is_null() {
        unsafe {
            *out_port = payload.port;
        }
    }
    if copy_str_to_buf(&payload.host, out_host, out_host_len) < 0 {
        return -3;
    }
    if copy_str_to_buf(&payload.receiver_id, out_receiver_id, out_receiver_id_len) < 0 {
        return -3;
    }
    0
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
