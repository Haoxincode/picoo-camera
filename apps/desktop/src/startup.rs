//! Launch-at-startup preference wiring — REQ-PICOO-UI-007 / PRD §16.
//!
//! The OS integration is delegated to `auto-launch`; this module keeps the
//! Picoo policy boundary (Windows current-user scope, macOS SMAppService and
//! explicit user actions) unit-testable.

use std::path::Path;

/// Value name written under the Windows Run key.
pub const RUN_VALUE_NAME: &str = "PicooCamera";

/// Abstraction over OS startup registration (unit-testable).
pub trait StartupStore {
    #[allow(dead_code)] // used by unit tests / future prefs UI introspection
    fn get(&self, name: &str) -> Option<String>;
    fn set(&mut self, name: &str, command: &str) -> Result<(), String>;
    fn remove(&mut self, name: &str) -> Result<(), String>;
}

/// In-memory store used by policy tests; production uses `AutoLaunchStartupStore`.
#[derive(Debug, Default, Clone)]
pub struct MemoryStartupStore {
    entries: std::collections::BTreeMap<String, String>,
}

impl StartupStore for MemoryStartupStore {
    fn get(&self, name: &str) -> Option<String> {
        self.entries.get(name).cloned()
    }

    fn set(&mut self, name: &str, command: &str) -> Result<(), String> {
        self.entries.insert(name.to_string(), command.to_string());
        Ok(())
    }

    fn remove(&mut self, name: &str) -> Result<(), String> {
        self.entries.remove(name);
        Ok(())
    }
}

/// Quote an executable path for a Run-key command line.
pub fn startup_command_line(exe: &Path) -> String {
    let path = exe.display().to_string();
    if path.contains(' ') {
        format!("\"{path}\"")
    } else {
        path
    }
}

/// Enable or disable launch-at-startup using the provided store.
pub fn apply_launch_at_startup<S: StartupStore + ?Sized>(
    store: &mut S,
    enabled: bool,
    exe: &Path,
) -> Result<(), String> {
    if enabled {
        store.set(RUN_VALUE_NAME, &startup_command_line(exe))
    } else {
        store.remove(RUN_VALUE_NAME)
    }
}

/// Adapter around the mature platform-specific login-item implementation.
pub struct AutoLaunchStartupStore {
    launch: auto_launch::AutoLaunch,
    command: String,
}

impl StartupStore for AutoLaunchStartupStore {
    fn get(&self, name: &str) -> Option<String> {
        (name == RUN_VALUE_NAME && self.launch.is_enabled().ok() == Some(true))
            .then(|| self.command.clone())
    }

    fn set(&mut self, name: &str, command: &str) -> Result<(), String> {
        if name != RUN_VALUE_NAME {
            return Err(format!("unsupported startup value: {name}"));
        }
        // `AutoLaunch` owns the quoting/escaping rules for each platform. The
        // command is retained only for the trait's introspection contract.
        self.launch.enable().map_err(|error| error.to_string())?;
        self.command = command.to_string();
        Ok(())
    }

    fn remove(&mut self, name: &str) -> Result<(), String> {
        if name != RUN_VALUE_NAME {
            return Err(format!("unsupported startup value: {name}"));
        }
        self.launch.disable().map_err(|error| error.to_string())
    }
}

pub fn platform_startup_store(exe: &Path) -> Result<Box<dyn StartupStore + Send>, String> {
    let path = exe
        .to_str()
        .ok_or_else(|| format!("executable path is not valid UTF-8: {}", exe.display()))?;
    let mut builder = auto_launch::AutoLaunchBuilder::new();
    builder.set_app_name("PicooCamera").set_app_path(path);
    #[cfg(windows)]
    builder.set_windows_enable_mode(auto_launch::WindowsEnableMode::CurrentUser);
    #[cfg(target_os = "macos")]
    builder.set_macos_launch_mode(auto_launch::MacOSLaunchMode::SMAppService);
    #[cfg(target_os = "linux")]
    builder.set_linux_launch_mode(auto_launch::LinuxLaunchMode::XdgAutostart);
    let launch = builder.build().map_err(|error| error.to_string())?;
    Ok(Box::new(AutoLaunchStartupStore {
        launch,
        command: startup_command_line(exe),
    }))
}

/// Apply prefs to the OS startup registration for the current executable.
#[cfg_attr(not(feature = "gpui-ui"), allow(dead_code))]
pub fn sync_launch_at_startup(enabled: bool) -> Result<(), String> {
    let exe = std::env::current_exe().map_err(|e| format!("current_exe: {e}"))?;
    let mut store = platform_startup_store(&exe)?;
    apply_launch_at_startup(&mut *store, enabled, &exe)
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::path::PathBuf;

    #[test]
    fn quotes_paths_with_spaces() {
        let cmd = startup_command_line(Path::new(
            r"C:\Program Files\Picoo Camera\picoo-desktop.exe",
        ));
        assert!(cmd.starts_with('"'));
        assert!(cmd.ends_with('"'));
    }

    #[test]
    fn apply_enable_and_disable_on_memory_store() {
        let mut store = MemoryStartupStore::default();
        let exe = PathBuf::from("/opt/picoo/picoo-desktop");
        apply_launch_at_startup(&mut store, true, &exe).expect("enable");
        assert_eq!(
            store.get(RUN_VALUE_NAME).as_deref(),
            Some("/opt/picoo/picoo-desktop")
        );
        apply_launch_at_startup(&mut store, false, &exe).expect("disable");
        assert!(store.get(RUN_VALUE_NAME).is_none());
    }
}
