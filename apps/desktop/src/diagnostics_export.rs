//! Shared diagnostics export for CLI and GPUI — REQ-PICOO-PRIVACY-003 / PUC-007.

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
    let json = build_diagnostics_json(status, ingress)?;
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
    build_diagnostics_json(status, ingress)
}

fn build_diagnostics_json(
    status: ReceiverStatus,
    ingress: IngressStats,
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
            status: format!("{status:?}"),
            ingress_access_units: ingress.access_units,
            ingress_packets_received: ingress.packets_received,
            ingress_packets_dropped_unpaired: ingress.packets_dropped_unpaired,
        }),
        trusted_devices: store.list().cloned().collect(),
        ..Default::default()
    });

    export_json(&report).map_err(|err| format!("serialize diagnostics: {err}"))
}
