//! Shared diagnostics export for CLI and GPUI — REQ-PICOO-PRIVACY-003 / PUC-007.

#![cfg_attr(not(feature = "gpui-ui"), allow(dead_code))]

use picoo_diagnostics::{build_report, export_json, DiagnosticInput, DiagnosticSessionSnapshot};
use picoo_pairing::TrustedDeviceStore;
use picoo_receiver::IngressStats;
use picoo_session::ReceiverStatus;

use crate::receiver_runtime::default_trusted_store_path;

pub struct DiagnosticsExportResult {
    pub path: Option<String>,
    pub json: String,
}

pub fn export_diagnostics_to_file(
    out_path: &str,
    status: ReceiverStatus,
    ingress: IngressStats,
) -> Result<DiagnosticsExportResult, String> {
    export_diagnostics_to_file_with_hosts(out_path, status, ingress, &[])
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
        json,
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
    use picoo_pairing::TrustedDevice;
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
        store.upsert(TrustedDevice {
            device_id: "pixel-9-pro-id".into(),
            device_name: "Pixel 9 Pro".into(),
            public_key: vec![1, 2, 3, 4],
            certificate_fingerprint: "abcdef0123456789deadbeef".into(),
            paired_at_ms: 1,
            last_connected_at_ms: Some(2),
        });
        store.save_to_path(&store_path).expect("save store");
        std::env::set_var("PICOO_TRUSTED_STORE", &store_path);

        let ingress = IngressStats {
            access_units: 10,
            packets_received: 20,
            packets_dropped_unpaired: 1,
            decode_invocations: 10,
            control_rejected_unpaired: 0,
        };
        let result = export_diagnostics_to_file_with_hosts(
            out_path.to_str().unwrap(),
            ReceiverStatus::Streaming,
            ingress,
            &["192.168.1.42".into()],
        )
        .expect("export");

        assert!(
            result.json.contains("\"includes_video\": false")
                || result.json.contains("\"includes_video\":false"),
            "includes_video must be false: {}",
            result.json
        );
        assert!(
            !result.json.contains("Pixel 9 Pro"),
            "device name must be redacted: {}",
            result.json
        );
        assert!(
            !result.json.contains("192.168.1.42"),
            "LAN IP must be redacted: {}",
            result.json
        );
        assert!(
            result.json.contains("192.168.xxx.xxx"),
            "expected redacted host form: {}",
            result.json
        );
        let on_disk = std::fs::read_to_string(&out_path).expect("read out");
        assert_eq!(on_disk, result.json);
    }
}
