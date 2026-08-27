//! Minimize-to-tray close policy — REQ-PICOO-UI-008 / PRD §16.
//!
//! GPUI wires [`CloseOutcome`] via `Window::on_window_should_close`. Win32
//! `Shell_NotifyIcon` still needs an HWND from the platform window; hide/minimize
//! keeps the process alive until that lands.

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

/// Tray menu actions (for future Win32 notify-icon menu).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[allow(dead_code)] // Shell_NotifyIcon menu wiring pending HWND from GPUI.
pub enum TrayMenuAction {
    Show,
    Quit,
}

impl TrayMenuAction {
    #[allow(dead_code)]
    pub fn label(self) -> &'static str {
        match self {
            Self::Show => "Show Picoo Camera",
            Self::Quit => "Quit",
        }
    }
}

/// Soft notify that the UI hid to tray (Shell_NotifyIcon HWND wiring pending).
#[cfg_attr(not(feature = "gpui-ui"), allow(dead_code))]
pub fn note_hidden_to_tray() {
    tracing::info!(
        target: "picoo_tray",
        "window close → hide to tray (REQ-PICOO-UI-008); Shell_NotifyIcon pending HWND"
    );
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
}
