//! Shared Receiver runtime for CLI `--serve` and GPUI shell — ARCH-PICOO-UI-001.
//!
//! Owns QUIC listen, mDNS advertisement, QR payload, and session pump. UI layers
//! observe [`ReceiverSnapshot`] and invoke commands without touching transport.

use std::net::SocketAddr;
use std::path::{Path, PathBuf};

use picoo_discovery::{
    generate_nonce, MdnsAdvertiser, QrConnectPayload, ReceiverAdvertisement, DEFAULT_QR_TTL_MS,
};
use picoo_pairing::{
    public_key_fingerprint, public_key_fingerprint_prefix, DeviceIdentity,
};
use picoo_protocol::control::StreamConfig;
use picoo_receiver::{IngressStats, ReceiverError, ReceiverIdentity, ReceiverSession};
use picoo_session::ReceiverStatus;
use picoo_transport::Endpoint;

use crate::prefs::DesktopPreferences;
use crate::qr_display;

pub use picoo_receiver::DEFAULT_SHARED_RING_NAME;

#[derive(Debug, Clone)]
#[cfg_attr(not(feature = "gpui-ui"), allow(dead_code))]
pub struct ActiveSenderSummary {
    pub sender_id: String,
    pub device_name: String,
}

#[derive(Debug, Clone)]
#[allow(dead_code)] // GPUI settings reads fields when `gpui-ui` is enabled.
pub struct TrustedDeviceSummary {
    pub device_id: String,
    pub device_name: String,
    pub certificate_fingerprint: String,
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

#[derive(Debug, Clone)]
#[allow(dead_code)] // GPUI shell reads fields when `gpui-ui` is enabled.
pub struct ReceiverSnapshot {
    pub status: ReceiverStatus,
    pub bind_addr: Option<SocketAddr>,
    pub pairing_short_code: Option<String>,
    pub qr_json: Option<String>,
    pub qr_ascii: Option<String>,
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
}

pub struct ReceiverRuntime {
    receiver: ReceiverSession,
    #[allow(dead_code)]
    mdns: Option<MdnsAdvertiser>,
    bind_addr: Option<SocketAddr>,
    qr_json: Option<String>,
    qr_ascii: Option<String>,
    /// Wall-clock expiry of the currently advertised QR payload.
    qr_expires_at_ms: u64,
    display_name: String,
    #[cfg_attr(not(feature = "gpui-ui"), allow(dead_code))]
    virtual_camera: crate::model::VirtualCameraStatus,
}

impl ReceiverRuntime {
    pub fn start(config: ReceiverRuntimeConfig) -> Result<Self, ReceiverError> {
        let mut receiver = ReceiverSession::new()
            .with_identity(config.identity.clone())
            .with_trusted_store(&config.trusted_store_path)?;

        if let Err(err) = receiver.attach_shared_ring(&config.shared_ring_name) {
            tracing::warn!("Shared Frame Ring unavailable: {err}");
        }

        let bind = receiver.listen(Endpoint {
            host: config.bind_host,
            port: 0,
        })?;

        let mut mdns = match MdnsAdvertiser::new() {
            Ok(advertiser) => Some(advertiser),
            Err(err) => {
                tracing::warn!("mDNS unavailable: {err}");
                None
            }
        };

        let fingerprint_prefix = public_key_fingerprint_prefix(&config.identity.public_key);
        let advertisement = ReceiverAdvertisement::new(
            config.identity.receiver_id.clone(),
            config.identity.display_name.clone(),
            bind.port(),
            fingerprint_prefix,
        );

        if let Some(advertiser) = mdns.as_mut() {
            if let Err(err) = advertiser.register("127.0.0.1", &advertisement) {
                tracing::warn!("mDNS register failed: {err}");
            } else {
                tracing::info!(
                    "mDNS advertising {} on port {}",
                    advertisement.display_name,
                    bind.port()
                );
            }
        }

        let (qr_json, qr_ascii, qr_expires_at_ms) =
            build_qr_payload(&config.identity, bind, &mut receiver);

        Ok(Self {
            receiver,
            mdns,
            bind_addr: Some(bind),
            qr_json,
            qr_ascii,
            qr_expires_at_ms,
            display_name: config.identity.display_name,
            virtual_camera: crate::model::VirtualCameraStatus::Unknown,
        })
    }

    #[cfg_attr(not(feature = "gpui-ui"), allow(dead_code))]
    pub fn from_prefs(prefs: &DesktopPreferences) -> Result<Self, ReceiverError> {
        let mut config = ReceiverRuntimeConfig::default();
        config.identity.display_name = prefs.display_name.clone();
        // Persist renamed display name into durable identity file.
        if let Ok(mut identity) = DeviceIdentity::load_or_create(default_identity_path(), &prefs.display_name)
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
            .set_use_default_placeholder(prefs.use_default_placeholder);
        // Refresh FrameHub/ring placeholder to match preference.
        let _ = runtime.receiver.publish_waiting_placeholder();
        Ok(runtime)
    }

    #[cfg_attr(not(feature = "gpui-ui"), allow(dead_code))]
    pub fn set_display_name(&mut self, name: String) {
        self.display_name = name.clone();
        self.receiver.set_display_name(name.clone());
        // Persist renamed display name into durable identity (REQ-PICOO-UI-002 / DISCOVERY-001).
        if let Ok(mut identity) =
            DeviceIdentity::load_or_create(default_identity_path(), &name)
        {
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
        let advertisement = ReceiverAdvertisement::new(
            identity.receiver_id.clone(),
            self.display_name.clone(),
            bind.port(),
            fingerprint_prefix,
        );
        if let Err(err) = advertiser.register("127.0.0.1", &advertisement) {
            tracing::warn!("mDNS re-advertise failed after rename: {err}");
        } else {
            tracing::info!(
                "mDNS re-advertising {} on port {}",
                advertisement.display_name,
                bind.port()
            );
        }
    }

    #[cfg_attr(not(feature = "gpui-ui"), allow(dead_code))]
    pub fn set_auto_accept_paired(&mut self, enabled: bool) {
        self.receiver.set_auto_accept_paired(enabled);
    }

    #[cfg_attr(not(feature = "gpui-ui"), allow(dead_code))]
    pub fn set_use_default_placeholder(&mut self, enabled: bool) {
        self.receiver.set_use_default_placeholder(enabled);
        let _ = self.receiver.publish_waiting_placeholder();
    }

    #[cfg_attr(not(feature = "gpui-ui"), allow(dead_code))]
    pub fn set_virtual_camera_status(&mut self, status: crate::model::VirtualCameraStatus) {
        self.virtual_camera = status;
        // REQ-PICOO-SESSION-001 / PUC-004: mirror VCam install into session status when idle.
        match status {
            crate::model::VirtualCameraStatus::NotInstalled => {
                self.receiver.mark_virtual_camera_unavailable();
            }
            crate::model::VirtualCameraStatus::Installed
            | crate::model::VirtualCameraStatus::Active
            | crate::model::VirtualCameraStatus::Unknown => {
                self.receiver.clear_virtual_camera_unavailable();
            }
        }
    }

    #[cfg_attr(not(feature = "gpui-ui"), allow(dead_code))]
    pub fn disconnect(&mut self) {
        self.receiver.close();
        let _ = self.receiver.publish_waiting_placeholder();
    }

    pub fn pump(&mut self) -> Result<(), ReceiverError> {
        self.refresh_qr_if_expired();
        self.receiver.pump()
    }

    /// Rotate QR payload when the short-lived nonce expires (REQ-PICOO-DISCOVERY-004).
    fn refresh_qr_if_expired(&mut self) {
        let Some(bind) = self.bind_addr else {
            return;
        };
        let now_ms = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map(|d| d.as_millis() as u64)
            .unwrap_or(0);
        if self.qr_json.is_some() && now_ms < self.qr_expires_at_ms {
            return;
        }
        let identity = ReceiverIdentity {
            receiver_id: self.receiver.identity().receiver_id.clone(),
            display_name: self.display_name.clone(),
            public_key: self.receiver.identity().public_key.clone(),
        };
        let (qr_json, qr_ascii, expires) =
            build_qr_payload(&identity, bind, &mut self.receiver);
        self.qr_json = qr_json;
        self.qr_ascii = qr_ascii;
        self.qr_expires_at_ms = expires;
    }

    pub fn snapshot(&self) -> ReceiverSnapshot {
        let active_sender =
            self.receiver
                .active_sender_summary()
                .map(|(sender_id, device_name)| ActiveSenderSummary {
                    sender_id,
                    device_name,
                });
        ReceiverSnapshot {
            status: self.receiver.status(),
            bind_addr: self.bind_addr,
            pairing_short_code: self.receiver.pairing_short_code().map(str::to_string),
            qr_json: self.qr_json.clone(),
            qr_ascii: self.qr_ascii.clone(),
            ingress: self.receiver.ingress_stats(),
            stream_config: self.receiver.stream_config().cloned(),
            stream_metrics: {
                let cfg = self.receiver.stream_config();
                let stats = self.receiver.last_stats();
                picoo_metrics::StreamMetrics {
                    width: cfg.map(|c| c.width).unwrap_or(0),
                    height: cfg.map(|c| c.height).unwrap_or(0),
                    fps: 30,
                    bitrate_bps: stats.map(|s| s.receive_bitrate).unwrap_or(0),
                    latency_ms: stats.map(|s| s.rtt_ms + s.frame_age_ms).unwrap_or(0.0),
                    packet_loss: stats.map(|s| s.packet_loss).unwrap_or(0.0),
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
                })
                .collect(),
            display_name: self.display_name.clone(),
            active_sender,
            virtual_camera: self.virtual_camera,
        }
    }

    #[allow(dead_code)]
    pub fn confirm_pairing(&mut self) {
        self.receiver.confirm_pairing_locally();
    }

    #[allow(dead_code)]
    pub fn remove_trusted_device(&mut self, device_id: &str) -> Result<bool, ReceiverError> {
        self.receiver.remove_trusted_device(device_id)
    }

    #[cfg_attr(not(feature = "gpui-ui"), allow(dead_code))]
    pub fn clear_trusted_devices(&mut self) -> Result<usize, ReceiverError> {
        self.receiver.clear_trusted_devices()
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

fn build_qr_payload(
    identity: &ReceiverIdentity,
    bind: SocketAddr,
    receiver: &mut ReceiverSession,
) -> (Option<String>, Option<String>, u64) {
    let now_ms = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_millis() as u64)
        .unwrap_or(0);
    let fingerprint = public_key_fingerprint(&identity.public_key);
    let nonce = generate_nonce();
    let qr = QrConnectPayload::new(
        bind.ip().to_string(),
        bind.port(),
        identity.receiver_id.clone(),
        fingerprint,
        nonce.clone(),
        now_ms,
        DEFAULT_QR_TTL_MS,
    );
    receiver.set_active_qr_nonce(nonce, qr.expires_at_ms);
    match qr.encode_json() {
        Ok(json) => {
            let ascii = qr_display::render_qr_ascii(&json).ok();
            (Some(json), ascii, qr.expires_at_ms)
        }
        Err(err) => {
            tracing::warn!("QR encode failed: {err}");
            (None, None, 0)
        }
    }
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
