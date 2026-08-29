//! Launch-at-startup preference wiring — REQ-PICOO-UI-007 / PRD §16.
//!
//! Windows: HKCU Run key. Other platforms: no-op store (prefs still persist).

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

/// In-memory store for tests and non-Windows hosts.
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

/// Platform store: Windows registry, otherwise memory (no OS effect).
#[cfg(windows)]
pub fn platform_startup_store() -> Result<Box<dyn StartupStore + Send>, String> {
    Ok(Box::new(WindowsRunKeyStore::open()?))
}

#[cfg(not(windows))]
pub fn platform_startup_store() -> Result<Box<dyn StartupStore + Send>, String> {
    Ok(Box::new(MemoryStartupStore::default()))
}

/// Apply prefs to the OS startup registration for the current executable.
#[cfg_attr(not(feature = "gpui-ui"), allow(dead_code))]
pub fn sync_launch_at_startup(enabled: bool) -> Result<(), String> {
    let exe = std::env::current_exe().map_err(|e| format!("current_exe: {e}"))?;
    let mut store = platform_startup_store()?;
    apply_launch_at_startup(&mut *store, enabled, &exe)
}

#[cfg(windows)]
mod windows_run {
    use super::*;
    use std::ffi::OsStr;
    use std::os::windows::ffi::OsStrExt;

    use windows::core::PCWSTR;
    use windows::Win32::Foundation::{ERROR_FILE_NOT_FOUND, ERROR_SUCCESS};
    use windows::Win32::System::Registry::{
        RegCloseKey, RegDeleteValueW, RegOpenKeyExW, RegQueryValueExW, RegSetValueExW, HKEY,
        HKEY_CURRENT_USER, KEY_READ, KEY_WRITE, REG_SZ,
    };

    const RUN_SUBKEY: &str = r"Software\Microsoft\Windows\CurrentVersion\Run";

    /// Stateless Run-key accessor. Opens/closes HKEY per call so the type is `Send`
    /// (raw `HKEY` is `!Send`, so retaining it would break the `gpui-ui` Windows build).
    #[derive(Debug, Default)]
    pub struct WindowsRunKeyStore;

    impl WindowsRunKeyStore {
        pub fn open() -> Result<Self, String> {
            // Probe that the Run key is reachable at construction time.
            let store = Self;
            store.with_key(|_| Ok(()))?;
            Ok(store)
        }

        fn with_key<R>(&self, f: impl FnOnce(HKEY) -> Result<R, String>) -> Result<R, String> {
            let subkey = wide(RUN_SUBKEY);
            let mut key = HKEY::default();
            let status = unsafe {
                RegOpenKeyExW(
                    HKEY_CURRENT_USER,
                    PCWSTR(subkey.as_ptr()),
                    None,
                    KEY_READ | KEY_WRITE,
                    &mut key,
                )
            };
            if status != ERROR_SUCCESS {
                return Err(format!("RegOpenKeyExW Run failed: {status:?}"));
            }
            let result = f(key);
            unsafe {
                let _ = RegCloseKey(key);
            }
            result
        }
    }

    impl StartupStore for WindowsRunKeyStore {
        fn get(&self, name: &str) -> Option<String> {
            self.with_key(|key| {
                let name_w = wide(name);
                let mut ty = REG_SZ;
                let mut size = 0u32;
                let status = unsafe {
                    RegQueryValueExW(
                        key,
                        PCWSTR(name_w.as_ptr()),
                        None,
                        Some(&mut ty),
                        None,
                        Some(&mut size),
                    )
                };
                if status != ERROR_SUCCESS || size == 0 {
                    return Ok(None);
                }
                let mut buf = vec![0u16; (size as usize / 2).max(1)];
                let status = unsafe {
                    RegQueryValueExW(
                        key,
                        PCWSTR(name_w.as_ptr()),
                        None,
                        Some(&mut ty),
                        Some(buf.as_mut_ptr().cast()),
                        Some(&mut size),
                    )
                };
                if status != ERROR_SUCCESS {
                    return Ok(None);
                }
                let len = buf.iter().position(|&c| c == 0).unwrap_or(buf.len());
                Ok(Some(String::from_utf16_lossy(&buf[..len])))
            })
            .ok()
            .flatten()
        }

        fn set(&mut self, name: &str, command: &str) -> Result<(), String> {
            self.with_key(|key| {
                let name_w = wide(name);
                let value_w = wide(command);
                let bytes = (value_w.len() * 2) as u32;
                let status = unsafe {
                    RegSetValueExW(
                        key,
                        PCWSTR(name_w.as_ptr()),
                        None,
                        REG_SZ,
                        Some(std::slice::from_raw_parts(
                            value_w.as_ptr().cast::<u8>(),
                            bytes as usize,
                        )),
                    )
                };
                if status != ERROR_SUCCESS {
                    return Err(format!("RegSetValueExW failed: {status:?}"));
                }
                Ok(())
            })
        }

        fn remove(&mut self, name: &str) -> Result<(), String> {
            self.with_key(|key| {
                let name_w = wide(name);
                let status = unsafe { RegDeleteValueW(key, PCWSTR(name_w.as_ptr())) };
                if status == ERROR_SUCCESS || status == ERROR_FILE_NOT_FOUND {
                    return Ok(());
                }
                Err(format!("RegDeleteValueW failed: {status:?}"))
            })
        }
    }

    fn wide(text: &str) -> Vec<u16> {
        OsStr::new(text).encode_wide().chain(Some(0)).collect()
    }
}

#[cfg(windows)]
use windows_run::WindowsRunKeyStore;

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
