//! Minimize-to-tray close policy — REQ-PICOO-UI-008 / PRD §16.
//!
//! GPUI wires [`CloseOutcome`] via `Window::on_window_should_close`. On Windows,
//! a message-only tray host HWND backs `Shell_NotifyIconW` + Show/Quit menu
//! (with FindWindowW("Picoo Camera") as fallback). Linux CI still records
//! ADD/MODIFY/DELETE intent without calling Shell APIs.

#![cfg_attr(not(feature = "gpui-ui"), allow(dead_code))]

use std::sync::Mutex;

use picoo_session::ReceiverStatus;

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

/// Side-effects for a tray menu selection (applied by GPUI / Win32 pump).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct TrayMenuOutcome {
    pub restore_window: bool,
    pub quit: bool,
}

impl TrayMenuAction {
    pub fn label(self) -> &'static str {
        match self {
            Self::Show => "Show Picoo Camera",
            Self::Quit => "Quit",
        }
    }

    pub fn apply(self) -> TrayMenuOutcome {
        match self {
            Self::Show => TrayMenuOutcome {
                restore_window: true,
                quit: false,
            },
            Self::Quit => TrayMenuOutcome {
                restore_window: false,
                quit: true,
            },
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

    /// Update tip while icon remains visible (status pump → live tip).
    pub fn set_tip(&mut self, tip: &str) {
        if self.tip == tip {
            return;
        }
        self.tip = tip.to_string();
        if self.visible {
            self.ops.push(NotifyIconOp::Modify);
            self.apply_win32(NotifyIconOp::Modify);
        }
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
        #[cfg(all(windows, feature = "windows-vcam"))]
        {
            self.apply_shell_notify_icon(op);
        }
        #[cfg(not(all(windows, feature = "windows-vcam")))]
        {
            if self.hwnd.is_some() {
                tracing::debug!(
                    target: "picoo_tray",
                    ?op,
                    hwnd = ?self.hwnd,
                    tip = %self.tip,
                    "notify-icon op recorded (Shell_NotifyIconW unavailable on this build)"
                );
            }
        }
    }

    /// Resolve HWND: message-only tray host (preferred), explicit injection, else FindWindowW.
    #[cfg(all(windows, feature = "windows-vcam"))]
    fn resolve_hwnd(&self) -> Option<windows::Win32::Foundation::HWND> {
        use windows::core::w;
        use windows::Win32::Foundation::HWND;
        use windows::Win32::UI::WindowsAndMessaging::FindWindowW;

        if let Some(host) = tray_message_hwnd() {
            return Some(host);
        }
        if let Some(raw) = self.hwnd {
            return Some(HWND(raw as *mut _));
        }
        unsafe { FindWindowW(None, w!("Picoo Camera")).ok() }.filter(|h| !h.is_invalid())
    }

    #[cfg(all(windows, feature = "windows-vcam"))]
    fn apply_shell_notify_icon(&self, op: NotifyIconOp) {
        use std::mem::size_of;
        use windows::Win32::UI::Shell::{
            Shell_NotifyIconW, NIF_MESSAGE, NIF_TIP, NIM_ADD, NIM_DELETE, NIM_MODIFY,
            NOTIFYICONDATAW,
        };

        let Some(hwnd) = self.resolve_hwnd() else {
            tracing::debug!(
                target: "picoo_tray",
                ?op,
                "Shell_NotifyIconW deferred — no HWND (tray host / set_notify_icon_hwnd / FindWindowW)"
            );
            return;
        };

        let mut tip_buf = [0u16; 128];
        for (i, c) in self.tip.encode_utf16().take(127).enumerate() {
            tip_buf[i] = c;
        }
        let mut data = NOTIFYICONDATAW {
            cbSize: size_of::<NOTIFYICONDATAW>() as u32,
            hWnd: hwnd,
            uID: TRAY_ICON_ID,
            uFlags: NIF_MESSAGE | NIF_TIP,
            uCallbackMessage: TRAY_CALLBACK_MSG,
            ..Default::default()
        };
        data.szTip = tip_buf;
        let msg = match op {
            NotifyIconOp::Add => NIM_ADD,
            NotifyIconOp::Modify => NIM_MODIFY,
            NotifyIconOp::Delete => NIM_DELETE,
        };
        let ok = unsafe { Shell_NotifyIconW(msg, &data) }.as_bool();
        if !ok {
            tracing::warn!(target: "picoo_tray", ?op, "Shell_NotifyIconW returned false");
        }
    }
}

/// Hover tip text derived from receiver session status (REQ-PICOO-UI-008).
pub fn tip_for_status(status: ReceiverStatus) -> String {
    format!("Picoo Camera — {}", status.as_label())
}

static NOTIFY_ICON: Mutex<NotifyIconController> = Mutex::new(NotifyIconController {
    visible: false,
    tip: String::new(),
    hwnd: None,
    ops: Vec::new(),
});

static PENDING_MENU_ACTION: Mutex<Option<TrayMenuAction>> = Mutex::new(None);

#[cfg(all(windows, feature = "windows-vcam"))]
const TRAY_ICON_ID: u32 = 1;
#[cfg(all(windows, feature = "windows-vcam"))]
const TRAY_CALLBACK_MSG: u32 = windows::Win32::UI::WindowsAndMessaging::WM_APP + 40;

#[cfg(all(windows, feature = "windows-vcam"))]
static TRAY_HOST_HWND: Mutex<Option<isize>> = Mutex::new(None);

#[cfg(all(windows, feature = "windows-vcam"))]
fn tray_message_hwnd() -> Option<windows::Win32::Foundation::HWND> {
    use windows::core::w;
    use windows::Win32::Foundation::{GetLastError, HWND};
    use windows::Win32::UI::WindowsAndMessaging::{
        CreateWindowExW, RegisterClassW, HWND_MESSAGE, WNDCLASSW,
    };

    if let Ok(guard) = TRAY_HOST_HWND.lock() {
        if let Some(raw) = *guard {
            return Some(HWND(raw as *mut _));
        }
    }

    let class_name = w!("PicooTrayHost");
    let wc = WNDCLASSW {
        lpfnWndProc: Some(tray_wnd_proc),
        lpszClassName: class_name,
        ..Default::default()
    };
    let atom = unsafe { RegisterClassW(&wc) };
    if atom == 0 {
        let err = unsafe { GetLastError() };
        // ERROR_CLASS_ALREADY_EXISTS = 1410
        if err.0 != 1410 {
            tracing::warn!(target: "picoo_tray", ?err, "RegisterClassW for tray host failed");
            return None;
        }
    }

    let hwnd = unsafe {
        CreateWindowExW(
            Default::default(),
            class_name,
            w!("Picoo Tray Host"),
            Default::default(),
            0,
            0,
            0,
            0,
            Some(HWND_MESSAGE),
            None,
            None,
            None,
        )
    };
    let Ok(hwnd) = hwnd else {
        tracing::warn!(target: "picoo_tray", "CreateWindowExW HWND_MESSAGE failed");
        return None;
    };
    if hwnd.is_invalid() {
        return None;
    }
    if let Ok(mut guard) = TRAY_HOST_HWND.lock() {
        *guard = Some(hwnd.0 as isize);
    }
    Some(hwnd)
}

#[cfg(all(windows, feature = "windows-vcam"))]
unsafe extern "system" fn tray_wnd_proc(
    hwnd: windows::Win32::Foundation::HWND,
    msg: u32,
    wparam: windows::Win32::Foundation::WPARAM,
    lparam: windows::Win32::Foundation::LPARAM,
) -> windows::Win32::Foundation::LRESULT {
    use windows::Win32::Foundation::LRESULT;
    use windows::Win32::UI::WindowsAndMessaging::{
        DefWindowProcW, WM_COMMAND, WM_CONTEXTMENU, WM_LBUTTONDBLCLK, WM_RBUTTONUP,
    };

    if msg == TRAY_CALLBACK_MSG {
        let mouse = lparam.0 as u32;
        if mouse == WM_RBUTTONUP || mouse == WM_CONTEXTMENU {
            show_tray_context_menu(hwnd);
            return LRESULT(0);
        }
        if mouse == WM_LBUTTONDBLCLK {
            enqueue_menu_action(TrayMenuAction::Show);
            return LRESULT(0);
        }
    }
    if msg == WM_COMMAND {
        match (wparam.0 as u32) & 0xffff {
            1 => enqueue_menu_action(TrayMenuAction::Show),
            2 => enqueue_menu_action(TrayMenuAction::Quit),
            _ => {}
        }
        return LRESULT(0);
    }
    unsafe { DefWindowProcW(hwnd, msg, wparam, lparam) }
}

#[cfg(all(windows, feature = "windows-vcam"))]
fn show_tray_context_menu(hwnd: windows::Win32::Foundation::HWND) {
    use windows::core::w;
    use windows::Win32::Foundation::POINT;
    use windows::Win32::UI::WindowsAndMessaging::{
        AppendMenuW, CreatePopupMenu, DestroyMenu, GetCursorPos, SetForegroundWindow,
        TrackPopupMenu, MF_STRING, TPM_LEFTALIGN, TPM_RIGHTBUTTON,
    };

    let Ok(menu) = (unsafe { CreatePopupMenu() }) else {
        return;
    };
    unsafe {
        let _ = AppendMenuW(menu, MF_STRING, 1, w!("Show Picoo Camera"));
        let _ = AppendMenuW(menu, MF_STRING, 2, w!("Quit"));
        let mut pt = POINT::default();
        let _ = GetCursorPos(&mut pt);
        let _ = SetForegroundWindow(hwnd);
        let _ = TrackPopupMenu(
            menu,
            TPM_LEFTALIGN | TPM_RIGHTBUTTON,
            pt.x,
            pt.y,
            None,
            hwnd,
            None,
        );
        let _ = DestroyMenu(menu);
    }
}

/// Drain Win32 tray host messages so context-menu actions reach the GPUI pump.
#[cfg_attr(not(feature = "gpui-ui"), allow(dead_code))]
pub fn pump_win32_tray_messages() {
    #[cfg(all(windows, feature = "windows-vcam"))]
    {
        use windows::Win32::UI::WindowsAndMessaging::{
            DispatchMessageW, PeekMessageW, TranslateMessage, MSG, PM_REMOVE,
        };
        let Some(hwnd) = tray_message_hwnd() else {
            return;
        };
        unsafe {
            let mut msg = MSG::default();
            while PeekMessageW(&mut msg, Some(hwnd), 0, 0, PM_REMOVE).as_bool() {
                let _ = TranslateMessage(&msg);
                DispatchMessageW(&msg);
            }
        }
    }
}

/// Soft notify that the UI hid to tray; records Shell_NotifyIcon ADD/MODIFY intent.
#[cfg_attr(not(feature = "gpui-ui"), allow(dead_code))]
pub fn note_hidden_to_tray() {
    note_hidden_to_tray_with_tip("Picoo Camera");
}

/// Hide-to-tray with a live status tip (preferred from GPUI close handler).
#[cfg_attr(not(feature = "gpui-ui"), allow(dead_code))]
pub fn note_hidden_to_tray_with_tip(tip: &str) {
    if let Ok(mut icon) = NOTIFY_ICON.lock() {
        icon.show(tip);
    }
    tracing::info!(
        target: "picoo_tray",
        tip = %tip,
        "window close → hide to tray (REQ-PICOO-UI-008); Shell_NotifyIcon HWND optional"
    );
}

/// Keep tray tip in sync while the icon is visible (status pump).
#[cfg_attr(not(feature = "gpui-ui"), allow(dead_code))]
pub fn sync_tray_tip(status: ReceiverStatus) {
    let tip = tip_for_status(status);
    if let Ok(mut icon) = NOTIFY_ICON.lock() {
        if icon.is_visible() {
            icon.set_tip(&tip);
        }
    }
}

/// Clear tray icon when quitting (Windows `NIM_DELETE` when HWND is known).
#[cfg_attr(not(feature = "gpui-ui"), allow(dead_code))]
pub fn note_tray_cleared() {
    if let Ok(mut icon) = NOTIFY_ICON.lock() {
        icon.hide();
    }
}

/// Test / GPUI / Win32 hook: provide HWND once the platform window exists.
#[allow(dead_code)]
pub fn set_notify_icon_hwnd(hwnd: Option<isize>) {
    if let Ok(mut icon) = NOTIFY_ICON.lock() {
        icon.set_hwnd(hwnd);
    }
}

/// Queue a context-menu action (Win32 menu → GPUI pump).
#[cfg_attr(not(feature = "gpui-ui"), allow(dead_code))]
pub fn enqueue_menu_action(action: TrayMenuAction) {
    if let Ok(mut slot) = PENDING_MENU_ACTION.lock() {
        *slot = Some(action);
    }
}

/// Drain one pending tray menu action for the GPUI / app pump.
#[cfg_attr(not(feature = "gpui-ui"), allow(dead_code))]
pub fn take_pending_menu_action() -> Option<TrayMenuAction> {
    PENDING_MENU_ACTION.lock().ok()?.take()
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
    fn tray_menu_apply_show_and_quit() {
        assert_eq!(
            TrayMenuAction::Show.apply(),
            TrayMenuOutcome {
                restore_window: true,
                quit: false,
            }
        );
        assert_eq!(
            TrayMenuAction::Quit.apply(),
            TrayMenuOutcome {
                restore_window: false,
                quit: true,
            }
        );
    }

    #[test]
    fn tip_for_status_includes_label() {
        assert_eq!(
            tip_for_status(ReceiverStatus::Streaming),
            "Picoo Camera — Streaming"
        );
        assert_eq!(
            tip_for_status(ReceiverStatus::Discovering),
            "Picoo Camera — Discovering"
        );
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
    fn set_tip_modifies_when_visible() {
        let mut icon = NotifyIconController::new();
        icon.show("Picoo Camera — Discovering");
        icon.take_ops();
        icon.set_tip("Picoo Camera — Streaming");
        assert_eq!(icon.take_ops(), vec![NotifyIconOp::Modify]);
        assert_eq!(icon.tip(), "Picoo Camera — Streaming");
        icon.set_tip("Picoo Camera — Streaming"); // no-op
        assert!(icon.take_ops().is_empty());
    }

    #[test]
    fn note_hidden_to_tray_shows_global_icon() {
        note_tray_cleared();
        note_hidden_to_tray_with_tip("Picoo Camera — Discovering");
        let icon = NOTIFY_ICON.lock().expect("lock");
        assert!(icon.is_visible());
        assert_eq!(icon.tip(), "Picoo Camera — Discovering");
        assert!(icon.ops.contains(&NotifyIconOp::Add) || icon.ops.contains(&NotifyIconOp::Modify));
    }

    #[test]
    fn sync_tray_tip_updates_while_visible() {
        note_tray_cleared();
        note_hidden_to_tray_with_tip("Picoo Camera — Discovering");
        sync_tray_tip(ReceiverStatus::Streaming);
        let icon = NOTIFY_ICON.lock().expect("lock");
        assert_eq!(icon.tip(), "Picoo Camera — Streaming");
    }

    #[test]
    fn enqueue_and_take_menu_action() {
        let _ = take_pending_menu_action();
        enqueue_menu_action(TrayMenuAction::Show);
        assert_eq!(take_pending_menu_action(), Some(TrayMenuAction::Show));
        assert_eq!(take_pending_menu_action(), None);
        enqueue_menu_action(TrayMenuAction::Quit);
        let outcome = take_pending_menu_action().unwrap().apply();
        assert!(outcome.quit);
    }
}
