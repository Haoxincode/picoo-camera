//! Diagnostic export with privacy redaction — REQ-PICOO-PRIVACY-003, PUC-007.
//!
//! Default export never includes video frames or raw pixel buffers.

use picoo_pairing::TrustedDevice;
use picoo_protocol::ALPN;
use serde::{Deserialize, Serialize};
use thiserror::Error;

const REPORT_VERSION: u32 = 1;

#[derive(Debug, Error)]
pub enum DiagnosticError {
    #[error("json: {0}")]
    Json(#[from] serde_json::Error),
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct RedactionPolicy {
    pub redact_ips: bool,
    pub redact_device_names: bool,
    pub redact_fingerprints: bool,
}

impl Default for RedactionPolicy {
    fn default() -> Self {
        Self {
            redact_ips: true,
            redact_device_names: true,
            redact_fingerprints: true,
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct DiagnosticSessionSnapshot {
    pub role: String,
    pub status: String,
    pub ingress_access_units: u64,
    pub ingress_packets_received: u64,
    pub ingress_packets_dropped_unpaired: u64,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct DiagnosticTrustedDevice {
    pub device_id: String,
    pub device_name: String,
    pub certificate_fingerprint: String,
    pub paired_at_ms: u64,
    pub last_connected_at_ms: Option<u64>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct DiagnosticReport {
    pub version: u32,
    pub exported_at_ms: u64,
    pub platform: String,
    pub app_version: String,
    pub protocol_version: String,
    pub redaction_enabled: bool,
    pub includes_video: bool,
    pub session: Option<DiagnosticSessionSnapshot>,
    pub trusted_devices: Vec<DiagnosticTrustedDevice>,
}

#[derive(Debug, Clone, Default)]
pub struct DiagnosticInput {
    pub platform: String,
    pub app_version: String,
    pub exported_at_ms: u64,
    pub redaction: RedactionPolicy,
    pub session: Option<DiagnosticSessionSnapshot>,
    pub trusted_devices: Vec<TrustedDevice>,
}

pub fn redact_ipv4(ip: &str) -> String {
    let parts: Vec<&str> = ip.split('.').collect();
    if parts.len() == 4 && parts.iter().all(|p| p.parse::<u8>().is_ok()) {
        return format!("{}.{}.xxx.xxx", parts[0], parts[1]);
    }
    "xxx.xxx.xxx.xxx".into()
}

pub fn redact_device_name(name: &str) -> String {
    let mut chars = name.chars();
    let Some(first) = chars.next() else {
        return "***".into();
    };
    if chars.next().is_none() {
        return format!("{first}***");
    }
    format!("{first}***")
}

pub fn redact_fingerprint(fingerprint: &str) -> String {
    let prefix: String = fingerprint.chars().take(8).collect();
    if prefix.is_empty() {
        "********".into()
    } else {
        format!("{prefix}…")
    }
}

pub fn redact_device_id(device_id: &str) -> String {
    let prefix: String = device_id.chars().take(6).collect();
    if prefix.is_empty() {
        "******".into()
    } else {
        format!("{prefix}…")
    }
}

fn map_trusted_device(device: &TrustedDevice, policy: &RedactionPolicy) -> DiagnosticTrustedDevice {
    DiagnosticTrustedDevice {
        device_id: if policy.redact_device_names {
            redact_device_id(&device.device_id)
        } else {
            device.device_id.clone()
        },
        device_name: if policy.redact_device_names {
            redact_device_name(&device.device_name)
        } else {
            device.device_name.clone()
        },
        certificate_fingerprint: if policy.redact_fingerprints {
            redact_fingerprint(&device.certificate_fingerprint)
        } else {
            device.certificate_fingerprint.clone()
        },
        paired_at_ms: device.paired_at_ms,
        last_connected_at_ms: device.last_connected_at_ms,
    }
}

pub fn build_report(input: DiagnosticInput) -> DiagnosticReport {
    let redaction_enabled = input.redaction.redact_ips
        || input.redaction.redact_device_names
        || input.redaction.redact_fingerprints;

    DiagnosticReport {
        version: REPORT_VERSION,
        exported_at_ms: input.exported_at_ms,
        platform: input.platform,
        app_version: input.app_version,
        protocol_version: ALPN.into(),
        redaction_enabled,
        includes_video: false,
        session: input.session,
        trusted_devices: input
            .trusted_devices
            .iter()
            .map(|device| map_trusted_device(device, &input.redaction))
            .collect(),
    }
}

pub fn export_json(report: &DiagnosticReport) -> Result<String, DiagnosticError> {
    Ok(serde_json::to_string_pretty(report)?)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn redacts_ip_device_and_fingerprint() {
        assert_eq!(redact_ipv4("192.168.1.42"), "192.168.xxx.xxx");
        assert_eq!(redact_device_name("Pixel 9 Pro"), "P***");
        assert_eq!(redact_fingerprint("abcdef0123456789"), "abcdef01…");
    }

    #[test]
    fn export_never_includes_video_flag() {
        let report = build_report(DiagnosticInput {
            platform: "linux".into(),
            app_version: "0.1.0".into(),
            exported_at_ms: 1,
            ..Default::default()
        });
        assert!(!report.includes_video);
        let json = export_json(&report).expect("json");
        assert!(!json.contains("video_frame"));
        assert!(!json.contains("pixel"));
    }

    #[test]
    fn trusted_devices_are_redacted_by_default() {
        let report = build_report(DiagnosticInput {
            platform: "android".into(),
            app_version: "0.1.0".into(),
            exported_at_ms: 1,
            trusted_devices: vec![TrustedDevice {
                device_id: "windows-receiver".into(),
                device_name: "Picoo Camera".into(),
                public_key: vec![1, 2, 3],
                certificate_fingerprint: "deadbeefcafebabe".into(),
                paired_at_ms: 100,
                last_connected_at_ms: Some(200),
            }],
            ..Default::default()
        });
        assert_eq!(report.trusted_devices[0].device_name, "P***");
        assert_eq!(
            report.trusted_devices[0].certificate_fingerprint,
            "deadbeef…"
        );
        let json = export_json(&report).expect("json");
        assert!(!json.contains("Picoo Camera"));
        assert!(!json.contains("deadbeefcafebabe"));
    }
}
