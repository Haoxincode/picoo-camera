//! Visible Windows startup failures — GUI subsystem has no console.

use std::fs::OpenOptions;
use std::io::Write;

use windows::core::{w, PCWSTR};
use windows::Win32::UI::WindowsAndMessaging::{MessageBoxW, MB_ICONERROR, MB_OK};

pub fn install_panic_hook() {
    let previous = std::panic::take_hook();
    std::panic::set_hook(Box::new(move |info| {
        report(&format!("Picoo Camera 启动时崩溃：{info}"));
        previous(info);
    }));
}

pub fn report_error(error: &impl std::fmt::Display) {
    report(&format!("无法启动 Picoo Camera：{error}"));
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

fn append_log(message: &str) {
    let Some(path) = crate::prefs::log_file_path() else {
        return;
    };
    if let Some(parent) = path.parent() {
        let _ = std::fs::create_dir_all(parent);
    }
    if let Ok(mut file) = OpenOptions::new().create(true).append(true).open(path) {
        let _ = writeln!(file, "{message}");
    }
}
