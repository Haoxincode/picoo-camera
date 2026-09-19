//! Reciprocal identity check for the Receiver-side native pipe producer.

use std::os::windows::io::{AsRawHandle, FromRawHandle, OwnedHandle};
use std::path::{Path, PathBuf};

use windows::core::{PCWSTR, PWSTR};
use windows::Win32::Foundation::{HANDLE, HMODULE};
use windows::Win32::System::LibraryLoader::{
    GetModuleFileNameW, GetModuleHandleExW, GET_MODULE_HANDLE_EX_FLAG_FROM_ADDRESS,
    GET_MODULE_HANDLE_EX_FLAG_UNCHANGED_REFCOUNT,
};
use windows::Win32::System::Threading::{
    OpenProcess, QueryFullProcessImageNameW, PROCESS_NAME_WIN32, PROCESS_QUERY_LIMITED_INFORMATION,
};

const DESKTOP_EXECUTABLE: &str = "picoo-desktop.exe";

pub(super) fn is_expected_receiver_process(process_id: u32) -> bool {
    let Some(module_path) = current_module_path() else {
        return false;
    };
    let Some(process_path) = process_image_path(process_id) else {
        return false;
    };
    receiver_path_matches_module(&module_path, &process_path)
}

fn current_module_path() -> Option<PathBuf> {
    let mut module = HMODULE::default();
    let address = PCWSTR(is_expected_receiver_process as *const () as *const u16);
    unsafe {
        GetModuleHandleExW(
            GET_MODULE_HANDLE_EX_FLAG_FROM_ADDRESS | GET_MODULE_HANDLE_EX_FLAG_UNCHANGED_REFCOUNT,
            address,
            &mut module,
        )
        .ok()?;
    }
    let mut path = vec![0u16; 32_768];
    let length = unsafe { GetModuleFileNameW(Some(module), &mut path) } as usize;
    if length == 0 || length >= path.len() {
        return None;
    }
    path.truncate(length);
    Some(PathBuf::from(String::from_utf16(&path).ok()?))
}

fn process_image_path(process_id: u32) -> Option<PathBuf> {
    if process_id == 0 {
        return None;
    }
    let process =
        unsafe { OpenProcess(PROCESS_QUERY_LIMITED_INFORMATION, false, process_id).ok()? };
    let process = unsafe { OwnedHandle::from_raw_handle(process.0 as *mut _) };
    let mut path = vec![0u16; 32_768];
    let mut length = path.len() as u32;
    unsafe {
        QueryFullProcessImageNameW(
            HANDLE(process.as_raw_handle()),
            PROCESS_NAME_WIN32,
            PWSTR(path.as_mut_ptr()),
            &mut length,
        )
        .ok()?;
    }
    path.truncate(length as usize);
    Some(PathBuf::from(String::from_utf16(&path).ok()?))
}

fn receiver_path_matches_module(module_path: &Path, process_path: &Path) -> bool {
    let Some(parent) = module_path.parent() else {
        return false;
    };
    let expected = parent.join(DESKTOP_EXECUTABLE);
    expected
        .to_string_lossy()
        .eq_ignore_ascii_case(&process_path.to_string_lossy())
}

#[cfg(test)]
mod tests {
    use super::receiver_path_matches_module;
    use std::path::Path;

    #[test]
    fn receiver_must_be_the_desktop_sibling_of_the_loaded_source() {
        assert!(receiver_path_matches_module(
            Path::new(r"C:\Program Files\Picoo Camera\PicooVirtualCameraSource.dll"),
            Path::new(r"c:\program files\picoo camera\PICOO-DESKTOP.EXE"),
        ));
        assert!(!receiver_path_matches_module(
            Path::new(r"C:\Program Files\Picoo Camera\PicooVirtualCameraSource.dll"),
            Path::new(r"C:\Users\Public\picoo-desktop.exe"),
        ));
    }
}
