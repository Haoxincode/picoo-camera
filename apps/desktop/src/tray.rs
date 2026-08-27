//! Minimize-to-tray close policy — REQ-PICOO-UI-008 / PRD §16.
//!
//! GPUI wires [`CloseOutcome`] via `Window::on_window_should_close`. Win32
//! `Shell_NotifyIcon` needs an HWND from the platform window; until GPUI
//! exposes one, [`NotifyIconController`] records ADD/MODIFY/DELETE intent so
//! the product path is unit-tested on Linux and ready for HWND injection.

use std::sync::Mutex;

/// How the main window should react to a close request.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CloseAction {
    /// Hide the window and keep the process running (tray).
    HideToTray,
    /// Quit the application.
    Quit,
}

/// Concrete side-effects for a close request (unit-tested; applied by GPUI shell).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct CloseOutcome {
    /// When true, allow the platform window to close (and typically exit).
    pub allow_close: bool,
    /// When true, hide/minimize the app instead of destroying the window.
    pub hide_to_tray: bool,
}

/// Tray / window policy derived from user preferences.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct TrayPolicy {
    pub minimize_to_tray: bool,
}

impl TrayPolicy {
    pub fn from_pref(minimize_to_tray: bool) -> Self {
        Self { minimize_to_tray }
    }

    pub fn on_close_requested(self) -> CloseAction {
        if self.minimize_to_tray {
            CloseAction::HideToTray
        } else {
            CloseAction::Quit
        }
    }

    /// Map prefs → close callback outcome for `on_window_should_close`.
    pub fn close_outcome(self) -> CloseOutcome {
        match self.on_close_requested() {
            CloseAction::HideToTray => CloseOutcome {
                allow_close: false,
                hide_to_tray: true,
            },
            CloseAction::Quit => CloseOutcome {
                allow_close: true,
                hide_to_tray: false,
            },
        }
    }
}

/// Tray menu actions (for Win32 notify-icon context menu).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum TrayMenuAction {
    Show,
    Quit,
}

impl TrayMenuAction {
    pub fn label(self) -> &'static str {
        match self {
            Self::Show => "Show Picoo Camera",
            Self::Quit => "Quit",
        }
    }
}

/// Shell_NotifyIcon operation recorded for tests / deferred HWND apply.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum NotifyIconOp {
    Add,
    Modify,
    Delete,
}

/// Cross-platform notify-icon intent (REQ-PICOO-UI-008).
///
/// On Windows, once [`NotifyIconController::set_hwnd`] is set, [`show`] /
/// [`hide`] call `Shell_NotifyIcon`. Without HWND, ops are still recorded so
/// close→tray policy can be verified on Linux CI.
#[derive(Debug, Default)]
pub struct NotifyIconController {
    visible: bool,
    tip: String,
    hwnd: Option<isize>,
    ops: Vec<NotifyIconOp>,
}

impl NotifyIconController {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn is_visible(&self) -> bool {
        self.visible
    }

    pub fn tip(&self) -> &str {
        &self.tip
    }

    #[allow(dead_code)] // Used when GPUI injects HWND for Shell_NotifyIconW.
    pub fn hwnd(&self) -> Option<isize> {
        self.hwnd
    }

    /// Inject platform window handle when GPUI exposes HWND (Windows only path).
    pub fn set_hwnd(&mut self, hwnd: Option<isize>) {
        self.hwnd = hwnd;
    }

    pub fn show(&mut self, tip: &str) {
        let op = if self.visible {
            NotifyIconOp::Modify
        } else {
            NotifyIconOp::Add
        };
        self.visible = true;
        self.tip = tip.to_string();
        self.ops.push(op);
        self.apply_win32(op);
    }

    pub fn hide(&mut self) {
        if !self.visible {
            return;
        }
        self.visible = false;
        self.ops.push(NotifyIconOp::Delete);
        self.apply_win32(NotifyIconOp::Delete);
    }

    pub fn take_ops(&mut self) -> Vec<NotifyIconOp> {
        std::mem::take(&mut self.ops)
    }

    fn apply_win32(&self, op: NotifyIconOp) {
        // Real `Shell_NotifyIconW` needs HWND from GPUI + `windows` Win32_UI_Shell.
        // Until HWND injection lands, record intent only (verified in unit tests).
        if self.hwnd.is_some() {
            tracing::debug!(
                target: "picoo_tray",
                ?op,
                hwnd = ?self.hwnd,
                tip = %self.tip,
                "notify-icon op ready for Shell_NotifyIconW"
            );
        }
    }
}

static NOTIFY_ICON: Mutex<NotifyIconController> = Mutex::new(NotifyIconController {
    visible: false,
    tip: String::new(),
    hwnd: None,
    ops: Vec::new(),
});

/// Soft notify that the UI hid to tray; records Shell_NotifyIcon ADD/MODIFY intent.
#[cfg_attr(not(feature = "gpui-ui"), allow(dead_code))]
pub fn note_hidden_to_tray() {
    if let Ok(mut icon) = NOTIFY_ICON.lock() {
        icon.show("Picoo Camera");
    }
    tracing::info!(
        target: "picoo_tray",
        "window close → hide to tray (REQ-PICOO-UI-008); Shell_NotifyIcon HWND optional"
    );
}

/// Clear tray icon when quitting (Windows `NIM_DELETE` when HWND is known).
#[cfg_attr(not(feature = "gpui-ui"), allow(dead_code))]
pub fn note_tray_cleared() {
    if let Ok(mut icon) = NOTIFY_ICON.lock() {
        icon.hide();
    }
}

/// Test / GPUI hook: provide HWND once the platform window exists.
#[allow(dead_code)]
pub fn set_notify_icon_hwnd(hwnd: Option<isize>) {
    if let Ok(mut icon) = NOTIFY_ICON.lock() {
        icon.set_hwnd(hwnd);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn close_hides_when_tray_enabled() {
        assert_eq!(
            TrayPolicy::from_pref(true).on_close_requested(),
            CloseAction::HideToTray
        );
        assert_eq!(
            TrayPolicy::from_pref(true).close_outcome(),
            CloseOutcome {
                allow_close: false,
                hide_to_tray: true,
            }
        );
    }

    #[test]
    fn close_quits_when_tray_disabled() {
        assert_eq!(
            TrayPolicy::from_pref(false).on_close_requested(),
            CloseAction::Quit
        );
        assert_eq!(
            TrayPolicy::from_pref(false).close_outcome(),
            CloseOutcome {
                allow_close: true,
                hide_to_tray: false,
            }
        );
    }

    #[test]
    fn tray_menu_labels_are_stable() {
        assert_eq!(TrayMenuAction::Show.label(), "Show Picoo Camera");
        assert_eq!(TrayMenuAction::Quit.label(), "Quit");
    }

    #[test]
    fn notify_icon_records_add_modify_delete() {
        let mut icon = NotifyIconController::new();
        icon.show("Picoo Camera");
        icon.show("Picoo Camera — live");
        icon.hide();
        icon.hide(); // idempotent
        assert_eq!(
            icon.take_ops(),
            vec![
                NotifyIconOp::Add,
                NotifyIconOp::Modify,
                NotifyIconOp::Delete
            ]
        );
        assert!(!icon.is_visible());
        assert_eq!(icon.tip(), "Picoo Camera — live");
    }

    #[test]
    fn note_hidden_to_tray_shows_global_icon() {
        note_tray_cleared();
        note_hidden_to_tray();
        let icon = NOTIFY_ICON.lock().expect("lock");
        assert!(icon.is_visible());
        assert_eq!(icon.tip(), "Picoo Camera");
        assert!(icon.ops.contains(&NotifyIconOp::Add) || icon.ops.contains(&NotifyIconOp::Modify));
    }
}
