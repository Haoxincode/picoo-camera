//! Windows virtual camera registration — REQ-PICOO-VCAM-002.

use std::ffi::OsStr;
use std::os::windows::ffi::OsStrExt;
use std::path::{Path, PathBuf};

use windows::core::{GUID, PCWSTR};
use windows::Win32::Foundation::ERROR_SUCCESS;
use windows::Win32::Media::MediaFoundation::{
    IMFVirtualCamera, MFCreateVirtualCamera, MFShutdown, MFStartup,
    MFVirtualCameraAccess_CurrentUser, MFVirtualCameraLifetime_Session,
    MFVirtualCameraLifetime_System, MFVirtualCameraType_SoftwareCameraSource, MF_VERSION,
};
use windows::Win32::System::Com::{CoInitializeEx, COINIT_MULTITHREADED};
use windows::Win32::System::Registry::{
    RegCloseKey, RegOpenKeyExW, RegQueryValueExW, HKEY_LOCAL_MACHINE, KEY_READ, REG_SZ,
};

/// CLSID for `PicooVirtualCameraSource.dll` — must match `picoo_vcam_ids.h`.
pub const PICOO_VCAM_CLSID: GUID = GUID::from_u128(0xa7c4e2f1_8b3d_4c6a_9e5f_1d2c3b4a5e6f);

const FRIENDLY_NAME: &str = "Picoo Camera";
const INPROC_SERVER_KEY: &str =
    r"SOFTWARE\Classes\CLSID\{A7C4E2F1-8B3D-4C6A-9E5F-1D2C3B4A5E6F}\InprocServer32";

pub struct VirtualCameraRegistration {
    camera: IMFVirtualCamera,
}

impl VirtualCameraRegistration {
    /// Register and start the Picoo Camera virtual camera for the current user session.
    pub fn register_and_start() -> Result<Self, String> {
        create_virtual_camera(MFVirtualCameraLifetime_Session)
    }

    /// Register a system-lifetime virtual camera (survives process exit; used by MSI install).
    pub fn register_system() -> Result<Self, String> {
        create_virtual_camera(MFVirtualCameraLifetime_System)
    }

    /// Remove a system-lifetime virtual camera registration (used by MSI uninstall).
    pub fn remove_system() -> Result<(), String> {
        let registration = create_virtual_camera(MFVirtualCameraLifetime_System)?;
        registration.remove()
    }

    /// Remove the virtual camera registration from the system.
    pub fn remove(self) -> Result<(), String> {
        unsafe {
            self.camera
                .Remove()
                .map_err(|e| format!("IMFVirtualCamera::Remove failed: {e}"))
        }
    }
}

/// Resolve `PicooVirtualCameraSource.dll` beside the desktop executable (or dev paths).
pub fn resolve_vcam_dll() -> Option<PathBuf> {
    candidate_vcam_dll_paths()
        .into_iter()
        .find(|path| path.is_file())
}

/// Whether HKLM COM registration points at an existing DLL path.
pub fn com_server_registered() -> bool {
    match (resolve_vcam_dll(), read_inproc_server_path()) {
        (Some(expected), Some(registered)) => paths_equivalent(&expected, &registered),
        _ => false,
    }
}

/// Register COM via `regsvr32` when WiX declarative keys are missing or stale (0x80040154).
pub fn ensure_com_server_registered() -> Result<(), String> {
    let dll = resolve_vcam_dll().ok_or_else(|| {
        "PicooVirtualCameraSource.dll not found beside picoo-desktop.exe; reinstall MSI or run from the Windows bundle directory".into()
    })?;

    if com_server_registered() {
        return Ok(());
    }

    register_com_server(&dll)
}

fn create_virtual_camera(
    lifetime: windows::Win32::Media::MediaFoundation::MFVirtualCameraLifetime,
) -> Result<VirtualCameraRegistration, String> {
    ensure_com_server_registered()?;

    let _com = ComInit::new()?;
    let _mf = MfInit::new()?;

    let friendly = wide(FRIENDLY_NAME);
    let clsid = wide(&format!("{{{}}}", guid_string(PICOO_VCAM_CLSID)));

    let camera = unsafe {
        MFCreateVirtualCamera(
            MFVirtualCameraType_SoftwareCameraSource,
            lifetime,
            MFVirtualCameraAccess_CurrentUser,
            PCWSTR(friendly.as_ptr()),
            PCWSTR(clsid.as_ptr()),
            None,
        )
    }
    .map_err(|e| format!("MFCreateVirtualCamera failed: {e}"))?;

    unsafe {
        camera
            .Start(None)
            .map_err(|e| format!("IMFVirtualCamera::Start failed: {e}"))?;
    }

    Ok(VirtualCameraRegistration { camera })
}

pub fn candidate_vcam_dll_paths() -> Vec<PathBuf> {
    let mut paths = Vec::new();
    if let Ok(exe) = std::env::current_exe() {
        if let Some(dir) = exe.parent() {
            paths.push(dir.join("PicooVirtualCameraSource.dll"));
            paths.push(dir.join("extensions").join("PicooVirtualCameraSource.dll"));
        }
    }
    paths.push(PathBuf::from(
        "extensions/windows-virtual-camera/mf-source/build/PicooVirtualCameraSource.dll",
    ));
    paths
}

fn read_inproc_server_path() -> Option<PathBuf> {
    let key_wide = wide(INPROC_SERVER_KEY);
    let mut hkey = Default::default();
    unsafe {
        RegOpenKeyExW(
            HKEY_LOCAL_MACHINE,
            PCWSTR(key_wide.as_ptr()),
            0,
            KEY_READ,
            &mut hkey,
        )
        .ok()?;
    }

    let mut kind = REG_SZ;
    let mut bytes = vec![0u8; 1024];
    let mut size = bytes.len() as u32;
    let query = unsafe {
        RegQueryValueExW(
            hkey,
            PCWSTR::null(),
            None,
            Some(&mut kind),
            Some(bytes.as_mut_ptr()),
            Some(&mut size),
        )
    };
    unsafe {
        let _ = RegCloseKey(hkey);
    }
    if query != ERROR_SUCCESS || kind != REG_SZ || size < 2 {
        return None;
    }
    bytes.truncate(size as usize);
    let wide_path: Vec<u16> = bytes
        .chunks_exact(2)
        .map(|chunk| u16::from_le_bytes([chunk[0], chunk[1]]))
        .take_while(|&c| c != 0)
        .collect();
    if wide_path.is_empty() {
        return None;
    }
    Some(PathBuf::from(String::from_utf16_lossy(&wide_path)))
}

fn paths_equivalent(left: &Path, right: &Path) -> bool {
    if left == right {
        return true;
    }
    match (std::fs::canonicalize(left), std::fs::canonicalize(right)) {
        (Ok(a), Ok(b)) => a == b,
        _ => false,
    }
}

fn register_com_server(dll: &Path) -> Result<(), String> {
    let status = std::process::Command::new("regsvr32")
        .arg("/s")
        .arg(dll)
        .status()
        .map_err(|e| format!("regsvr32 spawn failed: {e}"))?;
    if !status.success() {
        return Err(format!(
            "regsvr32 failed ({status}) for {}; run picoo-desktop as Administrator or reinstall PicooCamera.msi",
            dll.display()
        ));
    }
    if !com_server_registered() {
        return Err(
            "COM registration still missing after regsvr32; run as Administrator or reinstall MSI"
                .into(),
        );
    }
    Ok(())
}

struct ComInit;

impl ComInit {
    fn new() -> Result<Self, String> {
        unsafe {
            CoInitializeEx(None, COINIT_MULTITHREADED)
                .ok()
                .map_err(|e| format!("CoInitializeEx failed: {e}"))?;
        }
        Ok(Self)
    }
}

impl Drop for ComInit {
    fn drop(&mut self) {
        unsafe {
            windows::Win32::System::Com::CoUninitialize();
        }
    }
}

struct MfInit;

impl MfInit {
    fn new() -> Result<Self, String> {
        unsafe {
            MFStartup(MF_VERSION, Default::default())
                .map_err(|e| format!("MFStartup failed: {e}"))?;
        }
        Ok(Self)
    }
}

impl Drop for MfInit {
    fn drop(&mut self) {
        unsafe {
            let _ = MFShutdown();
        }
    }
}

fn wide(text: &str) -> Vec<u16> {
    OsStr::new(text).encode_wide().chain(Some(0)).collect()
}

fn guid_string(guid: GUID) -> String {
    format!(
        "{:08X}-{:04X}-{:04X}-{:02X}{:02X}-{:02X}{:02X}{:02X}{:02X}{:02X}{:02X}",
        guid.data1,
        guid.data2,
        guid.data3,
        guid.data4[0],
        guid.data4[1],
        guid.data4[2],
        guid.data4[3],
        guid.data4[4],
        guid.data4[5],
        guid.data4[6],
        guid.data4[7],
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn clsid_matches_cpp_header() {
        assert_eq!(
            guid_string(PICOO_VCAM_CLSID),
            "A7C4E2F1-8B3D-4C6A-9E5F-1D2C3B4A5E6F"
        );
    }

    #[test]
    fn candidate_paths_include_exe_dir() {
        let paths = candidate_vcam_dll_paths();
        assert!(!paths.is_empty());
    }
}
