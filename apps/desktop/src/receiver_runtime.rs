//! Shared Receiver runtime for CLI `--serve` and GPUI shell — ARCH-PICOO-UI-001.
//!
//! Owns QUIC listen, mDNS advertisement, and session pump. UI layers
//! observe [`ReceiverSnapshot`] and invoke commands without touching transport.

use std::net::SocketAddr;
use std::path::{Path, PathBuf};

use picoo_discovery::{
    local_advertise_host, MdnsAdvertiser, PairingState, ReceiverAdvertisement, DEFAULT_QUIC_PORT,
};
use picoo_pairing::{public_key_fingerprint_prefix, DeviceIdentity};
use picoo_protocol::control::{CameraCommand, StreamConfig};
use picoo_receiver::{IngressStats, ReceiverError, ReceiverIdentity, ReceiverSession};
use picoo_session::ReceiverStatus;
use picoo_transport::Endpoint;

use crate::live_diagnostics::{HistorySummary, LiveMetricsHistory};
use crate::prefs::DesktopPreferences;
pub use picoo_receiver::DEFAULT_SHARED_RING_NAME;

#[derive(Debug, Clone, PartialEq, Eq)]
#[cfg_attr(not(feature = "gpui-ui"), allow(dead_code))]
pub struct ActiveSenderSummary {
    pub sender_id: String,
    pub device_name: String,
}

#[derive(Debug, Clone, PartialEq, Eq)]
#[allow(dead_code)] // GPUI settings reads fields when `gpui-ui` is enabled.
pub struct TrustedDeviceSummary {
    pub device_id: String,
    pub device_name: String,
    pub certificate_fingerprint: String,
    pub last_connected_at_ms: u64,
    /// A→W V1: paired phones are Android.
    pub platform: &'static str,
}

#[derive(Debug, Clone)]
pub struct ReceiverRuntimeConfig {
    pub identity: ReceiverIdentity,
    pub trusted_store_path: PathBuf,
    pub shared_ring_name: String,
    pub bind_host: String,
}

impl Default for ReceiverRuntimeConfig {
    fn default() -> Self {
        Self {
            identity: load_receiver_identity("Picoo Camera"),
            trusted_store_path: default_trusted_store_path(),
            shared_ring_name: DEFAULT_SHARED_RING_NAME.into(),
            bind_host: "0.0.0.0".into(),
        }
    }
}

#[derive(Debug, Clone, PartialEq)]
#[allow(dead_code)] // GPUI shell reads fields when `gpui-ui` is enabled.
pub struct ReceiverSnapshot {
    pub status: ReceiverStatus,
    pub bind_addr: Option<SocketAddr>,
    /// Unicast IPv4 advertised through mDNS and shown for manual IP connection.
    pub advertise_host: String,
    /// Whether the mDNS advertiser was created successfully for this runtime.
    pub discovery_available: bool,
    pub pairing_short_code: Option<String>,
    /// Link jitter from last ReceiverStats (REQ-PICOO-UI-0001 AC-D-LIVE-02).
    pub link_jitter_ms: f64,
    /// Adaptive total playout target; distinct from queue occupancy.
    pub jitter_buffer_target_ms: f64,
    /// Mean first-fragment-to-buffer-exit delay in the latest stats window.
    pub jitter_buffer_actual_delay_ms: f64,
    /// Complete-AU queue PTS span; not a network jitter measurement.
    pub jitter_buffer_occupancy_ms: f64,
    /// Last complete one-second ReceiverStats window. `None` means no inbound
    /// sample exists yet; UI must not render that state as zero loss/latency.
    pub receiver_stats: Option<picoo_metrics::ReceiverStats>,
    /// Bounded desktop-process summary; samples remain available across a
    /// disconnect so the most recent fault can still be inspected.
    pub metrics_history: HistorySummary,
    pub ingress: IngressStats,
    pub stream_config: Option<StreamConfig>,
    pub stream_metrics: picoo_metrics::StreamMetrics,
    pub trusted_device_count: usize,
    pub trusted_devices: Vec<TrustedDeviceSummary>,
    pub display_name: String,
    #[cfg_attr(not(feature = "gpui-ui"), allow(dead_code))]
    pub active_sender: Option<ActiveSenderSummary>,
    #[cfg_attr(not(feature = "gpui-ui"), allow(dead_code))]
    pub virtual_camera: crate::model::VirtualCameraStatus,
    /// None when Shared Frame Ring attach succeeded (REQ-PICOO-FRAME-003 / PUC-004).
    pub shared_ring_error: Option<String>,
    /// Last production decoder failure; cleared after a real frame is committed.
    pub media_error: Option<String>,
}

pub struct ReceiverRuntime {
    receiver: ReceiverSession,
    #[allow(dead_code)]
    mdns: Option<MdnsAdvertiser>,
    bind_addr: Option<SocketAddr>,
    /// Unicast IPv4 advertised in mDNS (never 0.0.0.0 / 127.0.0.1).
    advertise_host: String,
    display_name: String,
    advertised_trusted_count: usize,
    #[cfg_attr(not(feature = "gpui-ui"), allow(dead_code))]
    virtual_camera: crate::model::VirtualCameraStatus,
    shared_ring_error: Option<String>,
    metrics_history: LiveMetricsHistory,
}

impl ReceiverRuntime {
    pub fn start(config: ReceiverRuntimeConfig) -> Result<Self, ReceiverError> {
        let mut receiver = ReceiverSession::new()
            .with_identity(config.identity.clone())
            .with_trusted_store(&config.trusted_store_path)?;

        let shared_ring_error = match receiver.attach_shared_ring(&config.shared_ring_name) {
            Ok(()) => None,
            Err(err) => {
                tracing::error!(
                    ring = %config.shared_ring_name,
                    "Shared Frame Ring unavailable — VCam will stay on placeholder: {err}"
                );
                Some(err.to_string())
            }
        };

        let bind = receiver.listen(Endpoint {
            host: config.bind_host,
            // Stable port matches WiX FirewallException (REQ-PICOO-VCAM-004).
            port: DEFAULT_QUIC_PORT,
        })?;

        let advertise_host = local_advertise_host().unwrap_or_else(|| {
            tracing::warn!(
                "no LAN IPv4 for mDNS/manual connection; falling back to 127.0.0.1 (phones on LAN will not connect)"
            );
            "127.0.0.1".into()
        });

        let mut mdns = match MdnsAdvertiser::new() {
            Ok(advertiser) => Some(advertiser),
            Err(err) => {
                tracing::warn!("mDNS unavailable: {err}");
                None
            }
        };

        let fingerprint_prefix = public_key_fingerprint_prefix(&config.identity.public_key);
        let trusted_count = receiver.trusted_devices().list().count();
        let advertisement = ReceiverAdvertisement::new(
            config.identity.receiver_id.clone(),
            config.identity.display_name.clone(),
            bind.port(),
            fingerprint_prefix,
        )
        .with_pairing_state(ReceiverAdvertisement::pairing_state_for_v1_receiver(
            trusted_count,
        ));

        if let Some(advertiser) = mdns.as_mut() {
            if let Err(err) = advertiser.register(&advertise_host, &advertisement) {
                tracing::warn!("mDNS register failed: {err}");
            } else {
                tracing::info!(
                    "mDNS advertising {} at {}:{} ({})",
                    advertisement.display_name,
                    advertise_host,
                    bind.port(),
                    advertisement.pairing_state.as_str()
                );
            }
        }

        Ok(Self {
            receiver,
            mdns,
            bind_addr: Some(bind),
            advertise_host,
            display_name: config.identity.display_name,
            advertised_trusted_count: trusted_count,
            virtual_camera: crate::model::VirtualCameraStatus::Unknown,
            shared_ring_error,
            metrics_history: LiveMetricsHistory::default(),
        })
    }

    #[cfg_attr(not(feature = "gpui-ui"), allow(dead_code))]
    pub fn from_prefs(prefs: &DesktopPreferences) -> Result<Self, ReceiverError> {
        let mut config = ReceiverRuntimeConfig::default();
        config.identity.display_name = prefs.display_name.clone();
        // Persist renamed display name into durable identity file.
        if let Ok(mut identity) =
            DeviceIdentity::load_or_create(default_identity_path(), &prefs.display_name)
        {
            if identity.device_name != prefs.display_name {
                identity.set_device_name(&prefs.display_name);
                let _ = identity.save_to_path(default_identity_path());
            }
            config.identity = receiver_identity_from_device(&identity);
        }
        let mut runtime = Self::start(config)?;
        runtime
            .receiver
            .set_auto_accept_paired(prefs.auto_accept_paired);
        runtime
            .receiver
            .set_placeholder_mode(prefs.placeholder_mode.to_frame_hub());
        // Refresh FrameHub/ring placeholder to match preference.
        let _ = runtime.receiver.publish_waiting_placeholder();
        Ok(runtime)
    }

    #[cfg_attr(not(feature = "gpui-ui"), allow(dead_code))]
    pub fn set_display_name(&mut self, name: String) {
        self.display_name = name.clone();
        self.receiver.set_display_name(name.clone());
        // Persist renamed display name into durable identity (REQ-PICOO-UI-002 / DISCOVERY-001).
        if let Ok(mut identity) = DeviceIdentity::load_or_create(default_identity_path(), &name) {
            if identity.device_name != name {
                identity.set_device_name(&name);
                let _ = identity.save_to_path(default_identity_path());
            }
        }
        self.refresh_mdns_advertisement();
    }

    /// Re-register mDNS with current display name / TXT (keeps bind port).
    fn refresh_mdns_advertisement(&mut self) {
        let Some(bind) = self.bind_addr else {
            return;
        };
        let Some(advertiser) = self.mdns.as_mut() else {
            return;
        };
        let identity = self.receiver.identity();
        let fingerprint_prefix = public_key_fingerprint_prefix(&identity.public_key);
        let trusted_count = self.receiver.trusted_devices().list().count();
        let pairing_state: PairingState =
            ReceiverAdvertisement::pairing_state_for_v1_receiver(trusted_count);
        let advertisement = ReceiverAdvertisement::new(
            identity.receiver_id.clone(),
            self.display_name.clone(),
            bind.port(),
            fingerprint_prefix,
        )
        .with_pairing_state(pairing_state);
        if let Err(err) = advertiser.register(&self.advertise_host, &advertisement) {
            tracing::warn!("mDNS re-advertise failed: {err}");
        } else {
            tracing::info!(
                "mDNS re-advertising {} at {}:{} ({})",
                advertisement.display_name,
                self.advertise_host,
                bind.port(),
                advertisement.pairing_state.as_str()
            );
        }
    }

    #[cfg_attr(not(feature = "gpui-ui"), allow(dead_code))]
    pub fn set_auto_accept_paired(&mut self, enabled: bool) {
        self.receiver.set_auto_accept_paired(enabled);
    }

    #[cfg_attr(not(feature = "gpui-ui"), allow(dead_code))]
    pub fn set_placeholder_mode(&mut self, mode: picoo_frame_hub::PlaceholderMode) {
        self.receiver.set_placeholder_mode(mode);
        let _ = self.receiver.publish_waiting_placeholder();
    }

    #[cfg_attr(not(feature = "gpui-ui"), allow(dead_code))]
    pub fn set_virtual_camera_status(&mut self, status: crate::model::VirtualCameraStatus) {
        self.virtual_camera = status;
        // REQ-PICOO-SESSION-001 / PUC-004: mirror VCam install into session status when idle.
        match status {
            crate::model::VirtualCameraStatus::Unknown
            | crate::model::VirtualCameraStatus::Bundled
            | crate::model::VirtualCameraStatus::AwaitingApproval
            | crate::model::VirtualCameraStatus::RestartRequired
            | crate::model::VirtualCameraStatus::Uninstalling
            | crate::model::VirtualCameraStatus::Installed
            | crate::model::VirtualCameraStatus::NotInstalled => {
                self.receiver.mark_virtual_camera_unavailable();
            }
            crate::model::VirtualCameraStatus::Active => {
                self.receiver.clear_virtual_camera_unavailable();
            }
        }
    }

    #[cfg_attr(not(feature = "gpui-ui"), allow(dead_code))]
    pub fn disconnect(&mut self) {
        self.receiver.close();
        let _ = self.receiver.publish_waiting_placeholder();
    }

    /// Desktop → phone remote camera control while streaming (PUC-005 / REQ-PICOO-UI-009).
    #[cfg_attr(not(feature = "gpui-ui"), allow(dead_code))]
    pub fn send_camera_command(&mut self, command: CameraCommand) -> Result<(), ReceiverError> {
        self.receiver.send_camera_command(command)
    }

    #[cfg_attr(not(feature = "gpui-ui"), allow(dead_code))]
    pub fn request_keyframe(&mut self) -> Result<(), ReceiverError> {
        self.receiver.request_keyframe()
    }

    pub fn pump(&mut self) -> Result<(), ReceiverError> {
        self.receiver.pump()?;
        let receiver_stats = self.receiver.last_stats().and_then(sanitize_receiver_stats);
        self.metrics_history.observe(
            receiver_stats.as_ref(),
            self.receiver.last_stats_revision(),
            self.receiver.decoded_fps(),
            std::time::Instant::now(),
        );
        if let Some(advertiser) = self.mdns.as_mut() {
            let changed = advertiser.poll();
            if changed {
                if advertiser.is_registered() {
                    tracing::info!("mDNS announcement is active");
                } else if let Some(error) = advertiser.last_error() {
                    tracing::warn!("mDNS announcement stopped: {error}");
                }
            }
        }
        let trusted_count = self.receiver.trusted_devices().list().count();
        if trusted_count != self.advertised_trusted_count {
            self.refresh_mdns_advertisement();
            self.advertised_trusted_count = trusted_count;
        }
        Ok(())
    }

    pub fn snapshot(&self) -> ReceiverSnapshot {
        let active_sender =
            self.receiver
                .active_sender_summary()
                .map(|(sender_id, device_name)| ActiveSenderSummary {
                    sender_id,
                    device_name,
                });
        let receiver_stats = self.receiver.last_stats().and_then(sanitize_receiver_stats);
        ReceiverSnapshot {
            status: self.receiver.status(),
            bind_addr: self.bind_addr,
            advertise_host: self.advertise_host.clone(),
            discovery_available: self
                .mdns
                .as_ref()
                .is_some_and(MdnsAdvertiser::is_registered),
            pairing_short_code: self.receiver.pairing_short_code().map(str::to_string),
            link_jitter_ms: receiver_stats
                .as_ref()
                .map(|stats| stats.jitter_ms)
                .unwrap_or(0.0),
            jitter_buffer_target_ms: receiver_stats
                .as_ref()
                .map(|stats| stats.jitter_buffer_target_ms)
                .unwrap_or(0.0),
            jitter_buffer_actual_delay_ms: receiver_stats
                .as_ref()
                .map(|stats| stats.jitter_buffer_actual_delay_ms)
                .unwrap_or(0.0),
            jitter_buffer_occupancy_ms: receiver_stats
                .as_ref()
                .map(|stats| stats.jitter_buffer_occupancy_ms)
                .unwrap_or(0.0),
            receiver_stats: receiver_stats.clone(),
            metrics_history: self.metrics_history.summary(),
            ingress: self.receiver.ingress_stats(),
            stream_config: self.receiver.stream_config().cloned(),
            stream_metrics: {
                let cfg = self.receiver.stream_config();
                let stats = receiver_stats.as_ref();
                picoo_metrics::StreamMetrics {
                    width: cfg.map(|c| c.width).unwrap_or(0),
                    height: cfg.map(|c| c.height).unwrap_or(0),
                    fps: self.receiver.decoded_fps(),
                    bitrate_bps: stats.map(|s| s.receive_bitrate).unwrap_or(0),
                    // `frame_age_ms` starts when decode completes; adding it to
                    // RTT double-counts local display residency and is not an
                    // end-to-end capture timestamp. Until clock synchronization
                    // exists, expose the measured path RTT without inventing E2E.
                    latency_ms: stats.map(|stats| stats.rtt_ms).unwrap_or(0.0),
                    packet_loss: stats.map(|stats| stats.packet_loss).unwrap_or(0.0),
                }
            },
            trusted_device_count: self.receiver.trusted_devices().list().count(),
            trusted_devices: self
                .receiver
                .trusted_devices()
                .list()
                .map(|d| TrustedDeviceSummary {
                    device_id: d.device_id.clone(),
                    device_name: d.device_name.clone(),
                    certificate_fingerprint: d.certificate_fingerprint.clone(),
                    last_connected_at_ms: d.last_connected_at_ms.unwrap_or(0),
                    platform: "Android",
                })
                .collect(),
            display_name: self.display_name.clone(),
            active_sender,
            virtual_camera: self.virtual_camera,
            shared_ring_error: self.shared_ring_error.clone(),
            media_error: self.receiver.last_media_error().map(str::to_string),
        }
    }

    #[allow(dead_code)]
    pub fn confirm_pairing(&mut self) -> Result<(), ReceiverError> {
        self.receiver.confirm_pairing_locally()?;
        Ok(())
    }

    #[allow(dead_code)]
    pub fn reject_pairing(&mut self) -> Result<(), ReceiverError> {
        self.receiver.reject_pairing_locally()
    }

    #[allow(dead_code)]
    pub fn remove_trusted_device(&mut self, device_id: &str) -> Result<bool, ReceiverError> {
        let removed = self.receiver.remove_trusted_device(device_id)?;
        if removed {
            self.refresh_mdns_advertisement();
        }
        Ok(removed)
    }

    #[allow(dead_code)]
    pub fn clear_trusted_devices(&mut self) -> Result<usize, ReceiverError> {
        let removed = self.receiver.clear_trusted_devices()?;
        if removed > 0 {
            self.refresh_mdns_advertisement();
        }
        Ok(removed)
    }

    pub fn receiver(&self) -> &ReceiverSession {
        &self.receiver
    }

    pub fn receiver_mut(&mut self) -> &mut ReceiverSession {
        &mut self.receiver
    }

    #[allow(dead_code)]
    pub fn trusted_store_path(&self) -> Option<&Path> {
        self.receiver.trusted_store_path()
    }
}

fn sanitize_receiver_stats(
    stats: &picoo_metrics::ReceiverStats,
) -> Option<picoo_metrics::ReceiverStats> {
    let finite = stats.rtt_ms.is_finite()
        && stats.packet_loss.is_finite()
        && stats.jitter_ms.is_finite()
        && stats.frame_age_ms.is_finite()
        && stats.jitter_buffer_target_ms.is_finite()
        && stats.jitter_buffer_actual_delay_ms.is_finite()
        && stats.jitter_buffer_occupancy_ms.is_finite()
        && stats.sender_queue_age_ms.is_finite();
    finite.then(|| picoo_metrics::ReceiverStats {
        rtt_ms: stats.rtt_ms.max(0.0),
        packet_loss: stats.packet_loss.clamp(0.0, 1.0),
        jitter_ms: stats.jitter_ms.max(0.0),
        reassembly_drop: stats.reassembly_drop,
        decoder_drop: stats.decoder_drop,
        frame_age_ms: stats.frame_age_ms.max(0.0),
        receive_bitrate: stats.receive_bitrate,
        jitter_buffer_target_ms: stats.jitter_buffer_target_ms.max(0.0),
        jitter_buffer_actual_delay_ms: stats.jitter_buffer_actual_delay_ms.max(0.0),
        jitter_buffer_occupancy_ms: stats.jitter_buffer_occupancy_ms.max(0.0),
        sender_queue_age_ms: stats.sender_queue_age_ms.max(0.0),
        sender_queue_dropped_access_units: stats.sender_queue_dropped_access_units,
        sender_quic_lost_packets: stats.sender_quic_lost_packets,
        sender_quic_sent_packets: stats.sender_quic_sent_packets,
        sender_video_buffered_bytes: stats.sender_video_buffered_bytes,
    })
}

fn default_identity_path() -> PathBuf {
    if cfg!(target_os = "windows") {
        std::env::var("APPDATA")
            .map(|appdata| {
                PathBuf::from(appdata)
                    .join("picoo-camera")
                    .join("receiver_identity.json")
            })
            .unwrap_or_else(|_| PathBuf::from("receiver_identity.json"))
    } else {
        let home = std::env::var("HOME").unwrap_or_else(|_| ".".into());
        PathBuf::from(home)
            .join(".config")
            .join("picoo-camera")
            .join("receiver_identity.json")
    }
}

fn load_receiver_identity(default_name: &str) -> ReceiverIdentity {
    match DeviceIdentity::load_or_create(default_identity_path(), default_name) {
        Ok(identity) => receiver_identity_from_device(&identity),
        Err(err) => {
            tracing::warn!("receiver identity load failed, using ephemeral: {err}");
            ReceiverIdentity::default()
        }
    }
}

fn receiver_identity_from_device(identity: &DeviceIdentity) -> ReceiverIdentity {
    ReceiverIdentity {
        receiver_id: identity.device_id.clone(),
        display_name: identity.device_name.clone(),
        public_key: identity.public_key().to_vec(),
    }
}

pub fn default_trusted_store_path() -> PathBuf {
    std::env::var("PICOO_TRUSTED_STORE")
        .map(PathBuf::from)
        .unwrap_or_else(|_| {
            if cfg!(target_os = "windows") {
                std::env::var("APPDATA")
                    .map(|appdata| {
                        PathBuf::from(appdata)
                            .join("picoo-camera")
                            .join("trusted_devices.json")
                    })
                    .unwrap_or_else(|_| PathBuf::from("trusted_devices.json"))
            } else {
                let home = std::env::var("HOME").unwrap_or_else(|_| ".".into());
                PathBuf::from(home)
                    .join(".config")
                    .join("picoo-camera")
                    .join("trusted_devices.json")
            }
        })
}

/// UTC `yyyy-MM-dd` for paired-device rows (PUC-007); em dash when unknown.
#[cfg_attr(not(feature = "gpui-ui"), allow(dead_code))]
pub fn format_last_connected_ms(ms: u64) -> String {
    if ms == 0 {
        return "—".into();
    }
    const DAY_MS: u64 = 86_400_000;
    let days = ms / DAY_MS;
    let z = days as i64 + 719_468;
    let era = if z >= 0 { z } else { z - 146_096 } / 146_097;
    let doe = (z - era * 146_097) as u64;
    let yoe = (doe - doe / 1460 + doe / 36524 - doe / 146_096) / 365;
    let y = yoe as i64 + era * 400;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
    let mp = (5 * doy + 2) / 153;
    let d = doy - (153 * mp + 2) / 5 + 1;
    let m = if mp < 10 { mp + 3 } else { mp - 9 };
    let y = if m <= 2 { y + 1 } else { y };
    format!("{y:04}-{m:02}-{d:02}")
}

#[cfg(test)]
mod tests {
    use super::{format_last_connected_ms, sanitize_receiver_stats};

    #[test]
    fn format_last_connected_utc_date_or_dash() {
        assert_eq!(format_last_connected_ms(0), "—");
        assert_eq!(format_last_connected_ms(1_577_836_800_000), "2020-01-01");
    }

    #[test]
    fn receiver_stats_reject_non_finite_windows() {
        assert!(sanitize_receiver_stats(&picoo_metrics::ReceiverStats {
            rtt_ms: f64::NAN,
            ..Default::default()
        })
        .is_none());
    }

    #[test]
    fn receiver_stats_clamp_finite_presentation_values() {
        let sanitized = sanitize_receiver_stats(&picoo_metrics::ReceiverStats {
            packet_loss: 2.0,
            jitter_ms: -3.0,
            jitter_buffer_target_ms: -1.0,
            jitter_buffer_actual_delay_ms: -2.0,
            jitter_buffer_occupancy_ms: -3.0,
            ..Default::default()
        })
        .expect("finite window remains present");
        assert_eq!(sanitized.rtt_ms, 0.0);
        assert_eq!(sanitized.packet_loss, 1.0);
        assert_eq!(sanitized.jitter_ms, 0.0);
        assert_eq!(sanitized.frame_age_ms, 0.0);
        assert_eq!(sanitized.jitter_buffer_target_ms, 0.0);
        assert_eq!(sanitized.jitter_buffer_actual_delay_ms, 0.0);
        assert_eq!(sanitized.jitter_buffer_occupancy_ms, 0.0);
    }
}
