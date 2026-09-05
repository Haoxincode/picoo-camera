//! Shared diagnostics export for CLI and GPUI — REQ-PICOO-PRIVACY-003 / PUC-007.

#![cfg_attr(not(feature = "gpui-ui"), allow(dead_code))]

use picoo_diagnostics::{build_report, export_json, DiagnosticInput, DiagnosticSessionSnapshot};
use picoo_pairing::TrustedDeviceStore;
use picoo_receiver::IngressStats;
use picoo_session::ReceiverStatus;

use crate::receiver_runtime::default_trusted_store_path;

pub struct DiagnosticsExportResult {
    pub path: Option<String>,
}

pub fn export_diagnostics_to_file_with_hosts(
    out_path: &str,
    status: ReceiverStatus,
    ingress: IngressStats,
    hosts: &[String],
) -> Result<DiagnosticsExportResult, String> {
    let json = build_diagnostics_json(status, ingress, hosts)?;
    std::fs::write(out_path, &json).map_err(|err| format!("write {out_path}: {err}"))?;
    Ok(DiagnosticsExportResult {
        path: Some(out_path.to_string()),
    })
}

pub fn export_diagnostics_json(
    status: ReceiverStatus,
    ingress: IngressStats,
) -> Result<String, String> {
    build_diagnostics_json(status, ingress, &[])
}

fn build_diagnostics_json(
    status: ReceiverStatus,
    ingress: IngressStats,
    hosts: &[String],
) -> Result<String, String> {
    let trusted_path = default_trusted_store_path();
    let store = TrustedDeviceStore::load_from_path(&trusted_path)
        .map_err(|err| format!("load trusted store {}: {err}", trusted_path.display()))?;

    let now_ms = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_millis() as u64)
        .unwrap_or(0);

    let report = build_report(DiagnosticInput {
        platform: std::env::consts::OS.into(),
        app_version: env!("CARGO_PKG_VERSION").into(),
        exported_at_ms: now_ms,
        session: Some(DiagnosticSessionSnapshot {
            role: "receiver".into(),
            status: status.as_label().into(),
            access_units: ingress.access_units,
            packets: ingress.packets_received,
            packets_dropped_unpaired: ingress.packets_dropped_unpaired,
            fec_recovered_fragments: ingress.fec_recovered_fragments,
            reassembly_partial_access_unit_drops: ingress.reassembly_partial_access_unit_drops,
            reassembly_whole_access_unit_gap_drops: ingress.reassembly_whole_access_unit_gap_drops,
            receive_queue_expired_access_units: ingress.receive_queue_expired_access_units,
            decode_invocations: ingress.decode_invocations,
            decoded_frames: ingress.decoded_frames,
            orientation_transform_frames: ingress.orientation_transform_frames,
            orientation_transform_total_us: ingress.orientation_transform_total_us,
            orientation_transform_max_us: ingress.orientation_transform_max_us,
            recovery_dropped_access_units: ingress.recovery_dropped_access_units,
            decoder_capacity_dropped_access_units: ingress.decoder_capacity_dropped_access_units,
            decoder_resets: ingress.decoder_resets,
            recovery_reference_lost: ingress.recovery_reference_lost,
            recovery_reference_late: ingress.recovery_reference_late,
            recovery_jitter_capacity: ingress.recovery_jitter_capacity,
            recovery_arrived_after_playout: ingress.recovery_arrived_after_playout,
            recovery_jitter_expired: ingress.recovery_jitter_expired,
            recovery_decoder_errors: ingress.recovery_decoder_errors,
            keyframe_requests: ingress.keyframe_requests,
            hosts: Vec::new(),
        }),
        trusted_devices: store.list().cloned().collect(),
        hosts: hosts.to_vec(),
        ..Default::default()
    });

    export_json(&report).map_err(|err| format!("serialize diagnostics: {err}"))
}

#[cfg(test)]
mod tests {
    use super::*;
    use picoo_pairing::{public_key_fingerprint, DeviceIdentity, TrustedDevice};
    use std::path::PathBuf;

    fn temp_paths(tag: &str) -> (PathBuf, PathBuf) {
        let nanos = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_nanos();
        let dir = std::env::temp_dir().join(format!("picoo-diag-{tag}-{nanos}"));
        let _ = std::fs::create_dir_all(&dir);
        (dir.join("trusted.json"), dir.join("out.json"))
    }

    #[test]
    fn export_redacts_hosts_and_device_names_never_includes_video() {
        let (store_path, out_path) = temp_paths("export");
        let mut store = TrustedDeviceStore::new();
        let identity = DeviceIdentity::from_secret_bytes("Pixel 9 Pro", &[7; 32])
            .expect("valid deterministic Ed25519 fixture");
        store.upsert(TrustedDevice {
            device_id: identity.device_id().into(),
            device_name: "Pixel 9 Pro".into(),
            public_key: identity.public_key().to_vec(),
            certificate_fingerprint: public_key_fingerprint(identity.public_key()),
            paired_at_ms: 1,
            last_connected_at_ms: Some(2),
        });
        store.save_to_path(&store_path).expect("save store");
        std::env::set_var("PICOO_TRUSTED_STORE", &store_path);

        let ingress = IngressStats {
            access_units: 10,
            packets_received: 20,
            packets_dropped_unpaired: 1,
            fec_recovered_fragments: 4,
            decode_invocations: 10,
            decoded_frames: 9,
            recovery_dropped_access_units: 2,
            decoder_resets: 3,
            recovery_reference_lost: 0,
            recovery_reference_late: 0,
            recovery_decoder_errors: 0,
            keyframe_requests: 1,
            control_rejected_unpaired: 0,
            ..IngressStats::default()
        };
        let result = export_diagnostics_to_file_with_hosts(
            out_path.to_str().unwrap(),
            ReceiverStatus::Streaming,
            ingress,
            &["192.168.1.42".into()],
        )
        .expect("export");

        let on_disk = std::fs::read_to_string(&out_path).expect("read out");
        assert_eq!(result.path.as_deref(), out_path.to_str());
        assert!(
            on_disk.contains("\"includes_video\": false")
                || on_disk.contains("\"includes_video\":false"),
            "includes_video must be false: {on_disk}"
        );
        assert!(
            !on_disk.contains("Pixel 9 Pro"),
            "device name must be redacted: {on_disk}"
        );
        assert!(
            !on_disk.contains("192.168.1.42"),
            "LAN IP must be redacted: {on_disk}"
        );
        assert!(
            on_disk.contains("192.168.xxx.xxx"),
            "expected redacted host form: {on_disk}"
        );
        assert!(
            on_disk.contains("\"recovery_dropped_access_units\": 2")
                && on_disk.contains("\"decoder_resets\": 3")
                && on_disk.contains("\"keyframe_requests\": 1"),
            "expected decoder recovery counters: {on_disk}"
        );
    }
}
