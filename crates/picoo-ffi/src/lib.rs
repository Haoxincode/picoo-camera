//! C ABI entry points for mobile platforms — REQ-PICOO-STACK-003.

#![allow(clippy::not_unsafe_ptr_arg_deref)]

use picoo_diagnostics::{build_report, export_json, DiagnosticInput};
use picoo_discovery::MdnsBrowser;
use picoo_packet::extract_sps_pps;
use picoo_pairing::{DeviceIdentity, TrustedDeviceStore};
use picoo_sender::{SenderSession, SessionStats, StreamConfigParams};
use picoo_session::SenderStatus;
use picoo_transport::{Endpoint, QuicSenderTransport};
use std::ffi::CStr;
use std::slice;
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
    status.as_code()
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

/// User-initiated disconnect (no auto-reconnect until the next connect). PUC-005.
#[no_mangle]
pub extern "C" fn picoo_sender_disconnect(handle: *mut std::ffi::c_void) -> i32 {
    if handle.is_null() {
        return -1;
    }
    let inner = unsafe { &*(handle as *mut SenderInner) };
    inner.session.lock().expect("sender lock").disconnect();
    0
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

/// Mark Permission Required (REQ-PICOO-SESSION-001). Returns 0 on success.
#[no_mangle]
pub extern "C" fn picoo_sender_mark_permission_required(handle: *mut std::ffi::c_void) -> i32 {
    if handle.is_null() {
        return -1;
    }
    let inner = unsafe { &*(handle as *mut SenderInner) };
    inner
        .session
        .lock()
        .expect("sender lock")
        .mark_permission_required();
    0
}

/// Clear Permission Required after the host grants access. Returns 0 on success.
#[no_mangle]
pub extern "C" fn picoo_sender_clear_permission_required(handle: *mut std::ffi::c_void) -> i32 {
    if handle.is_null() {
        return -1;
    }
    let inner = unsafe { &*(handle as *mut SenderInner) };
    inner
        .session
        .lock()
        .expect("sender lock")
        .clear_permission_required();
    0
}

/// Send ClientHello after QUIC connect (PUC-001).
///
/// `qr_nonce` may be null/empty for mDNS; required to match receiver active QR for QR path.
#[no_mangle]
pub extern "C" fn picoo_sender_send_client_hello(
    handle: *mut std::ffi::c_void,
    sender_id: *const std::ffi::c_char,
    device_name: *const std::ffi::c_char,
    public_key: *const u8,
    public_key_len: usize,
    qr_nonce: *const std::ffi::c_char,
) -> i32 {
    if handle.is_null() || sender_id.is_null() || device_name.is_null() {
        return -1;
    }
    let sender_id = unsafe { CStr::from_ptr(sender_id) }.to_string_lossy();
    let device_name = unsafe { CStr::from_ptr(device_name) }.to_string_lossy();
    let qr_nonce = if qr_nonce.is_null() {
        String::new()
    } else {
        unsafe { CStr::from_ptr(qr_nonce) }.to_string_lossy().into_owned()
    };
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
        .send_client_hello_with_qr(&sender_id, &device_name, key, &qr_nonce)
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

/// Configure stream parameters before/at streaming (PUC-005 / REQ-PICOO-PROTOCOL-005).
///
/// `sps`/`pps` may be null/0 when unknown. Prefer NAL payloads without start codes;
/// Annex-B blobs are also accepted when both parameter sets are present in one buffer
/// passed via `sps` (with `pps` empty) — see `picoo_h264_extract_sps_pps`.
#[no_mangle]
pub extern "C" fn picoo_sender_set_stream_config(
    handle: *mut std::ffi::c_void,
    width: u32,
    height: u32,
    fps: u32,
    bitrate_bps: u32,
    stream_epoch: u32,
    mirrored: u8,
    rotation: u32,
    sps: *const u8,
    sps_len: usize,
    pps: *const u8,
    pps_len: usize,
) -> i32 {
    if handle.is_null() {
        return -1;
    }
    let (sps_bytes, pps_bytes) = copy_parameter_sets(sps, sps_len, pps, pps_len);
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
            rotation,
            sps: sps_bytes,
            pps: pps_bytes,
        });
    0
}

fn copy_parameter_sets(
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

/// Latest ReceiverStats feedback for live UI (PUC-005 / REQ-PICOO-PROTOCOL-006).
///
/// Writes `[rtt_ms, packet_loss, jitter_ms, frame_age_ms, receive_bitrate, jitter_depth_ms]`
/// into `out` (length 6). Returns 0 when stats are available, 1 when none yet, -1 on error.
#[no_mangle]
pub extern "C" fn picoo_sender_last_receiver_stats(
    handle: *mut std::ffi::c_void,
    out: *mut f64,
    out_len: usize,
) -> i32 {
    if handle.is_null() || out.is_null() || out_len < 6 {
        return -1;
    }
    let inner = unsafe { &*(handle as *mut SenderInner) };
    let session = inner.session.lock().expect("sender lock");
    let Some(stats) = session.last_receiver_stats() else {
        return 1;
    };
    unsafe {
        *out.add(0) = stats.rtt_ms;
        *out.add(1) = stats.packet_loss;
        *out.add(2) = stats.jitter_ms;
        *out.add(3) = stats.frame_age_ms;
        *out.add(4) = f64::from(stats.receive_bitrate);
        *out.add(5) = stats.jitter_buffer_depth_ms;
    }
    0
}

/// Returns 1 if receiver requested an IDR (consumes the flag). REQ-PICOO-SESSION-003.
#[no_mangle]
pub extern "C" fn picoo_sender_take_keyframe_request(handle: *mut std::ffi::c_void) -> i32 {
    if handle.is_null() {
        return -1;
    }
    let inner = unsafe { &*(handle as *mut SenderInner) };
    if inner
        .session
        .lock()
        .expect("sender lock")
        .take_keyframe_request()
    {
        1
    } else {
        0
    }
}

/// Returns 1 if ABR asks the host to drop resolution (consumes the flag). REQ-PICOO-MEDIA-010.
#[no_mangle]
pub extern "C" fn picoo_sender_take_resolution_downshift(handle: *mut std::ffi::c_void) -> i32 {
    if handle.is_null() {
        return -1;
    }
    let inner = unsafe { &*(handle as *mut SenderInner) };
    if inner
        .session
        .lock()
        .expect("sender lock")
        .take_resolution_downshift()
    {
        1
    } else {
        0
    }
}

/// Returns 1 if ABR asks the host to restore preferred resolution (consumes the flag).
#[no_mangle]
pub extern "C" fn picoo_sender_take_resolution_upshift(handle: *mut std::ffi::c_void) -> i32 {
    if handle.is_null() {
        return -1;
    }
    let inner = unsafe { &*(handle as *mut SenderInner) };
    if inner
        .session
        .lock()
        .expect("sender lock")
        .take_resolution_upshift()
    {
        1
    } else {
        0
    }
}

/// Set preferred capture height for ABR upshift decisions (720 or 1080).
#[no_mangle]
pub extern "C" fn picoo_sender_set_preferred_height(
    handle: *mut std::ffi::c_void,
    height: u32,
) -> i32 {
    if handle.is_null() {
        return -1;
    }
    let inner = unsafe { &*(handle as *mut SenderInner) };
    inner
        .session
        .lock()
        .expect("sender lock")
        .set_preferred_height(height);
    0
}

/// Host applied encode height — sync ABR ladder (thermal / user toggle). MEDIA-010.
#[no_mangle]
pub extern "C" fn picoo_sender_sync_encode_height(
    handle: *mut std::ffi::c_void,
    height: u32,
) -> i32 {
    if handle.is_null() {
        return -1;
    }
    let inner = unsafe { &*(handle as *mut SenderInner) };
    inner
        .session
        .lock()
        .expect("sender lock")
        .sync_encode_height(height);
    0
}

/// Thermal hold: block ABR 720→1080 while overheating (MEDIA-010).
#[no_mangle]
pub extern "C" fn picoo_sender_set_thermal_hold(handle: *mut std::ffi::c_void, hold: i32) -> i32 {
    if handle.is_null() {
        return -1;
    }
    let inner = unsafe { &*(handle as *mut SenderInner) };
    inner
        .session
        .lock()
        .expect("sender lock")
        .set_thermal_hold(hold != 0);
    0
}

/// Max height advertised by receiver Capabilities (0 if unknown). REQ-PICOO-MEDIA-002.
#[no_mangle]
pub extern "C" fn picoo_sender_receiver_max_height(handle: *mut std::ffi::c_void) -> u32 {
    if handle.is_null() {
        return 0;
    }
    let inner = unsafe { &*(handle as *mut SenderInner) };
    let session = inner.session.lock().expect("sender lock");
    session
        .receiver_capabilities()
        .map(|caps| caps.resolutions.iter().map(|r| r.height).max().unwrap_or(0))
        .unwrap_or(0)
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

/// Connected receiver display name from ServerHello.
#[no_mangle]
pub extern "C" fn picoo_sender_connected_receiver_display_name(
    handle: *mut std::ffi::c_void,
    out: *mut std::ffi::c_char,
    out_len: usize,
) -> i32 {
    if handle.is_null() {
        return -1;
    }
    let inner = unsafe { &*(handle as *mut SenderInner) };
    let session = inner.session.lock().expect("sender lock");
    match session.connected_receiver_display_name() {
        Some(name) => copy_str_to_buf(name, out, out_len),
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
    /// TXT `pairing_state` (`open` / `paired_only`); empty if unknown.
    pub pairing_state: [u8; 32],
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

/// Clear every trusted device. Returns the number removed (≥0), or negative on error.
#[no_mangle]
pub extern "C" fn picoo_trusted_store_clear(handle: *mut std::ffi::c_void) -> i32 {
    if handle.is_null() {
        return -1;
    }
    let inner = unsafe { &*(handle as *mut TrustedStoreInner) };
    let mut store = inner.store.lock().expect("store lock");
    store.clear() as i32
}

#[no_mangle]
pub extern "C" fn picoo_trusted_store_save(handle: *mut std::ffi::c_void) -> i32 {
    if handle.is_null() {
        return -1;
    }
    let inner = unsafe { &*(handle as *mut TrustedStoreInner) };
    let path = inner.path.lock().expect("path lock").clone();
    let Some(path) = path else {
        return -2;
    };
    match inner.store.lock().expect("store lock").save_to_path(&path) {
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
    let path = unsafe { CStr::from_ptr(path) }.to_string_lossy();
    let default_name = if default_name.is_null() {
        "Picoo Phone".to_string()
    } else {
        unsafe { CStr::from_ptr(default_name) }
            .to_string_lossy()
            .into_owned()
    };
    match DeviceIdentity::load_or_create(path.as_ref(), &default_name) {
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
    copy_str_to_buf(&identity.device_id, out, out_len)
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
    copy_str_to_buf(&identity.device_name, out, out_len)
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
            pairing_state: [0; 32],
        };
        write_field(&mut item.receiver_id, &entry.advertisement.receiver_id);
        write_field(&mut item.display_name, &entry.advertisement.display_name);
        write_field(&mut item.host, &entry.host);
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
    export_diagnostics_with_session(
        trusted_store_path,
        platform,
        app_version,
        None,
        &[],
    )
}

fn export_diagnostics_with_session(
    trusted_store_path: &str,
    platform: &str,
    app_version: &str,
    session: Option<picoo_diagnostics::DiagnosticSessionSnapshot>,
    hosts: &[String],
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
        session,
        trusted_devices: store.list().cloned().collect(),
        hosts: hosts.to_vec(),
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

/// Export diagnostics including sender/receiver session snapshot (PRIVACY-003 / PUC-007).
///
/// Session counters are role-neutral (`access_units` / `packets`).
/// `peer_host` may be null or empty. `packets_dropped_unpaired` is 0 on sender.
#[no_mangle]
pub extern "C" fn picoo_export_diagnostics_to_path_with_session(
    trusted_store_path: *const std::ffi::c_char,
    platform: *const std::ffi::c_char,
    app_version: *const std::ffi::c_char,
    role: *const std::ffi::c_char,
    status: *const std::ffi::c_char,
    access_units: u64,
    packets_received: u64,
    packets_dropped_unpaired: u64,
    peer_host: *const std::ffi::c_char,
    out_path: *const std::ffi::c_char,
) -> i32 {
    if trusted_store_path.is_null()
        || platform.is_null()
        || app_version.is_null()
        || role.is_null()
        || status.is_null()
        || out_path.is_null()
    {
        return -1;
    }
    let trusted_store_path = unsafe { CStr::from_ptr(trusted_store_path) }.to_string_lossy();
    let platform = unsafe { CStr::from_ptr(platform) }.to_string_lossy();
    let app_version = unsafe { CStr::from_ptr(app_version) }.to_string_lossy();
    let role = unsafe { CStr::from_ptr(role) }.to_string_lossy();
    let status = unsafe { CStr::from_ptr(status) }.to_string_lossy();
    let out_path = unsafe { CStr::from_ptr(out_path) }.to_string_lossy();
    let hosts = if peer_host.is_null() {
        Vec::new()
    } else {
        let host = unsafe { CStr::from_ptr(peer_host) }.to_string_lossy();
        if host.is_empty() {
            Vec::new()
        } else {
            vec![host.into_owned()]
        }
    };
    let session = Some(picoo_diagnostics::DiagnosticSessionSnapshot {
        role: role.into_owned(),
        status: status.into_owned(),
        access_units,
        packets: packets_received,
        packets_dropped_unpaired,
        hosts: Vec::new(),
    });
    match export_diagnostics_with_session(
        trusted_store_path.as_ref(),
        platform.as_ref(),
        app_version.as_ref(),
        session,
        &hosts,
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

/// QR JSON connect payload parse helper — returns host/port/receiver_id/nonce or negative on error.
/// Returns -4 if payload is expired (REQ-PICOO-DISCOVERY-004).
#[no_mangle]
pub extern "C" fn picoo_qr_connect_parse(
    json: *const std::ffi::c_char,
    out_host: *mut std::ffi::c_char,
    out_host_len: usize,
    out_port: *mut u16,
    out_receiver_id: *mut std::ffi::c_char,
    out_receiver_id_len: usize,
    out_expires_at_ms: *mut u64,
    out_nonce: *mut std::ffi::c_char,
    out_nonce_len: usize,
) -> i32 {
    if json.is_null() {
        return -1;
    }
    let json = unsafe { CStr::from_ptr(json) }.to_string_lossy();
    let payload = match picoo_discovery::QrConnectPayload::decode_json(&json) {
        Ok(payload) => payload,
        Err(_) => return -2,
    };
    let now_ms = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_millis() as u64)
        .unwrap_or(0);
    if payload.is_expired(now_ms) {
        return -4;
    }
    if !out_port.is_null() {
        unsafe {
            *out_port = payload.port;
        }
    }
    if !out_expires_at_ms.is_null() {
        unsafe {
            *out_expires_at_ms = payload.expires_at_ms;
        }
    }
    if copy_str_to_buf(&payload.host, out_host, out_host_len) < 0 {
        return -3;
    }
    if copy_str_to_buf(&payload.receiver_id, out_receiver_id, out_receiver_id_len) < 0 {
        return -3;
    }
    if !out_nonce.is_null() && out_nonce_len > 0 {
        if copy_str_to_buf(&payload.nonce, out_nonce, out_nonce_len) < 0 {
            return -3;
        }
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
        let mut stats = [0.0f64; 6];
        assert_eq!(
            picoo_sender_last_receiver_stats(handle, stats.as_mut_ptr(), stats.len()),
            1,
            "no ReceiverStats yet"
        );
        assert_eq!(picoo_sender_disconnect(handle), 0);
        assert_eq!(
            picoo_sender_status(handle),
            picoo_session::SenderStatus::Disconnected.as_code()
        );
        picoo_sender_destroy(handle);
    }

    #[test]
    fn qr_connect_parse_rejects_expired_payload() {
        use picoo_discovery::{QrConnectPayload, DEFAULT_QR_TTL_MS};

        let payload = QrConnectPayload::new(
            "192.168.1.10",
            4433,
            "recv-1",
            "fp",
            "nonce",
            1_000,
            DEFAULT_QR_TTL_MS,
        );
        let json = payload.encode_json().expect("encode");
        let mut host = [0u8; 64];
        let mut receiver_id = [0u8; 64];
        let mut port = 0u16;
        let mut expires = 0u64;
        assert_eq!(
            picoo_qr_connect_parse(
                std::ffi::CString::new(json).unwrap().as_ptr(),
                host.as_mut_ptr() as *mut std::ffi::c_char,
                host.len(),
                &mut port,
                receiver_id.as_mut_ptr() as *mut std::ffi::c_char,
                receiver_id.len(),
                &mut expires,
                std::ptr::null_mut(),
                0,
            ),
            -4
        );
    }

    #[test]
    fn identity_load_roundtrip_via_ffi() {
        let dir = tempfile::tempdir().expect("tempdir");
        let path = dir.path().join("id.json");
        let path_c = std::ffi::CString::new(path.to_str().unwrap()).unwrap();
        let name_c = std::ffi::CString::new("TestPhone").unwrap();
        let handle = picoo_identity_load_or_create(path_c.as_ptr(), name_c.as_ptr());
        assert!(!handle.is_null());
        let mut id_buf = [0u8; 64];
        let n = picoo_identity_device_id(
            handle,
            id_buf.as_mut_ptr() as *mut std::ffi::c_char,
            id_buf.len(),
        );
        assert!(n > 0);
        let mut key = [0u8; 32];
        assert_eq!(
            picoo_identity_public_key(handle, key.as_mut_ptr(), key.len()),
            32
        );
        picoo_identity_destroy(handle);

        let again = picoo_identity_load_or_create(path_c.as_ptr(), name_c.as_ptr());
        assert!(!again.is_null());
        let mut key2 = [0u8; 32];
        assert_eq!(
            picoo_identity_public_key(again, key2.as_mut_ptr(), key2.len()),
            32
        );
        assert_eq!(key, key2);
        picoo_identity_destroy(again);
    }

    #[test]
    fn extract_sps_pps_via_ffi() {
        let sps = [0x67u8, 0x42, 0x00, 0x0a];
        let pps = [0x68u8, 0xce, 0x3c, 0x80];
        let mut annex = Vec::new();
        annex.extend_from_slice(&[0, 0, 0, 1]);
        annex.extend_from_slice(&sps);
        annex.extend_from_slice(&[0, 0, 0, 1]);
        annex.extend_from_slice(&pps);
        let mut sps_out = [0u8; 64];
        let mut pps_out = [0u8; 64];
        let mut sps_len = sps_out.len();
        let mut pps_len = pps_out.len();
        assert_eq!(
            picoo_h264_extract_sps_pps(
                annex.as_ptr(),
                annex.len(),
                sps_out.as_mut_ptr(),
                &mut sps_len,
                pps_out.as_mut_ptr(),
                &mut pps_len,
            ),
            0
        );
        assert_eq!(&sps_out[..sps_len], &sps);
        assert_eq!(&pps_out[..pps_len], &pps);
    }

    #[test]
    fn receiver_max_height_zero_before_capabilities() {
        let handle = picoo_sender_create();
        assert!(!handle.is_null());
        assert_eq!(picoo_sender_receiver_max_height(handle), 0);
        picoo_sender_destroy(handle);
    }

    #[test]
    fn export_diagnostics_with_session_includes_redacted_host() {
        use std::ffi::CString;
        use std::fs;

        let dir = tempfile::tempdir().expect("tempdir");
        let store_path = dir.path().join("trusted.json");
        let out_path = dir.path().join("diag.json");
        fs::write(
            &store_path,
            r#"{"version":1,"devices":[]}"#,
        )
        .expect("empty store");

        let store = CString::new(store_path.to_str().unwrap()).unwrap();
        let platform = CString::new("android").unwrap();
        let version = CString::new("0.1.0").unwrap();
        let role = CString::new("sender").unwrap();
        let status = CString::new("Streaming").unwrap();
        let host = CString::new("192.168.1.42:4433").unwrap();
        let out = CString::new(out_path.to_str().unwrap()).unwrap();

        assert_eq!(
            picoo_export_diagnostics_to_path_with_session(
                store.as_ptr(),
                platform.as_ptr(),
                version.as_ptr(),
                role.as_ptr(),
                status.as_ptr(),
                12,
                34,
                0,
                host.as_ptr(),
                out.as_ptr(),
            ),
            0
        );
        let json = fs::read_to_string(&out_path).expect("read");
        // pretty-printed: `"includes_video": false`
        assert!(
            json.contains("\"includes_video\": false"),
            "PRIVACY-002 no-video flag missing: {json}"
        );
        assert!(json.contains("\"role\": \"sender\""), "{json}");
        assert!(json.contains("\"status\": \"Streaming\""), "{json}");
        assert!(json.contains("xxx"), "peer host must be redacted: {json}");
        assert!(!json.contains("192.168.1.42"), "{json}");
        assert_eq!(json.matches("\"access_units\": 12").count(), 1, "{json}");
        assert_eq!(json.matches("\"packets\": 34").count(), 1, "{json}");
        assert!(
            !json.contains("ingress_"),
            "session counters must be role-neutral: {json}"
        );
    }
}
