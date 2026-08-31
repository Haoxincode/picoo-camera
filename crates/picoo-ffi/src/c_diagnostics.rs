use crate::handles::copy_str_to_buf;
use picoo_diagnostics::{build_report, export_json, DiagnosticInput};
use picoo_pairing::TrustedDeviceStore;
use std::ffi::CStr;

pub(crate) fn export_diagnostics_from_trusted_path(
    trusted_store_path: &str,
    platform: &str,
    app_version: &str,
) -> Result<String, i32> {
    export_diagnostics_with_session(trusted_store_path, platform, app_version, None, &[])
}

pub(crate) fn export_diagnostics_with_session(
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
