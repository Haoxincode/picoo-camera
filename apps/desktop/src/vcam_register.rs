//! Windows virtual camera registration — REQ-PICOO-VCAM-002.

use std::ffi::OsStr;
use std::os::windows::ffi::OsStrExt;
use std::path::{Path, PathBuf};
use std::time::{Duration, Instant};

use windows::core::{GUID, HRESULT, PCWSTR};
use windows::Win32::Foundation::{CloseHandle, ERROR_CANCELLED, ERROR_SUCCESS, WAIT_OBJECT_0};
use windows::Win32::Media::MediaFoundation::{
    IMFActivate, IMFAttributes, IMFVirtualCamera, MFCreateAttributes, MFCreateVirtualCamera,
    MFEnumDeviceSources, MFVirtualCameraAccess_AllUsers, MFVirtualCameraAccess_CurrentUser,
    MFVirtualCameraLifetime_Session, MFVirtualCameraLifetime_System,
    MFVirtualCameraType_SoftwareCameraSource, MF_DEVSOURCE_ATTRIBUTE_FRIENDLY_NAME,
    MF_DEVSOURCE_ATTRIBUTE_SOURCE_TYPE, MF_DEVSOURCE_ATTRIBUTE_SOURCE_TYPE_VIDCAP_GUID,
    MF_DEVSOURCE_ATTRIBUTE_SOURCE_TYPE_VIDCAP_SYMBOLIC_LINK,
};
use windows::Win32::System::Com::CoTaskMemFree;
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

#[path = "vcam_register/host_contract.rs"]
mod host_contract;
#[path = "vcam_register/runtime.rs"]
mod runtime;
pub use host_contract::{verify_camera_absent, verify_installed_host_contract};
use runtime::{ComInit, EnumerationComInit, MfInit, ShellComInit};

/// CLSID for the Rust `PicooVirtualCameraSource.dll` COM server.
pub const PICOO_VCAM_CLSID: GUID = GUID::from_u128(0xa7c4e2f1_8b3d_4c6a_9e5f_1d2c3b4a5e6f);

const FRIENDLY_NAME: &str = "Picoo Camera";
const CLSID_KEY: &str = r"SOFTWARE\Classes\CLSID\{A7C4E2F1-8B3D-4C6A-9E5F-1D2C3B4A5E6F}";
const INPROC_SERVER_KEY: &str =
    r"SOFTWARE\Classes\CLSID\{A7C4E2F1-8B3D-4C6A-9E5F-1D2C3B4A5E6F}\InprocServer32";
const PRODUCT_KEY: &str = r"SOFTWARE\Picoo\PicooCamera";
const SYMBOLIC_LINK_VALUE: &str = "vcam_symbolic_link";
// Microsoft’s own virtual-camera manager allows up to 20 seconds for the
// interactive user's device enumeration. This is a convergence probe, not
// part of the authoritative IMFVirtualCamera::Start transaction.
const ENUMERATION_WAIT_TIMEOUT: Duration = Duration::from_secs(20);
const ENUMERATION_RETRY_INTERVAL: Duration = Duration::from_millis(100);

pub struct VirtualCameraRegistration {
    // Keep the camera first: after `Drop::drop` calls Shutdown, Rust releases
    // fields in declaration order, so IMFVirtualCamera is released before
    // MFShutdown and CoUninitialize tear down the runtimes it depends on.
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
        // Ownership is established by a previously committed symbolic link,
        // not by whether the Windows Installer service account can enumerate
        // the device in its non-interactive session.
        let existed_before_attempt = system_registration_identity_persisted();
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
        // Start() is the documented create/register operation. Device
        // enumeration is asynchronous and session-scoped, so the MSI service
        // must not roll back a successful registration merely because its own
        // MFEnumDeviceSources probe cannot see the interactive user's device.
        Ok(registration)
    }

    /// Remove a system-lifetime virtual camera registration (used by MSI uninstall).
    pub fn remove_system() -> Result<(), String> {
        // A failed/rolled-back registration has no owned device identity. Treat
        // removal as idempotent instead of asking MF to synthesize and remove an
        // identity that Picoo never committed. This is also the forward-upgrade
        // bridge for installers affected by the legacy uninstall crash.
        if read_registry_string(PRODUCT_KEY, Some(SYMBOLIC_LINK_VALUE)).is_none() {
            return Ok(());
        }
        repair_installed_com_server()?;
        let registration = create_virtual_camera_identity(
            MFVirtualCameraLifetime_System,
            MFVirtualCameraAccess_AllUsers,
        )?;
        unsafe {
            registration
                .camera
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

impl Drop for VirtualCameraRegistration {
    fn drop(&mut self) {
        // Microsoft requires Shutdown before releasing IMFVirtualCamera. Keep
        // teardown best-effort so every success and early-error path still
        // releases the COM object before MfInit and ComInit are dropped.
        unsafe {
            let _ = self.camera.Shutdown();
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

/// Whether a successful system registration committed its stable identity.
///
/// This does not claim that the current user can already enumerate the camera;
/// callers must use `registered_camera_symbolic_link` for the Active verdict.
pub fn system_registration_identity_persisted() -> bool {
    read_registry_string(PRODUCT_KEY, Some(SYMBOLIC_LINK_VALUE))
        .is_some_and(|value| !value.is_empty())
}

/// Return the symbolic link only when Media Foundation can enumerate the
/// registered Picoo video-capture device for the current user.
///
/// Windows owns the public device name and appends a localized "Windows
/// Virtual Camera" suffix. The persisted symbolic link is the stable identity;
/// comparing it avoids both localized-name false negatives and same-name
/// devices impersonating Picoo's registration.
pub fn registered_camera_symbolic_link() -> Result<Option<String>, String> {
    let Some(expected_link) = read_registry_string(PRODUCT_KEY, Some(SYMBOLIC_LINK_VALUE)) else {
        return Ok(None);
    };
    let _com = EnumerationComInit::new()?;
    let _mf = MfInit::new()?;
    Ok(enumerate_registered_camera_activation(&expected_link)?.map(|(_, link)| link))
}

/// Enumerate the exact persisted device identity while the caller owns COM and
/// Media Foundation runtime guards.
fn enumerate_registered_camera_activation(
    expected_link: &str,
) -> Result<Option<(IMFActivate, String)>, String> {
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
        let mut matched = None;
        for device in devices.iter_mut() {
            let Some(device) = device.as_ref() else {
                continue;
            };
            let symbolic_link = mf_attribute_string(
                device,
                &MF_DEVSOURCE_ATTRIBUTE_SOURCE_TYPE_VIDCAP_SYMBOLIC_LINK,
            );
            if camera_identity_matches(&expected_link, symbolic_link.as_deref()) {
                let friendly = mf_attribute_string(device, &MF_DEVSOURCE_ATTRIBUTE_FRIENDLY_NAME);
                tracing::debug!(
                    friendly_name = friendly.as_deref().unwrap_or("<missing>"),
                    %expected_link,
                    "matched Picoo Camera Media Foundation identity"
                );
                matched = symbolic_link.map(|link| (device.clone(), link));
                break;
            }
        }
        for device in devices.iter_mut() {
            let _ = device.take();
        }
        CoTaskMemFree(Some(raw_devices.cast()));
        matched
    };
    Ok(result)
}

fn camera_identity_matches(expected_link: &str, actual_link: Option<&str>) -> bool {
    // Windows device-interface paths are case-insensitive. MF may return a
    // canonicalized casing different from the value exposed by Start().
    actual_link.is_some_and(|actual| actual.eq_ignore_ascii_case(expected_link))
}

/// Wait for the software-device registration to propagate into Media
/// Foundation enumeration. `IMFVirtualCamera::Start` can return before the
/// device is visible to a second process, so a single immediate probe is not a
/// valid registration verdict.
pub fn wait_for_registered_camera(timeout: Duration) -> Result<String, String> {
    let deadline = Instant::now() + timeout;
    let mut last_error = None;
    loop {
        match registered_camera_symbolic_link() {
            Ok(Some(symbolic_link)) => return Ok(symbolic_link),
            Ok(None) => {}
            Err(err) => last_error = Some(err),
        }
        if Instant::now() >= deadline {
            break;
        }
        std::thread::sleep(ENUMERATION_RETRY_INTERVAL);
    }

    let detail = last_error.map_or_else(
        || "Windows 尚未把设备发布到 Media Foundation 摄像头列表".to_string(),
        |err| format!("最后一次枚举错误：{err}"),
    );
    Err(format!(
        "虚拟摄像头注册命令已完成，但等待 {} 秒后仍不可枚举；{detail}",
        timeout.as_secs()
    ))
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

    if !com_server_registered() {
        return Err(format!(
            "管理员修复进程结束（退出代码 {exit_code}），但 COM 注册仍不可用；请重新运行 PicooCamera.msi"
        ));
    }
    if exit_code != 0 {
        return Err(format!(
            "管理员修复进程失败（退出代码 {exit_code}）；请重新运行 PicooCamera.msi"
        ));
    }
    if let Err(err) = wait_for_registered_camera(ENUMERATION_WAIT_TIMEOUT) {
        // The elevated command already completed the authoritative Start and
        // persisted its identity. Keep this as a non-fatal publishing state so
        // users can restart a cached meeting app (or Windows) without undoing
        // the valid system registration.
        tracing::warn!(%err, "virtual camera registered but is not yet enumerable for the interactive user");
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
    let mut registration = create_virtual_camera_identity(lifetime, access)?;

    if let Err(err) = unsafe { registration.camera.Start(None) } {
        if remove_on_failure {
            unsafe {
                let _ = registration.camera.Remove();
            }
        }
        return Err(format!("IMFVirtualCamera::Start failed: {err}"));
    }

    let symbolic_link = mf_attribute_string(
        &registration.camera,
        &MF_DEVSOURCE_ATTRIBUTE_SOURCE_TYPE_VIDCAP_SYMBOLIC_LINK,
    )
    .filter(|value| !value.is_empty());
    let Some(symbolic_link) = symbolic_link else {
        if remove_on_failure {
            unsafe {
                let _ = registration.camera.Remove();
            }
        }
        return Err(
            "IMFVirtualCamera::Start 已返回成功，但 Windows 未提供摄像头 symbolic link".to_string(),
        );
    };

    registration._symbolic_link = symbolic_link;
    Ok(registration)
}

fn create_virtual_camera_identity(
    lifetime: windows::Win32::Media::MediaFoundation::MFVirtualCameraLifetime,
    access: windows::Win32::Media::MediaFoundation::MFVirtualCameraAccess,
) -> Result<VirtualCameraRegistration, String> {
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
    // Field declaration order on `VirtualCameraRegistration` is intentional:
    // release the COM camera before MFShutdown and CoUninitialize. Returning a
    // tuple here previously let the maintenance path tear those runtimes down
    // first and then release `IMFVirtualCamera`, which can access-violate during
    // MSI uninstall.
    Ok(VirtualCameraRegistration {
        camera,
        _symbolic_link: String::new(),
        _mf: mf,
        _com: com,
    })
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
    let (pairs, _) = bytes.as_chunks::<2>();
    let wide_path: Vec<u16> = pairs
        .iter()
        .map(|&pair| u16::from_le_bytes(pair))
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

    #[test]
    fn camera_identity_uses_symbolic_link_not_windows_owned_display_name() {
        let expected = r"\\?\swd#vcamdevapi#picoo";
        assert!(camera_identity_matches(expected, Some(expected)));
        assert!(camera_identity_matches(
            expected,
            Some(r"\\?\SWD#VCAMDEVAPI#PICOO")
        ));
        assert!(!camera_identity_matches(
            expected,
            Some(r"\\?\swd#vcamdevapi#another-camera")
        ));
        assert!(!camera_identity_matches(expected, None));
    }
}
