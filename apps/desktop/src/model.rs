//! Desktop UI state — ARCH-PICOO-UI-001.

use picoo_metrics::StreamMetrics;
use picoo_session::ReceiverStatus;
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct DeviceSummary {
    pub device_id: String,
    pub display_name: String,
    pub platform: String,
    pub paired: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SessionSummary {
    pub sender_name: String,
    pub width: u32,
    pub height: u32,
    pub fps: u32,
}

#[derive(Debug, Clone, Copy, Default, Serialize, Deserialize)]
pub enum VirtualCameraStatus {
    #[default]
    Unknown,
    Installed,
    NotInstalled,
    Active,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AppError {
    pub code: String,
    pub message: String,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct DesktopAppState {
    pub receiver_status: ReceiverStatus,
    pub discovered_devices: Vec<DeviceSummary>,
    pub active_session: Option<SessionSummary>,
    pub virtual_camera: VirtualCameraStatus,
    pub metrics: StreamMetrics,
    pub last_error: Option<AppError>,
}
