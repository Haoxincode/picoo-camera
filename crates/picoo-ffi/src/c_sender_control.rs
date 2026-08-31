use crate::c_media::copy_parameter_sets;
use crate::c_sender::PicooEncoderDirective;
use crate::handles::{copy_str_to_buf, RecoverMutex, SenderInner};
use picoo_sender::StreamConfigParams;
use std::ffi::CStr;

/// Send ClientHello after QUIC connect (PUC-001 / PUC-008).
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
        .lock_or_recover()
        .send_client_hello(&sender_id, &device_name, key)
    {
        Ok(()) => 0,
        Err(_) => -2,
    }
}

/// Send the phone-side PairingConfirm after the user verifies the six-digit code.
/// Streaming starts only after the receiver returns PairingComplete.
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
        .lock_or_recover()
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
    let session = inner.session.lock_or_recover();
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
        .lock_or_recover()
        .set_stream_config(StreamConfigParams {
            width,
            height,
            fps,
            bitrate_bps,
            stream_epoch: 0,
            mirrored: mirrored != 0,
            rotation,
            sps: sps_bytes,
            pps: pps_bytes,
        });
    0
}

/// Returns 1 if receiver requested an IDR (consumes the flag). REQ-PICOO-SESSION-003.
#[no_mangle]
pub extern "C" fn picoo_sender_take_keyframe_request(handle: *mut std::ffi::c_void) -> i32 {
    if handle.is_null() {
        return -1;
    }
    let inner = unsafe { &*(handle as *mut SenderInner) };
    if inner.session.lock_or_recover().take_keyframe_request() {
        1
    } else {
        0
    }
}

/// Read the pending ABR encoder transition without acknowledging it.
#[no_mangle]
pub extern "C" fn picoo_sender_peek_encoder_directive(
    handle: *mut std::ffi::c_void,
    out: *mut PicooEncoderDirective,
) -> i32 {
    if handle.is_null() || out.is_null() {
        return -1;
    }
    let inner = unsafe { &*(handle as *mut SenderInner) };
    let session = inner.session.lock_or_recover();
    let Some(directive) = session.pending_encoder_directive() else {
        return 0;
    };
    unsafe { *out = directive.into() };
    1
}

/// Confirm that the native encoder applied the matching directive.
#[no_mangle]
pub extern "C" fn picoo_sender_ack_encoder_directive(
    handle: *mut std::ffi::c_void,
    directive_id: u64,
    actual_height: u32,
) -> i32 {
    if handle.is_null() {
        return -1;
    }
    let inner = unsafe { &*(handle as *mut SenderInner) };
    i32::from(
        inner
            .session
            .lock_or_recover()
            .acknowledge_encoder_directive(directive_id, actual_height),
    )
}

/// Reject a directive without changing the active Rust ABR ladder.
#[no_mangle]
pub extern "C" fn picoo_sender_nack_encoder_directive(
    handle: *mut std::ffi::c_void,
    directive_id: u64,
) -> i32 {
    if handle.is_null() {
        return -1;
    }
    let inner = unsafe { &*(handle as *mut SenderInner) };
    i32::from(
        inner
            .session
            .lock_or_recover()
            .reject_encoder_directive(directive_id),
    )
}

/// Consume a desktop-originated CameraCommand (PUC-005 / REQ-PICOO-UI-009).
///
/// Returns the command enum value (`1` SWITCH_FRONT … `4` SET_MIRROR), or `0` when
/// none pending. Optional outs: `out_width`/`out_height` for SET_RESOLUTION,
/// `out_mirrored` (0/1) for SET_MIRROR.
#[no_mangle]
pub extern "C" fn picoo_sender_take_camera_command(
    handle: *mut std::ffi::c_void,
    out_width: *mut u32,
    out_height: *mut u32,
    out_mirrored: *mut i32,
) -> i32 {
    if handle.is_null() {
        return -1;
    }
    let inner = unsafe { &*(handle as *mut SenderInner) };
    let Some(cmd) = inner.session.lock_or_recover().take_camera_command() else {
        return 0;
    };
    if !out_width.is_null() || !out_height.is_null() {
        let (w, h) = cmd
            .resolution
            .as_ref()
            .map(|r| (r.width, r.height))
            .unwrap_or((0, 0));
        if !out_width.is_null() {
            unsafe {
                *out_width = w;
            }
        }
        if !out_height.is_null() {
            unsafe {
                *out_height = h;
            }
        }
    }
    if !out_mirrored.is_null() {
        unsafe {
            *out_mirrored = i32::from(cmd.mirrored);
        }
    }
    cmd.command
}

/// Set preferred capture height for ABR decisions (480, 720, or 1080).
#[no_mangle]
pub extern "C" fn picoo_sender_set_preferred_height(
    handle: *mut std::ffi::c_void,
    height: u32,
) -> i32 {
    if handle.is_null() {
        return -1;
    }
    let inner = unsafe { &*(handle as *mut SenderInner) };
    inner.session.lock_or_recover().set_preferred_height(height);
    0
}

/// Allocate the next stream epoch before a native encoder discontinuity.
#[no_mangle]
pub extern "C" fn picoo_sender_begin_stream_reconfiguration(handle: *mut std::ffi::c_void) -> u32 {
    if handle.is_null() {
        return 0;
    }
    let inner = unsafe { &*(handle as *mut SenderInner) };
    inner
        .session
        .lock_or_recover()
        .begin_stream_reconfiguration()
}

#[no_mangle]
pub extern "C" fn picoo_sender_cancel_stream_reconfiguration(
    handle: *mut std::ffi::c_void,
    stream_epoch: u32,
) -> i32 {
    if handle.is_null() {
        return -1;
    }
    let inner = unsafe { &*(handle as *mut SenderInner) };
    if inner
        .session
        .lock_or_recover()
        .cancel_stream_reconfiguration(stream_epoch)
    {
        0
    } else {
        -2
    }
}

/// Report a successfully applied native encoder height outside an ABR directive.
#[no_mangle]
pub extern "C" fn picoo_sender_report_encoder_height(
    handle: *mut std::ffi::c_void,
    height: u32,
    stream_epoch: u32,
) -> i32 {
    if handle.is_null() || height == 0 {
        return -1;
    }
    let inner = unsafe { &*(handle as *mut SenderInner) };
    if inner
        .session
        .lock_or_recover()
        .report_encoder_height(height, stream_epoch)
    {
        0
    } else {
        -2
    }
}

/// Thermal hold: block ABR upshift above 720 while overheating (MEDIA-010).
#[no_mangle]
pub extern "C" fn picoo_sender_set_thermal_hold(handle: *mut std::ffi::c_void, hold: i32) -> i32 {
    if handle.is_null() {
        return -1;
    }
    let inner = unsafe { &*(handle as *mut SenderInner) };
    inner.session.lock_or_recover().set_thermal_hold(hold != 0);
    0
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
        .lock_or_recover()
        .attach_trusted_store(path.as_ref())
    {
        Ok(()) => 0,
        Err(_) => -2,
    }
}

/// Remove a trusted receiver from the sender's attached store.
/// Returns 1 if removed, 0 if not found, negative on invalid input or storage error.
#[no_mangle]
pub extern "C" fn picoo_sender_remove_trusted_device(
    handle: *mut std::ffi::c_void,
    device_id: *const std::ffi::c_char,
) -> i32 {
    if handle.is_null() || device_id.is_null() {
        return -1;
    }
    let device_id = unsafe { CStr::from_ptr(device_id) }.to_string_lossy();
    let inner = unsafe { &*(handle as *mut SenderInner) };
    match inner
        .session
        .lock_or_recover()
        .remove_trusted_device(device_id.as_ref())
    {
        Ok(removed) => i32::from(removed),
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
    let session = inner.session.lock_or_recover();
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
    let session = inner.session.lock_or_recover();
    match session.connected_receiver_display_name() {
        Some(name) => copy_str_to_buf(name, out, out_len),
        None => 0,
    }
}
