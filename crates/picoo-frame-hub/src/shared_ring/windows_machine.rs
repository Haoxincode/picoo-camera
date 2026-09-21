use std::os::windows::ffi::OsStrExt;
use std::path::{Path, PathBuf};

use windows::core::PCWSTR;
use windows::Win32::Foundation::{CloseHandle, LocalFree, ERROR_SUCCESS, HANDLE, HLOCAL, LUID};
use windows::Win32::Security::Authorization::{
    ConvertStringSecurityDescriptorToSecurityDescriptorW, SetNamedSecurityInfoW, SDDL_REVISION_1,
    SE_FILE_OBJECT,
};
use windows::Win32::Security::{
    AdjustTokenPrivileges, GetSecurityDescriptorDacl, GetSecurityDescriptorSacl,
    LookupPrivilegeValueW, ACL, DACL_SECURITY_INFORMATION, LABEL_SECURITY_INFORMATION,
    LUID_AND_ATTRIBUTES, PROTECTED_DACL_SECURITY_INFORMATION, PROTECTED_SACL_SECURITY_INFORMATION,
    PSECURITY_DESCRIPTOR, SACL_SECURITY_INFORMATION, SE_PRIVILEGE_ENABLED, SE_SECURITY_NAME,
    TOKEN_ADJUST_PRIVILEGES, TOKEN_PRIVILEGES, TOKEN_QUERY,
};
use windows::Win32::System::Threading::{GetCurrentProcess, OpenProcessToken};

use super::lock::map_file_err;
use super::windows_acl::WINDOWS_SHARED_RING_DIRECTORY_SDDL;
use super::SharedRingError;

pub use super::windows_acl::WINDOWS_SHARED_RING_DIRECTORY;

pub fn windows_shared_ring_path(name: &str) -> PathBuf {
    windows_shared_ring_path_in(program_data_directory(), name)
}

pub fn windows_shared_ring_directory() -> PathBuf {
    program_data_directory().join(WINDOWS_SHARED_RING_DIRECTORY)
}

/// Create `%ProgramData%\Picoo Camera` and apply the installer SDDL.
///
/// Ordinary Receiver startup never calls this. MSI and the elevated
/// `--register-vcam` repair path are the only writers of directory ACL/IL.
pub fn provision_windows_shared_ring_directory() -> Result<PathBuf, SharedRingError> {
    let directory = windows_shared_ring_directory();
    std::fs::create_dir_all(&directory).map_err(|error| map_file_err(&directory, error))?;
    apply_shared_ring_security(&directory)?;
    if let Ok(entries) = std::fs::read_dir(&directory) {
        for entry in entries.flatten() {
            let name = entry.file_name();
            if name
                .to_str()
                .is_some_and(|name| name.starts_with("frame-ring-"))
            {
                apply_shared_ring_security(&entry.path())?;
            }
        }
    }
    Ok(directory)
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

fn apply_shared_ring_security(path: &Path) -> Result<(), SharedRingError> {
    let wide = path
        .as_os_str()
        .encode_wide()
        .chain(std::iter::once(0))
        .collect::<Vec<_>>();
    let sddl = WINDOWS_SHARED_RING_DIRECTORY_SDDL
        .encode_utf16()
        .chain(std::iter::once(0))
        .collect::<Vec<_>>();
    let mut descriptor = PSECURITY_DESCRIPTOR::default();
    unsafe {
        ConvertStringSecurityDescriptorToSecurityDescriptorW(
            PCWSTR(sddl.as_ptr()),
            SDDL_REVISION_1,
            &mut descriptor,
            None,
        )
        .map_err(|error| security_err(path, error))?;
    }
    let result = (|| {
        let mut dacl_present = false.into();
        let mut dacl_defaulted = false.into();
        let mut dacl = std::ptr::null_mut::<ACL>();
        unsafe {
            GetSecurityDescriptorDacl(
                descriptor,
                &mut dacl_present,
                &mut dacl,
                &mut dacl_defaulted,
            )
            .map_err(|error| security_err(path, error))?;
        }
        if !bool::from(dacl_present) || dacl.is_null() {
            return Err(SharedRingError::FileMapping {
                path: path.to_path_buf(),
                message: "shared ring directory SDDL is missing a DACL".into(),
            });
        }

        let mut sacl_present = false.into();
        let mut sacl_defaulted = false.into();
        let mut sacl = std::ptr::null_mut::<ACL>();
        unsafe {
            GetSecurityDescriptorSacl(
                descriptor,
                &mut sacl_present,
                &mut sacl,
                &mut sacl_defaulted,
            )
            .map_err(|error| security_err(path, error))?;
        }

        let _privilege = enable_security_privilege();
        let info = DACL_SECURITY_INFORMATION
            | PROTECTED_DACL_SECURITY_INFORMATION
            | LABEL_SECURITY_INFORMATION
            | SACL_SECURITY_INFORMATION
            | PROTECTED_SACL_SECURITY_INFORMATION;
        let error = unsafe {
            SetNamedSecurityInfoW(
                PCWSTR(wide.as_ptr()),
                SE_FILE_OBJECT,
                info,
                None,
                None,
                Some(dacl as *const ACL),
                if bool::from(sacl_present) && !sacl.is_null() {
                    Some(sacl as *const ACL)
                } else {
                    None
                },
            )
        };
        if error != ERROR_SUCCESS {
            return Err(map_file_err(
                path,
                std::io::Error::from_raw_os_error(error.0 as i32),
            ));
        }
        Ok(())
    })();
    unsafe {
        let _ = LocalFree(Some(HLOCAL(descriptor.0)));
    }
    result
}

fn security_err(path: &Path, error: windows::core::Error) -> SharedRingError {
    SharedRingError::FileMapping {
        path: path.to_path_buf(),
        message: error.to_string(),
    }
}

struct SecurityPrivilegeGuard {
    token: HANDLE,
    previous: TOKEN_PRIVILEGES,
}

impl Drop for SecurityPrivilegeGuard {
    fn drop(&mut self) {
        unsafe {
            let _ = AdjustTokenPrivileges(
                self.token,
                false,
                Some(&self.previous as *const TOKEN_PRIVILEGES),
                0,
                None,
                None,
            );
            let _ = CloseHandle(self.token);
        }
    }
}

fn enable_security_privilege() -> Option<SecurityPrivilegeGuard> {
    let mut token = HANDLE::default();
    unsafe {
        OpenProcessToken(
            GetCurrentProcess(),
            TOKEN_ADJUST_PRIVILEGES | TOKEN_QUERY,
            &mut token,
        )
        .ok()?;
    }
    let mut luid = LUID::default();
    if unsafe { LookupPrivilegeValueW(PCWSTR::null(), SE_SECURITY_NAME, &mut luid) }.is_err() {
        unsafe {
            let _ = CloseHandle(token);
        }
        return None;
    }
    let privileges = TOKEN_PRIVILEGES {
        PrivilegeCount: 1,
        Privileges: [LUID_AND_ATTRIBUTES {
            Luid: luid,
            Attributes: SE_PRIVILEGE_ENABLED,
        }],
    };
    let mut previous = TOKEN_PRIVILEGES::default();
    let mut previous_len = 0u32;
    if unsafe {
        AdjustTokenPrivileges(
            token,
            false,
            Some(&privileges as *const TOKEN_PRIVILEGES),
            std::mem::size_of::<TOKEN_PRIVILEGES>() as u32,
            Some(&mut previous as *mut TOKEN_PRIVILEGES),
            Some(&mut previous_len),
        )
    }
    .is_err()
    {
        unsafe {
            let _ = CloseHandle(token);
        }
        return None;
    }
    if previous.PrivilegeCount == 0 {
        previous = privileges;
        previous.Privileges[0].Attributes = Default::default();
    }
    Some(SecurityPrivilegeGuard { token, previous })
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
