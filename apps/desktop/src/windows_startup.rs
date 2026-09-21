//! Visible Windows startup failures — GUI subsystem has no console.

use std::fs::OpenOptions;
use std::io::Write;
use std::os::windows::io::{FromRawHandle, OwnedHandle, RawHandle};
use std::sync::OnceLock;

use windows::core::{w, PCWSTR};
use windows::Win32::Foundation::{GetLastError, ERROR_ALREADY_EXISTS};
use windows::Win32::System::Threading::CreateMutexW;
use windows::Win32::UI::WindowsAndMessaging::{
    FindWindowW, MessageBoxW, SetForegroundWindow, ShowWindow, MB_ICONERROR, MB_OK, SW_RESTORE,
    SW_SHOW,
};

static INSTANCE_MUTEX: OnceLock<OwnedHandle> = OnceLock::new();

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum InstanceClaim {
    Unique,
    AlreadyRunning,
}

pub fn install_panic_hook() {
    let previous = std::panic::take_hook();
    std::panic::set_hook(Box::new(move |info| {
        report(&format!("Picoo Camera 启动时崩溃：{info}"));
        previous(info);
    }));
}

/// Keep one interactive desktop process. A second shortcut click restores the
/// existing window (often hidden in the tray) instead of failing to bind UDP 4433.
pub fn claim_single_instance() -> InstanceClaim {
    let handle = match unsafe { CreateMutexW(None, true, w!("Local\\PicooCamera.SingleInstance")) }
    {
        Ok(handle) => handle,
        Err(error) => {
            tracing::warn!(%error, "single-instance mutex unavailable");
            return InstanceClaim::Unique;
        }
    };
    let already = unsafe { GetLastError() } == ERROR_ALREADY_EXISTS;
    let owned = unsafe { OwnedHandle::from_raw_handle(handle.0 as RawHandle) };
    if already {
        if !activate_running_instance() {
            report(
                "Picoo Camera 已在运行。请点击任务栏托盘图标打开窗口，或在任务管理器中结束 picoo-desktop 后再试。",
            );
        }
        return InstanceClaim::AlreadyRunning;
    }
    let _ = INSTANCE_MUTEX.set(owned);
    append_log("single-instance mutex acquired; launching GPUI");
    InstanceClaim::Unique
}

pub fn report_error(error: &impl std::fmt::Display) {
    report(&startup_error_message(&error.to_string()));
}

fn startup_error_message(error: &str) -> String {
    if error.contains("10048")
        || error.contains("只允许使用一次")
        || error.contains("Address already in use")
    {
        "无法启动 Picoo Camera：局域网端口 4433 已被占用。请先退出任务栏托盘中已运行的 Picoo Camera，或关闭占用该端口的程序。".into()
    } else {
        format!("无法启动 Picoo Camera：{error}")
    }
}

fn activate_running_instance() -> bool {
    // GPUI registers class "Zed::Window". Title can still be empty if the
    // custom TitleBar has not called SetWindowTextW yet. Class-only is last
    // so a running Zed editor is not activated instead of Picoo Camera.
    let hwnd = unsafe { FindWindowW(w!("Zed::Window"), w!("Picoo Camera")) }
        .ok()
        .or_else(|| unsafe { FindWindowW(None, w!("Picoo Camera")) }.ok())
        .or_else(|| unsafe { FindWindowW(w!("Zed::Window"), None) }.ok());
    let Some(hwnd) = hwnd.filter(|hwnd| !hwnd.is_invalid()) else {
        return false;
    };
    unsafe {
        let _ = ShowWindow(hwnd, SW_RESTORE);
        let _ = ShowWindow(hwnd, SW_SHOW);
        let _ = SetForegroundWindow(hwnd);
    }
    true
}

fn report(message: &str) {
    tracing::error!("{message}");
    append_log(message);
    let mut text: Vec<u16> = message.encode_utf16().collect();
    if text.len() > 2000 {
        text.truncate(2000);
    }
    text.push(0);
    unsafe {
        MessageBoxW(
            None,
            PCWSTR(text.as_ptr()),
            w!("Picoo Camera"),
            MB_OK | MB_ICONERROR,
        );
    }
}

pub(crate) fn append_log(message: &str) {
    let Some(path) = crate::prefs::log_file_path() else {
        return;
    };
    if let Some(parent) = path.parent() {
        let _ = std::fs::create_dir_all(parent);
    }
    if let Ok(mut file) = OpenOptions::new().create(true).append(true).open(path) {
        let _ = writeln!(file, "{:?} {message}", std::time::SystemTime::now());
    }
}

#[cfg(test)]
mod tests {
    use super::startup_error_message;

    #[test]
    fn address_in_use_explains_the_quic_port() {
        let message = startup_error_message(
            "transport: connection failed: I/O error: 通常每个套接字地址(协议/网络地址/端口)只允许使用一次。 (os error 10048)",
        );
        assert!(message.contains("4433"), "{message}");
        assert!(message.contains("托盘"), "{message}");
    }
}
