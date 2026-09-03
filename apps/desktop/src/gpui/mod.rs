//! GPUI desktop shell — ARCH-PICOO-UI-001.
//!
//! Desktop navigation and receiver states driven by [`ReceiverRuntime`] snapshots.

mod bootstrap;
mod connect;
mod device;
mod diagnostics;
mod icons;
mod lifecycle;
mod nav;
mod pages;
mod pairing;
mod vcam;
mod widgets;

pub use bootstrap::run_gpui_app;

use gpui::*;
use gpui_component::input::InputState;

use crate::model::VirtualCameraStatus;
use crate::prefs::DesktopPreferences;
use crate::preview_pipeline::PreviewPipeline;
use crate::receiver_runtime::{ReceiverRuntime, ReceiverSnapshot};
use crate::video_surface::VideoSurface;

use pages::DiagnosticsExportState;
use vcam::VcamSetupState;

#[derive(Clone, Copy, PartialEq, Eq)]
enum DesktopPage {
    FirstLaunch,
    Waiting,
    Live,
}

#[derive(Clone, Copy, PartialEq, Eq)]
enum DesktopSection {
    Connect,
    VirtualCamera,
    Network,
    General,
    Help,
    About,
}

#[derive(Default)]
struct PairingDialogLifecycle {
    visible: bool,
    close_requested: bool,
}

impl PairingDialogLifecycle {
    fn is_visible(&self) -> bool {
        self.visible
    }

    fn mark_opened(&mut self) {
        self.visible = true;
        self.close_requested = false;
    }

    fn request_close(&mut self) -> bool {
        if !self.visible || self.close_requested {
            return false;
        }
        self.close_requested = true;
        true
    }

    fn mark_closed(&mut self) {
        self.visible = false;
        self.close_requested = false;
    }
}

impl DesktopSection {
    fn label(self) -> &'static str {
        match self {
            Self::Connect => "连接",
            Self::VirtualCamera => "虚拟摄像头",
            Self::Network => "网络",
            Self::General => "通用",
            Self::Help => "帮助",
            Self::About => "关于",
        }
    }
}

struct PicooDesktopApp {
    runtime: ReceiverRuntime,
    prefs: DesktopPreferences,
    tray_policy: crate::tray::TrayPolicy,
    page: DesktopPage,
    section: DesktopSection,
    sidebar_collapsed: bool,
    pump_started: bool,
    last_presented_snapshot: ReceiverSnapshot,
    preview_pipeline: PreviewPipeline,
    video_surface: VideoSurface,
    display_name_input: Entity<InputState>,
    _subscriptions: Vec<Subscription>,
    vcam_status: VirtualCameraStatus,
    vcam_setup_state: VcamSetupState,
    diagnostics_message: Option<String>,
    diagnostics_error: Option<String>,
    diagnostics_export: DiagnosticsExportState,
    window_handle: AnyWindowHandle,
    /// Pairing code whose dialog has been successfully opened.
    pairing_dialog_code: Option<String>,
    /// Pairing code currently scheduled for dialog presentation.
    pairing_dialog_pending: Option<String>,
    pairing_dialog: PairingDialogLifecycle,
    pairing_locally_confirmed: bool,
    /// Current post-pairing same-name replacement prompt. Domain identity,
    /// never a list index (REQ-PICOO-PAIRING-006).
    identity_replacement_dialog_revision: Option<u64>,
}

#[cfg(test)]
mod tests {
    use super::PairingDialogLifecycle;

    #[test]
    fn pairing_dialog_stays_visible_until_close_completes() {
        let mut lifecycle = PairingDialogLifecycle::default();
        lifecycle.mark_opened();

        assert!(lifecycle.request_close());
        assert!(lifecycle.is_visible());
        assert!(!lifecycle.request_close());

        lifecycle.mark_closed();
        assert!(!lifecycle.is_visible());
        assert!(!lifecycle.request_close());
    }
}
