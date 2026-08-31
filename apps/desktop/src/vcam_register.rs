//! Windows virtual camera registration — REQ-PICOO-VCAM-002.

use std::ffi::OsStr;
use std::os::windows::ffi::OsStrExt;
use std::path::{Path, PathBuf};

use windows::core::{GUID, HRESULT, PCWSTR};
use windows::Win32::Foundation::{
    CloseHandle, ERROR_CANCELLED, ERROR_SUCCESS, RPC_E_CHANGED_MODE, WAIT_OBJECT_0,
};
use windows::Win32::Media::MediaFoundation::{
    IMFActivate, IMFAttributes, IMFVirtualCamera, MFCreateAttributes, MFCreateVirtualCamera,
    MFEnumDeviceSources, MFShutdown, MFStartup, MFVirtualCameraAccess_AllUsers,
    MFVirtualCameraAccess_CurrentUser, MFVirtualCameraLifetime_Session,
    MFVirtualCameraLifetime_System, MFVirtualCameraType_SoftwareCameraSource,
    MF_DEVSOURCE_ATTRIBUTE_FRIENDLY_NAME, MF_DEVSOURCE_ATTRIBUTE_SOURCE_TYPE,
    MF_DEVSOURCE_ATTRIBUTE_SOURCE_TYPE_VIDCAP_GUID,
    MF_DEVSOURCE_ATTRIBUTE_SOURCE_TYPE_VIDCAP_SYMBOLIC_LINK, MF_VERSION,
};
use windows::Win32::System::Com::{
    CoInitializeEx, CoTaskMemFree, COINIT_APARTMENTTHREADED, COINIT_DISABLE_OLE1DDE,
    COINIT_MULTITHREADED,
};
use windows::Win32::System::Registry::{
    RegCloseKey, RegCreateKeyExW, RegDeleteValueW, RegOpenKeyExW, RegQueryValueExW, RegSetValueExW,
    HKEY_LOCAL_MACHINE, KEY_READ, KEY_WRITE, REG_OPTION_NON_VOLATILE, REG_SZ,
};
use windows::Win32::System::Threading::{GetExitCodeProcess, WaitForSingleObject, INFINITE};
use windows::Win32::UI::Shell::{
    FOLDERID_ProgramFilesX64, SHGetKnownFolderPath, ShellExecuteExW, KF_FLAG_DEFAULT,
    SEE_MASK_NOASYNC, SEE_MASK_NOCLOSEPROCESS, SHELLEXECUTEINFOW,
};
use windows::Win32::UI::WindowsAndMessaging::SW_HIDE;

/// CLSID for the Rust `PicooVirtualCameraSource.dll` COM server.
pub const PICOO_VCAM_CLSID: GUID = GUID::from_u128(0xa7c4e2f1_8b3d_4c6a_9e5f_1d2c3b4a5e6f);

const FRIENDLY_NAME: &str = "Picoo Camera";
const CLSID_KEY: &str = r"SOFTWARE\Classes\CLSID\{A7C4E2F1-8B3D-4C6A-9E5F-1D2C3B4A5E6F}";
const INPROC_SERVER_KEY: &str =
    r"SOFTWARE\Classes\CLSID\{A7C4E2F1-8B3D-4C6A-9E5F-1D2C3B4A5E6F}\InprocServer32";
const PRODUCT_KEY: &str = r"SOFTWARE\Picoo\PicooCamera";
const SYMBOLIC_LINK_VALUE: &str = "vcam_symbolic_link";

pub struct VirtualCameraRegistration {
    camera: IMFVirtualCamera,
    _symbolic_link: String,
    _mf: MfInit,
    _com: ComInit,
}

impl VirtualCameraRegistration {
    /// Register and start the Picoo Camera virtual camera for the current user session.
    pub fn register_and_start() -> Result<Self, String> {
        ensure_com_server_registered()?;
        start_virtual_camera(
            MFVirtualCameraLifetime_Session,
            MFVirtualCameraAccess_CurrentUser,
            true,
        )
    }

    /// Register an all-users, system-lifetime virtual camera (MSI/UAC only).
    pub fn register_system() -> Result<Self, String> {
        repair_installed_com_server()?;
        let existed_before_attempt = registered_camera_symbolic_link()?.is_some();
        let registration = start_virtual_camera(
            MFVirtualCameraLifetime_System,
            MFVirtualCameraAccess_AllUsers,
            !existed_before_attempt,
        )?;
        if let Err(err) = write_registry_string(
            PRODUCT_KEY,
            Some(SYMBOLIC_LINK_VALUE),
            &registration._symbolic_link,
        ) {
            if !existed_before_attempt {
                let _ = registration.remove();
            }
            return Err(format!("虚拟摄像头已创建，但无法保存设备身份：{err}"));
        }
        Ok(registration)
    }

    /// Remove a system-lifetime virtual camera registration (used by MSI uninstall).
    pub fn remove_system() -> Result<(), String> {
        repair_installed_com_server()?;
        let (camera, _mf, _com) = create_virtual_camera_identity(
            MFVirtualCameraLifetime_System,
            MFVirtualCameraAccess_AllUsers,
        )?;
        unsafe {
            camera
                .Remove()
                .map_err(|err| format!("IMFVirtualCamera::Remove failed: {err}"))?;
        }
        // This value only supports detection. WiX owns the declarative COM
        // transaction, so post-Remove metadata cleanup must be best-effort.
        let _ = delete_registry_value(PRODUCT_KEY, SYMBOLIC_LINK_VALUE);
        Ok(())
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

/// Return the symbolic link only when Media Foundation can enumerate the
/// registered Picoo video-capture device for the current user.
pub fn registered_camera_symbolic_link() -> Result<Option<String>, String> {
    let Some(expected_link) = read_registry_string(PRODUCT_KEY, Some(SYMBOLIC_LINK_VALUE)) else {
        return Ok(None);
    };
    let _com = EnumerationComInit::new()?;
    let _mf = MfInit::new()?;
    let mut attributes = None;
    unsafe {
        MFCreateAttributes(&mut attributes, 1)
            .map_err(|err| format!("创建摄像头枚举属性失败：{err}"))?;
    }
    let attributes = attributes.ok_or_else(|| "Media Foundation 未返回枚举属性".to_string())?;
    unsafe {
        attributes
            .SetGUID(
                &MF_DEVSOURCE_ATTRIBUTE_SOURCE_TYPE,
                &MF_DEVSOURCE_ATTRIBUTE_SOURCE_TYPE_VIDCAP_GUID,
            )
            .map_err(|err| format!("配置摄像头枚举属性失败：{err}"))?;
    }

    let mut raw_devices: *mut Option<IMFActivate> = std::ptr::null_mut();
    let mut device_count = 0u32;
    unsafe {
        MFEnumDeviceSources(&attributes, &mut raw_devices, &mut device_count)
            .map_err(|err| format!("枚举 Windows 摄像头失败：{err}"))?;
    }
    if raw_devices.is_null() {
        return Ok(None);
    }

    let result = unsafe {
        let devices = std::slice::from_raw_parts_mut(raw_devices, device_count as usize);
        let mut match_link = None;
        for device in devices.iter_mut() {
            let Some(device) = device.as_ref() else {
                continue;
            };
            let friendly = mf_attribute_string(device, &MF_DEVSOURCE_ATTRIBUTE_FRIENDLY_NAME);
            let symbolic_link = mf_attribute_string(
                device,
                &MF_DEVSOURCE_ATTRIBUTE_SOURCE_TYPE_VIDCAP_SYMBOLIC_LINK,
            );
            if friendly.as_deref() == Some(FRIENDLY_NAME)
                && symbolic_link.as_deref() == Some(expected_link.as_str())
            {
                match_link = symbolic_link;
                break;
            }
        }
        for device in devices.iter_mut() {
            let _ = device.take();
        }
        CoTaskMemFree(Some(raw_devices.cast()));
        match_link
    };
    Ok(result)
}

/// Repair declarative COM registration when the installer keys are missing or stale.
pub fn ensure_com_server_registered() -> Result<(), String> {
    let dll = resolve_vcam_dll().ok_or_else(|| {
        "PicooVirtualCameraSource.dll not found beside picoo-desktop.exe; reinstall MSI or run from the Windows bundle directory".to_string()
    })?;

    if com_server_registered() {
        return Ok(());
    }

    register_com_server(&dll)
}

fn ensure_installed_com_server_registered() -> Result<(), String> {
    let dll = resolve_installed_vcam_dll()?;
    if read_inproc_server_path().is_some_and(|registered| paths_equivalent(&dll, &registered)) {
        return Ok(());
    }
    register_com_server(&dll)
}

fn repair_installed_com_server() -> Result<(), String> {
    let _com = ShellComInit::new()?;
    ensure_installed_com_server_registered()
}

/// Run the existing maintenance command through the Windows UAC boundary.
///
/// This is intentionally called only from an explicit user action. Ordinary app
/// startup remains read-only and never prompts for elevation.
pub fn repair_system_registration_elevated() -> Result<(), String> {
    let _com = ShellComInit::new()?;
    resolve_installed_vcam_dll()?;

    let executable =
        std::env::current_exe().map_err(|err| format!("无法定位 Picoo Camera 程序：{err}"))?;
    let executable = wide_os(executable.as_os_str());
    let verb = wide("runas");
    let parameters = wide("--register-vcam --no-wait");

    let mut execute_info = SHELLEXECUTEINFOW {
        cbSize: std::mem::size_of::<SHELLEXECUTEINFOW>() as u32,
        fMask: SEE_MASK_NOCLOSEPROCESS | SEE_MASK_NOASYNC,
        lpVerb: PCWSTR(verb.as_ptr()),
        lpFile: PCWSTR(executable.as_ptr()),
        lpParameters: PCWSTR(parameters.as_ptr()),
        nShow: SW_HIDE.0,
        ..Default::default()
    };

    if let Err(err) = unsafe { ShellExecuteExW(&mut execute_info) } {
        if err.code() == HRESULT::from_win32(ERROR_CANCELLED.0) {
            return Err("你取消了 Windows 管理员授权；未修改系统注册".to_string());
        }
        return Err(format!("无法请求 Windows 管理员权限：{err}"));
    }

    let process = execute_info.hProcess;
    if process.is_invalid() {
        return Err("Windows 未返回修复进程句柄".to_string());
    }

    let wait = unsafe { WaitForSingleObject(process, INFINITE) };
    if wait != WAIT_OBJECT_0 {
        unsafe {
            let _ = CloseHandle(process);
        }
        return Err(format!("等待虚拟摄像头修复进程失败（{wait:?}）"));
    }

    let mut exit_code = 1u32;
    let exit_result = unsafe { GetExitCodeProcess(process, &mut exit_code) };
    unsafe {
        let _ = CloseHandle(process);
    }
    exit_result.map_err(|err| format!("无法读取虚拟摄像头修复结果：{err}"))?;

    if exit_code != 0 {
        return Err(format!(
            "管理员修复进程失败（退出代码 {exit_code}）；请重新运行 PicooCamera.msi"
        ));
    }
    if !com_server_registered() {
        return Err("修复进程已结束，但 COM 注册仍不可用；请重新运行 PicooCamera.msi".to_string());
    }
    Ok(())
}

fn resolve_installed_vcam_dll() -> Result<PathBuf, String> {
    let executable = std::env::current_exe()
        .and_then(std::fs::canonicalize)
        .map_err(|err| format!("无法定位 Picoo Camera 安装目录：{err}"))?;
    let program_files = program_files_x64()?;
    if !executable.starts_with(&program_files) {
        return Err(
            "当前程序不是从 Windows 的 Picoo Camera 安装目录运行。请先运行 PicooCamera.msi；便携目录不会写入系统 COM 注册。"
                .to_string(),
        );
    }
    let install_dir = executable
        .parent()
        .ok_or_else(|| "Picoo Camera 程序路径没有父目录".to_string())?;
    let dll = install_dir.join("PicooVirtualCameraSource.dll");
    if !dll.is_file() {
        return Err(
            "安装目录缺少 PicooVirtualCameraSource.dll。请重新运行 PicooCamera.msi。".to_string(),
        );
    }
    Ok(dll)
}

fn program_files_x64() -> Result<PathBuf, String> {
    let path = unsafe {
        SHGetKnownFolderPath(&FOLDERID_ProgramFilesX64, KF_FLAG_DEFAULT, None)
            .map_err(|err| format!("无法读取 Windows Program Files 目录：{err}"))?
    };
    let value = unsafe { path.to_string() };
    unsafe {
        CoTaskMemFree(Some(path.as_ptr().cast()));
    }
    let value = value.map_err(|err| format!("Program Files 路径不是有效 Unicode：{err}"))?;
    std::fs::canonicalize(value)
        .map_err(|err| format!("无法解析 Windows Program Files 目录：{err}"))
}

fn start_virtual_camera(
    lifetime: windows::Win32::Media::MediaFoundation::MFVirtualCameraLifetime,
    access: windows::Win32::Media::MediaFoundation::MFVirtualCameraAccess,
    remove_on_failure: bool,
) -> Result<VirtualCameraRegistration, String> {
    let (camera, mf, com) = create_virtual_camera_identity(lifetime, access)?;

    if let Err(err) = unsafe { camera.Start(None) } {
        if remove_on_failure {
            unsafe {
                let _ = camera.Remove();
            }
        }
        return Err(format!("IMFVirtualCamera::Start failed: {err}"));
    }

    let symbolic_link = mf_attribute_string(
        &camera,
        &MF_DEVSOURCE_ATTRIBUTE_SOURCE_TYPE_VIDCAP_SYMBOLIC_LINK,
    )
    .filter(|value| !value.is_empty());
    let Some(symbolic_link) = symbolic_link else {
        if remove_on_failure {
            unsafe {
                let _ = camera.Remove();
            }
        }
        return Err(
            "IMFVirtualCamera::Start 已返回成功，但 Windows 未提供摄像头 symbolic link".to_string(),
        );
    };

    Ok(VirtualCameraRegistration {
        camera,
        _symbolic_link: symbolic_link,
        _mf: mf,
        _com: com,
    })
}

fn create_virtual_camera_identity(
    lifetime: windows::Win32::Media::MediaFoundation::MFVirtualCameraLifetime,
    access: windows::Win32::Media::MediaFoundation::MFVirtualCameraAccess,
) -> Result<(IMFVirtualCamera, MfInit, ComInit), String> {
    let com = ComInit::new()?;
    let mf = MfInit::new()?;
    let friendly = wide(FRIENDLY_NAME);
    let clsid = wide(&format!("{{{}}}", guid_string(PICOO_VCAM_CLSID)));

    let camera = unsafe {
        MFCreateVirtualCamera(
            MFVirtualCameraType_SoftwareCameraSource,
            lifetime,
            access,
            PCWSTR(friendly.as_ptr()),
            PCWSTR(clsid.as_ptr()),
            None,
        )
    }
    .map_err(|e| format!("MFCreateVirtualCamera failed: {e}"))?;
    Ok((camera, mf, com))
}

fn mf_attribute_string(attributes: &IMFAttributes, key: &GUID) -> Option<String> {
    let length = unsafe { attributes.GetStringLength(key).ok()? };
    let mut buffer = vec![0u16; length.saturating_add(1) as usize];
    unsafe { attributes.GetString(key, &mut buffer, None).ok()? };
    buffer.truncate(length as usize);
    String::from_utf16(&buffer).ok()
}

pub fn candidate_vcam_dll_paths() -> Vec<PathBuf> {
    let mut paths = Vec::new();
    if let Ok(exe) = std::env::current_exe() {
        if let Some(dir) = exe.parent() {
            paths.push(dir.join("PicooVirtualCameraSource.dll"));
            paths.push(dir.join("extensions").join("PicooVirtualCameraSource.dll"));
        }
    }
    paths.push(PathBuf::from("target/release/PicooVirtualCameraSource.dll"));
    paths.push(PathBuf::from(
        "target/release/picoo_virtual_camera_source.dll",
    ));
    paths
}

fn read_inproc_server_path() -> Option<PathBuf> {
    read_registry_string(INPROC_SERVER_KEY, None).map(PathBuf::from)
}

fn read_registry_string(key: &str, name: Option<&str>) -> Option<String> {
    let key_wide = wide(key);
    let name_wide = name.map(wide);
    let value_name = name_wide
        .as_ref()
        .map_or_else(PCWSTR::null, |value| PCWSTR(value.as_ptr()));
    let mut hkey = Default::default();
    unsafe {
        if RegOpenKeyExW(
            HKEY_LOCAL_MACHINE,
            PCWSTR(key_wide.as_ptr()),
            None,
            KEY_READ,
            &mut hkey,
        )
        .is_err()
        {
            return None;
        }
    }

    let mut kind = REG_SZ;
    let mut size = 0u32;
    let size_query = unsafe {
        RegQueryValueExW(
            hkey,
            value_name,
            None,
            Some(&mut kind),
            None,
            Some(&mut size),
        )
    };
    if size_query != ERROR_SUCCESS || kind != REG_SZ || size < 2 {
        unsafe {
            let _ = RegCloseKey(hkey);
        }
        return None;
    }
    let mut bytes = vec![0u8; size as usize];
    let query = unsafe {
        RegQueryValueExW(
            hkey,
            value_name,
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
    Some(String::from_utf16_lossy(&wide_path))
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
    write_registry_string(CLSID_KEY, None, FRIENDLY_NAME)?;
    write_registry_string(INPROC_SERVER_KEY, None, &dll.to_string_lossy())?;
    write_registry_string(INPROC_SERVER_KEY, Some("ThreadingModel"), "Both")?;
    if !com_server_registered() {
        return Err(
            "COM registration is still missing; run as Administrator or reinstall PicooCamera.msi"
                .to_string(),
        );
    }
    Ok(())
}

fn delete_registry_value(key: &str, name: &str) -> Result<(), String> {
    let key = wide(key);
    let name = wide(name);
    let mut handle = Default::default();
    let open = unsafe {
        RegOpenKeyExW(
            HKEY_LOCAL_MACHINE,
            PCWSTR(key.as_ptr()),
            None,
            KEY_WRITE,
            &mut handle,
        )
    };
    if open.0 == windows::Win32::Foundation::ERROR_FILE_NOT_FOUND.0 {
        return Ok(());
    }
    if open != ERROR_SUCCESS {
        return Err(format!(
            "failed to open virtual camera identity key ({open:?})"
        ));
    }
    let status = unsafe { RegDeleteValueW(handle, PCWSTR(name.as_ptr())) };
    unsafe {
        let _ = RegCloseKey(handle);
    }
    if status == ERROR_SUCCESS || status.0 == windows::Win32::Foundation::ERROR_FILE_NOT_FOUND.0 {
        Ok(())
    } else {
        Err(format!(
            "failed to remove virtual camera identity ({status:?})"
        ))
    }
}

fn write_registry_string(key: &str, name: Option<&str>, value: &str) -> Result<(), String> {
    let key = wide(key);
    let name = name.map(wide);
    let value = wide(value);
    let bytes = unsafe {
        std::slice::from_raw_parts(
            value.as_ptr().cast::<u8>(),
            value.len() * std::mem::size_of::<u16>(),
        )
    };
    let mut handle = Default::default();
    let create = unsafe {
        RegCreateKeyExW(
            HKEY_LOCAL_MACHINE,
            PCWSTR(key.as_ptr()),
            None,
            PCWSTR::null(),
            REG_OPTION_NON_VOLATILE,
            KEY_WRITE,
            None,
            &mut handle,
            None,
        )
    };
    if create != ERROR_SUCCESS {
        return Err(format!(
            "failed to open COM registry key ({create:?}); run as Administrator"
        ));
    }
    let value_name = name
        .as_ref()
        .map_or_else(PCWSTR::null, |name| PCWSTR(name.as_ptr()));
    let set = unsafe { RegSetValueExW(handle, value_name, None, REG_SZ, Some(bytes)) };
    unsafe {
        let _ = RegCloseKey(handle);
    }
    if set != ERROR_SUCCESS {
        return Err(format!(
            "failed to write COM registry value ({set:?}); run as Administrator"
        ));
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

/// Initializes COM when the caller has no apartment yet, while accepting the
/// GPUI thread's existing STA apartment. Only a successful initialization is
/// paired with `CoUninitialize`.
struct EnumerationComInit {
    initialized_here: bool,
}

impl EnumerationComInit {
    fn new() -> Result<Self, String> {
        match unsafe { CoInitializeEx(None, COINIT_MULTITHREADED) } {
            Ok(()) => Ok(Self {
                initialized_here: true,
            }),
            Err(err) if err.code() == RPC_E_CHANGED_MODE => Ok(Self {
                initialized_here: false,
            }),
            Err(err) => Err(format!("摄像头枚举 COM 初始化失败：{err}")),
        }
    }
}

impl Drop for EnumerationComInit {
    fn drop(&mut self) {
        if self.initialized_here {
            unsafe {
                windows::Win32::System::Com::CoUninitialize();
            }
        }
    }
}

impl Drop for ComInit {
    fn drop(&mut self) {
        unsafe {
            windows::Win32::System::Com::CoUninitialize();
        }
    }
}

struct ShellComInit;

impl ShellComInit {
    fn new() -> Result<Self, String> {
        unsafe {
            CoInitializeEx(None, COINIT_APARTMENTTHREADED | COINIT_DISABLE_OLE1DDE)
                .ok()
                .map_err(|e| format!("Shell COM initialization failed: {e}"))?;
        }
        Ok(Self)
    }
}

impl Drop for ShellComInit {
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
    wide_os(OsStr::new(text))
}

fn wide_os(text: &OsStr) -> Vec<u16> {
    text.encode_wide().chain(Some(0)).collect()
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
    fn clsid_is_stable() {
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
