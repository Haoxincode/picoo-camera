//! Diagnostic export with privacy redaction — REQ-PICOO-PRIVACY-003, PUC-007.
//!
//! Default export never includes video frames or raw pixel buffers.

use picoo_pairing::TrustedDevice;
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
    /// Access units on the active role path (sender encode / receiver decode).
    pub access_units: u64,
    /// Packets on the active role path (sender egress or receiver ingress).
    pub packets: u64,
    /// Receiver-only unpaired drops; sender exports 0.
    pub packets_dropped_unpaired: u64,
    /// Receiver data fragments reconstructed locally from FEC parity.
    #[serde(default)]
    pub fec_recovered_fragments: u64,
    #[serde(default)]
    pub reassembly_partial_access_unit_drops: u64,
    #[serde(default)]
    pub reassembly_whole_access_unit_gap_drops: u64,
    /// Receiver decoder invocations; 0 for senders and older producers.
    #[serde(default)]
    pub decode_invocations: u64,
    /// Frames committed after decode.
    #[serde(default)]
    pub decoded_frames: u64,
    /// Delta AUs intentionally discarded while waiting for a fresh IDR.
    #[serde(default)]
    pub recovery_dropped_access_units: u64,
    /// Decoder state resets caused by epoch changes or recovery.
    #[serde(default)]
    pub decoder_resets: u64,
    #[serde(default)]
    pub recovery_reference_lost: u64,
    #[serde(default)]
    pub recovery_reference_late: u64,
    #[serde(default)]
    pub recovery_jitter_capacity: u64,
    #[serde(default)]
    pub recovery_arrived_after_playout: u64,
    #[serde(default)]
    pub recovery_jitter_expired: u64,
    #[serde(default)]
    pub recovery_decoder_errors: u64,
    /// Reliable-stream IDR requests queued by Receiver.
    #[serde(default)]
    pub keyframe_requests: u64,
    /// Optional bind / peer hosts (IPv4), redacted when policy.redact_ips.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub hosts: Vec<String>,
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
    pub protocol_name: String,
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
    /// Raw hosts to attach to session (will be redacted).
    pub hosts: Vec<String>,
}

pub fn redact_ipv4(ip: &str) -> String {
    let parts: Vec<&str> = ip.split('.').collect();
    if parts.len() == 4 && parts.iter().all(|p| p.parse::<u8>().is_ok()) {
        return format!("{}.{}.xxx.xxx", parts[0], parts[1]);
    }
    "xxx.xxx.xxx.xxx".into()
}

/// Redact a host that may be `ip`, `ip:port`, or a hostname.
pub fn redact_host(host: &str, redact_ips: bool) -> String {
    if !redact_ips {
        return host.to_string();
    }
    let (addr, port) = match host.rsplit_once(':') {
        Some((addr, port)) if port.chars().all(|c| c.is_ascii_digit()) => (addr, Some(port)),
        _ => (host, None),
    };
    // Strip IPv6 brackets if present.
    let addr = addr.trim_start_matches('[').trim_end_matches(']');
    let redacted = if addr.split('.').count() == 4 {
        redact_ipv4(addr)
    } else if addr.contains(':') {
        // IPv6 — coarse redaction.
        "xxxx:xxxx:xxxx:xxxx::".into()
    } else {
        // Hostname — keep first label only.
        let label = addr.split('.').next().unwrap_or(addr);
        format!("{}.***", redact_device_name(label).trim_end_matches('*'))
    };
    match port {
        Some(p) => format!("{redacted}:{p}"),
        None => redacted,
    }
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

    let mut session = input.session;
    if let Some(ref mut snap) = session {
        let mut hosts = snap.hosts.clone();
        hosts.extend(input.hosts.iter().cloned());
        snap.hosts = hosts
            .into_iter()
            .map(|h| redact_host(&h, input.redaction.redact_ips))
            .collect();
    } else if !input.hosts.is_empty() {
        session = Some(DiagnosticSessionSnapshot {
            role: "unknown".into(),
            status: "unknown".into(),
            access_units: 0,
            packets: 0,
            packets_dropped_unpaired: 0,
            fec_recovered_fragments: 0,
            reassembly_partial_access_unit_drops: 0,
            reassembly_whole_access_unit_gap_drops: 0,
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
            hosts: input
                .hosts
                .iter()
                .map(|h| redact_host(h, input.redaction.redact_ips))
                .collect(),
        });
    }

    DiagnosticReport {
        version: REPORT_VERSION,
        exported_at_ms: input.exported_at_ms,
        platform: input.platform,
        app_version: input.app_version,
        protocol_name: "PCP".into(),
        redaction_enabled,
        includes_video: false,
        session,
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
        assert_eq!(redact_host("10.0.0.5:4433", true), "10.0.xxx.xxx:4433");
        assert_eq!(redact_host("10.0.0.5:4433", false), "10.0.0.5:4433");
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

    #[test]
    fn session_hosts_are_redacted_in_export_json() {
        // REQ-PICOO-PRIVACY-003: raw LAN IPs must not appear in default export.
        let report = build_report(DiagnosticInput {
            platform: "linux".into(),
            app_version: "0.1.0".into(),
            exported_at_ms: 1,
            session: Some(DiagnosticSessionSnapshot {
                role: "receiver".into(),
                status: "Streaming".into(),
                access_units: 1,
                packets: 2,
                packets_dropped_unpaired: 0,
                fec_recovered_fragments: 0,
                reassembly_partial_access_unit_drops: 0,
                reassembly_whole_access_unit_gap_drops: 0,
                decode_invocations: 1,
                decoded_frames: 1,
                recovery_dropped_access_units: 0,
                decoder_resets: 0,
                recovery_reference_lost: 0,
                recovery_reference_late: 0,
                recovery_jitter_capacity: 0,
                recovery_arrived_after_playout: 0,
                recovery_jitter_expired: 0,
                recovery_decoder_errors: 0,
                keyframe_requests: 0,
                hosts: vec!["192.168.1.42:4433".into()],
            }),
            hosts: vec!["10.0.0.7".into()],
            ..Default::default()
        });
        let hosts = &report.session.as_ref().unwrap().hosts;
        assert!(hosts.iter().all(|h| h.contains("xxx")));
        let json = export_json(&report).expect("json");
        assert!(!json.contains("192.168.1.42"));
        assert!(!json.contains("10.0.0.7"));
    }
}
