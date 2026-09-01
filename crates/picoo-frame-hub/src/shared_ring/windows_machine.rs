use std::path::{Path, PathBuf};

/// Per-machine directory provisioned by PicooCamera.msi for the Receiver ↔
/// Windows Frame Server shared ring. The Frame Server source runs as Local
/// Service in Session 0, so user-profile Temp paths and session-local named
/// mappings are not a valid cross-process identity.
pub const WINDOWS_SHARED_RING_DIRECTORY: &str = "Picoo Camera";

pub fn windows_shared_ring_path(name: &str) -> PathBuf {
    windows_shared_ring_path_in(program_data_directory(), name)
}

fn program_data_directory() -> PathBuf {
    known_program_data_directory().unwrap_or_else(|| PathBuf::from(r"C:\ProgramData"))
}

fn known_program_data_directory() -> Option<PathBuf> {
    use std::{ffi::OsString, os::windows::ffi::OsStringExt};
    use windows_sys::Win32::{
        System::Com::CoTaskMemFree,
        UI::Shell::{FOLDERID_ProgramData, SHGetKnownFolderPath, KF_FLAG_DEFAULT},
    };

    let mut raw_path = std::ptr::null_mut();
    // SAFETY: SHGetKnownFolderPath initializes a CoTaskMem-owned, NUL-terminated
    // UTF-16 buffer on success. The buffer is copied before CoTaskMemFree.
    let result = unsafe {
        SHGetKnownFolderPath(
            &FOLDERID_ProgramData,
            KF_FLAG_DEFAULT as u32,
            std::ptr::null_mut(),
            &mut raw_path,
        )
    };
    if result < 0 || raw_path.is_null() {
        return None;
    }
    let mut len = 0;
    // SAFETY: Success guarantees a NUL-terminated PWSTR.
    unsafe {
        while *raw_path.add(len) != 0 {
            len += 1;
        }
    }
    // SAFETY: The preceding scan found the terminator within the API-owned
    // string, so the slice contains exactly the path code units.
    let path = unsafe { OsString::from_wide(std::slice::from_raw_parts(raw_path, len)) };
    // SAFETY: raw_path was allocated by SHGetKnownFolderPath.
    unsafe { CoTaskMemFree(raw_path.cast()) };
    (!path.is_empty()).then(|| PathBuf::from(path))
}

fn windows_shared_ring_path_in(base: impl AsRef<Path>, name: &str) -> PathBuf {
    // Encode every byte so a caller-provided diagnostic ring name can never
    // escape the installer-owned directory or collide through path separators.
    let encoded_name = name
        .as_bytes()
        .iter()
        .map(|byte| format!("{byte:02x}"))
        .collect::<String>();
    base.as_ref()
        .join(WINDOWS_SHARED_RING_DIRECTORY)
        .join(format!("frame-ring-{encoded_name}.bin"))
}

#[cfg(test)]
mod tests {
    use super::{windows_shared_ring_path_in, WINDOWS_SHARED_RING_DIRECTORY};
    use std::{ffi::OsStr, path::Path};

    #[test]
    fn ring_name_cannot_escape_machine_directory() {
        let path = windows_shared_ring_path_in(Path::new(r"C:\ProgramData"), r"..\other/ring");
        assert_eq!(
            path.parent().and_then(Path::file_name),
            Some(OsStr::new(WINDOWS_SHARED_RING_DIRECTORY))
        );
        assert!(path
            .file_name()
            .and_then(|name| name.to_str())
            .is_some_and(|name| !name.contains('/') && !name.contains('\\')));
    }
}
