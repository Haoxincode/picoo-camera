//! Minimize-to-tray close policy — REQ-PICOO-UI-008 / PRD §16.
//!
//! Win32 `Shell_NotifyIcon` wiring is platform-specific; the close-action policy
//! is unit-tested on all hosts so prefs toggles have defined behavior.

#![allow(dead_code)] // Win32 notify-icon menu wiring lands with shell integration.

/// How the main window should react to a close request.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CloseAction {
    /// Hide the window and keep the process running (tray).
    HideToTray,
    /// Quit the application.
    Quit,
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
}

/// Tray menu actions (for future Win32 notify-icon menu).
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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn close_hides_when_tray_enabled() {
        assert_eq!(
            TrayPolicy::from_pref(true).on_close_requested(),
            CloseAction::HideToTray
        );
    }

    #[test]
    fn close_quits_when_tray_disabled() {
        assert_eq!(
            TrayPolicy::from_pref(false).on_close_requested(),
            CloseAction::Quit
        );
    }

    #[test]
    fn tray_menu_labels_are_stable() {
        assert_eq!(TrayMenuAction::Show.label(), "Show Picoo Camera");
        assert_eq!(TrayMenuAction::Quit.label(), "Quit");
    }
}
