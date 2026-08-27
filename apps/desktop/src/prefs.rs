//! Desktop preferences persistence — REQ-PICOO-UI-002 / PRD §16.

#![cfg_attr(not(feature = "gpui-ui"), allow(dead_code))]

use std::fs;
use std::path::PathBuf;

use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Default)]
pub enum LogLevel {
    Error,
    Warn,
    #[default]
    Info,
    Debug,
    Trace,
}

impl LogLevel {
    pub const ALL: [LogLevel; 5] = [
        LogLevel::Error,
        LogLevel::Warn,
        LogLevel::Info,
        LogLevel::Debug,
        LogLevel::Trace,
    ];

    pub fn label(self) -> &'static str {
        match self {
            LogLevel::Error => "Error",
            LogLevel::Warn => "Warn",
            LogLevel::Info => "Info",
            LogLevel::Debug => "Debug",
            LogLevel::Trace => "Trace",
        }
    }

    pub fn env_filter(self) -> &'static str {
        match self {
            LogLevel::Error => "error",
            LogLevel::Warn => "warn",
            LogLevel::Info => "info",
            LogLevel::Debug => "debug",
            LogLevel::Trace => "trace",
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DesktopPreferences {
    pub first_launch_completed: bool,
    pub display_name: String,
    pub auto_accept_paired: bool,
    pub launch_at_startup: bool,
    pub minimize_to_tray: bool,
    pub use_default_placeholder: bool,
    pub log_level: LogLevel,
}

impl Default for DesktopPreferences {
    fn default() -> Self {
        Self {
            first_launch_completed: false,
            display_name: "Picoo Camera".into(),
            auto_accept_paired: true,
            launch_at_startup: false,
            minimize_to_tray: true,
            use_default_placeholder: true,
            log_level: LogLevel::Info,
        }
    }
}

pub fn prefs_path() -> PathBuf {
    std::env::var("PICOO_PREFS")
        .map(PathBuf::from)
        .unwrap_or_else(|_| {
            if cfg!(target_os = "windows") {
                std::env::var("APPDATA")
                    .map(|appdata| {
                        PathBuf::from(appdata)
                            .join("picoo-camera")
                            .join("prefs.json")
                    })
                    .unwrap_or_else(|_| PathBuf::from("prefs.json"))
            } else {
                let home = std::env::var("HOME").unwrap_or_else(|_| ".".into());
                PathBuf::from(home)
                    .join(".config")
                    .join("picoo-camera")
                    .join("prefs.json")
            }
        })
}

pub fn load_prefs() -> DesktopPreferences {
    let path = prefs_path();
    match fs::read_to_string(&path) {
        Ok(raw) => serde_json::from_str(&raw).unwrap_or_default(),
        Err(_) => DesktopPreferences::default(),
    }
}

pub fn save_prefs(prefs: &DesktopPreferences) -> Result<(), String> {
    let path = prefs_path();
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent).map_err(|err| format!("create prefs dir: {err}"))?;
    }
    let json =
        serde_json::to_string_pretty(prefs).map_err(|err| format!("serialize prefs: {err}"))?;
    fs::write(&path, json).map_err(|err| format!("write prefs: {err}"))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn default_prefs_roundtrip() {
        let prefs = DesktopPreferences::default();
        let json = serde_json::to_string(&prefs).unwrap();
        let parsed: DesktopPreferences = serde_json::from_str(&json).unwrap();
        assert_eq!(parsed.display_name, "Picoo Camera");
        assert!(parsed.auto_accept_paired);
    }
}
