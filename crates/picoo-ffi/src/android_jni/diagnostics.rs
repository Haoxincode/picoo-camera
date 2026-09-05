use jni::objects::{JObject, JString};
use jni::sys::{jint, jlong};
use jni::JNIEnv;
use picoo_diagnostics::DiagnosticSessionSnapshot;

use super::{java_string, optional_java_string};
use crate::c_diagnostics::{export_diagnostics_from_trusted_path, export_diagnostics_with_session};

#[no_mangle]
pub extern "system" fn Java_com_picoo_camera_jni_PicooNative_exportDiagnosticsToPath(
    mut env: JNIEnv<'_>,
    _this: JObject<'_>,
    trusted_store_path: JString<'_>,
    platform: JString<'_>,
    app_version: JString<'_>,
    out_path: JString<'_>,
) -> jint {
    let (Some(trusted_store_path), Some(platform), Some(app_version), Some(out_path)) = (
        java_string(&mut env, trusted_store_path),
        java_string(&mut env, platform),
        java_string(&mut env, app_version),
        java_string(&mut env, out_path),
    ) else {
        return -1;
    };
    match export_diagnostics_from_trusted_path(&trusted_store_path, &platform, &app_version) {
        Ok(json) => std::fs::write(out_path, json).map(|_| 0).unwrap_or(-4),
        Err(code) => code,
    }
}

#[no_mangle]
pub extern "system" fn Java_com_picoo_camera_jni_PicooNative_exportDiagnosticsToPathWithSession(
    mut env: JNIEnv<'_>,
    _this: JObject<'_>,
    trusted_store_path: JString<'_>,
    platform: JString<'_>,
    app_version: JString<'_>,
    role: JString<'_>,
    status: JString<'_>,
    access_units: jlong,
    packets: jlong,
    packets_dropped_unpaired: jlong,
    peer_host: JString<'_>,
    out_path: JString<'_>,
) -> jint {
    let (
        Some(trusted_store_path),
        Some(platform),
        Some(app_version),
        Some(role),
        Some(status),
        Some(out_path),
    ) = (
        java_string(&mut env, trusted_store_path),
        java_string(&mut env, platform),
        java_string(&mut env, app_version),
        java_string(&mut env, role),
        java_string(&mut env, status),
        java_string(&mut env, out_path),
    )
    else {
        return -1;
    };
    let hosts = optional_java_string(&mut env, peer_host)
        .map(|host| vec![host])
        .unwrap_or_default();
    let snapshot = DiagnosticSessionSnapshot {
        role,
        status,
        access_units: access_units as u64,
        packets: packets as u64,
        packets_dropped_unpaired: packets_dropped_unpaired as u64,
        fec_recovered_fragments: 0,
        reassembly_partial_access_unit_drops: 0,
        reassembly_whole_access_unit_gap_drops: 0,
        receive_queue_expired_access_units: 0,
        decode_invocations: 0,
        decoded_frames: 0,
        recovery_dropped_access_units: 0,
        decoder_resets: 0,
        recovery_reference_lost: 0,
        recovery_reference_late: 0,
        recovery_jitter_capacity: 0,
        recovery_arrived_after_playout: 0,
        recovery_jitter_expired: 0,
        recovery_decoder_errors: 0,
        keyframe_requests: 0,
        hosts: Vec::new(),
    };
    match export_diagnostics_with_session(
        &trusted_store_path,
        &platform,
        &app_version,
        Some(snapshot),
        &hosts,
    ) {
        Ok(json) => std::fs::write(out_path, json).map(|_| 0).unwrap_or(-4),
        Err(code) => code,
    }
}
