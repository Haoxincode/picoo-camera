use crate::handles::{RecoverMutex, SenderInner};
use picoo_pairing::DeviceIdentity;
use picoo_sender::{EncoderDirective, SenderError, SenderSession, SessionStats};
use picoo_session::SenderStatus;
use picoo_transport::{ClientNetworkBinding, Endpoint, QuicSenderTransport, TransportError};
use std::ffi::CStr;
use std::sync::Mutex;

fn sender_status_code(status: SenderStatus) -> i32 {
    status.as_code()
}

/// Returns the unversioned protocol name for FFI smoke tests.
#[no_mangle]
pub extern "C" fn picoo_protocol_name() -> *const std::ffi::c_char {
    static NAME: &[u8] = b"PCP\0";
    NAME.as_ptr() as *const std::ffi::c_char
}

/// Create a sender session bound to a durable signing identity.
///
/// The session clones the identity, so the caller may destroy the identity
/// handle independently after this function returns.
#[no_mangle]
pub extern "C" fn picoo_sender_create(
    identity_handle: *mut std::ffi::c_void,
) -> *mut std::ffi::c_void {
    if identity_handle.is_null() {
        return std::ptr::null_mut();
    }
    let identity = unsafe { &*(identity_handle as *mut DeviceIdentity) }.clone();
    Box::into_raw(Box::new(SenderInner {
        session: Mutex::new(SenderSession::new_with_identity(
            QuicSenderTransport::new(),
            identity,
        )),
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

/// Connect QUIC session to host:port (PCP ALPN `picoocam`).
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
    let mut session = inner.session.lock_or_recover();
    match session.connect(Endpoint {
        host: host.into_owned(),
        port,
    }) {
        Ok(_) => 0,
        Err(SenderError::Transport(TransportError::NetworkBindingFailed(_))) => -3,
        Err(_) => -2,
    }
}

/// Bind future Sender QUIC sockets to an Apple Network.framework interface index.
///
/// The binding is retained by the transport and applied again to automatic reconnect sockets.
#[no_mangle]
pub extern "C" fn picoo_sender_set_apple_network_interface(
    handle: *mut std::ffi::c_void,
    interface_index: u32,
) -> i32 {
    if handle.is_null() || interface_index == 0 {
        return -1;
    }
    let inner = unsafe { &*(handle as *mut SenderInner) };
    inner
        .session
        .lock_or_recover()
        .transport_mut()
        .set_network_binding(ClientNetworkBinding::AppleInterface(interface_index));
    0
}

/// User-initiated disconnect (no auto-reconnect until the next connect). PUC-005.
#[no_mangle]
pub extern "C" fn picoo_sender_disconnect(handle: *mut std::ffi::c_void) -> i32 {
    if handle.is_null() {
        return -1;
    }
    let inner = unsafe { &*(handle as *mut SenderInner) };
    inner.session.lock_or_recover().disconnect();
    0
}

/// Drive QUIC I/O (call periodically from platform thread).
#[no_mangle]
pub extern "C" fn picoo_sender_pump(handle: *mut std::ffi::c_void) -> i32 {
    if handle.is_null() {
        return -1;
    }
    let inner = unsafe { &*(handle as *mut SenderInner) };
    match inner.session.lock_or_recover().pump() {
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
    let mut session = inner.session.lock_or_recover();

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
    let mut session = inner.session.lock_or_recover();
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

#[repr(C)]
#[derive(Debug, Clone, Copy, Default)]
pub struct PicooEncoderDirective {
    pub id: u64,
    pub kind: u32,
    pub target_height: u32,
    pub target_bitrate_bps: u32,
    pub stream_epoch: u32,
}

/// Coherent sender control-plane state captured under one Rust session lock.
///
/// Platform UIs should prefer this over combining individual getters: fields
/// in one snapshot always describe the same session instant.
#[repr(C)]
#[derive(Debug, Clone, Copy, Default)]
pub struct PicooSenderSnapshot {
    pub status: i32,
    pub current_bitrate_bps: u32,
    pub active_height: u32,
    pub receiver_max_height: u32,
    pub stream_epoch: u32,
    pub reconnect_attempt: u32,
    pub reconnect_delay_ms: u64,
}

pub(crate) fn sender_snapshot(session: &SenderSession<QuicSenderTransport>) -> PicooSenderSnapshot {
    PicooSenderSnapshot {
        status: sender_status_code(session.status()),
        current_bitrate_bps: session.current_bitrate_bps(),
        active_height: session.bitrate_active_height(),
        receiver_max_height: session.receiver_max_height(),
        stream_epoch: session.current_stream_epoch(),
        reconnect_attempt: session.reconnect_attempt(),
        reconnect_delay_ms: session.last_scheduled_reconnect_delay_ms().unwrap_or(0),
    }
}

impl From<EncoderDirective> for PicooEncoderDirective {
    fn from(value: EncoderDirective) -> Self {
        Self {
            id: value.id,
            kind: value.kind as u32,
            target_height: value.target_height,
            target_bitrate_bps: value.target_bitrate_bps,
            stream_epoch: value.stream_epoch,
        }
    }
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
    let stats: SessionStats = inner.session.lock_or_recover().stats();
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
    inner.session.lock_or_recover().pending_packets() as u64
}

/// Read coherent sender control-plane state under one session lock.
#[no_mangle]
pub extern "C" fn picoo_sender_snapshot(
    handle: *mut std::ffi::c_void,
    out: *mut PicooSenderSnapshot,
) -> i32 {
    if handle.is_null() || out.is_null() {
        return -1;
    }
    let inner = unsafe { &*(handle as *mut SenderInner) };
    let snapshot = sender_snapshot(&inner.session.lock_or_recover());
    unsafe { *out = snapshot };
    0
}

/// Mark Permission Required (REQ-PICOO-SESSION-001). Returns 0 on success.
#[no_mangle]
pub extern "C" fn picoo_sender_mark_permission_required(handle: *mut std::ffi::c_void) -> i32 {
    if handle.is_null() {
        return -1;
    }
    let inner = unsafe { &*(handle as *mut SenderInner) };
    inner.session.lock_or_recover().mark_permission_required();
    0
}

/// Clear Permission Required after the host grants access. Returns 0 on success.
#[no_mangle]
pub extern "C" fn picoo_sender_clear_permission_required(handle: *mut std::ffi::c_void) -> i32 {
    if handle.is_null() {
        return -1;
    }
    let inner = unsafe { &*(handle as *mut SenderInner) };
    inner.session.lock_or_recover().clear_permission_required();
    0
}

/// Latest ReceiverStats feedback for live UI (PUC-005 / REQ-PICOO-PROTOCOL-006).
///
/// Writes `[rtt_ms, packet_loss, jitter_ms, frame_age_ms, receive_bitrate,
/// jitter_target_ms, jitter_actual_delay_ms, jitter_occupancy_ms]` into `out`
/// (length 8). Returns 0 when stats are available, 1 when none yet, -1 on error.
#[no_mangle]
pub extern "C" fn picoo_sender_last_receiver_stats(
    handle: *mut std::ffi::c_void,
    out: *mut f64,
    out_len: usize,
) -> i32 {
    if handle.is_null() || out.is_null() || out_len < 8 {
        return -1;
    }
    let inner = unsafe { &*(handle as *mut SenderInner) };
    let session = inner.session.lock_or_recover();
    let Some(stats) = session.last_receiver_stats() else {
        return 1;
    };
    unsafe {
        *out.add(0) = stats.rtt_ms;
        *out.add(1) = stats.packet_loss;
        *out.add(2) = stats.jitter_ms;
        *out.add(3) = stats.frame_age_ms;
        *out.add(4) = f64::from(stats.receive_bitrate);
        *out.add(5) = stats.jitter_buffer_target_ms;
        *out.add(6) = stats.jitter_buffer_actual_delay_ms;
        *out.add(7) = stats.jitter_buffer_occupancy_ms;
    }
    0
}

/// Copy last SessionError code (e.g. PUBLIC_KEY_CHANGED). Returns bytes written
/// excluding NUL, 0 if none, -1 on error.
#[no_mangle]
pub extern "C" fn picoo_sender_last_session_error(
    handle: *mut std::ffi::c_void,
    out: *mut std::os::raw::c_char,
    out_len: usize,
) -> i32 {
    if handle.is_null() || out.is_null() || out_len == 0 {
        return -1;
    }
    let inner = unsafe { &*(handle as *mut SenderInner) };
    let session = inner.session.lock_or_recover();
    let Some(code) = session.last_session_error() else {
        unsafe { *out = 0 };
        return 0;
    };
    let bytes = code.as_bytes();
    let copy = bytes.len().min(out_len.saturating_sub(1));
    unsafe {
        std::ptr::copy_nonoverlapping(bytes.as_ptr(), out as *mut u8, copy);
        *out.add(copy) = 0;
    }
    copy as i32
}
