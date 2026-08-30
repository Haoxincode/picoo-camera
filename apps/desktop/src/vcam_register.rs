//! Windows virtual camera registration — REQ-PICOO-VCAM-002.

use std::ffi::OsStr;
use std::os::windows::ffi::OsStrExt;
use std::path::{Path, PathBuf};
use std::sync::mpsc;
use std::thread;

use windows::core::{GUID, HRESULT, PCWSTR};
use windows::Win32::Foundation::{CloseHandle, ERROR_CANCELLED, ERROR_SUCCESS, WAIT_OBJECT_0};
use windows::Win32::Media::MediaFoundation::{
    IMFVirtualCamera, MFCreateVirtualCamera, MFShutdown, MFStartup,
    MFVirtualCameraAccess_CurrentUser, MFVirtualCameraLifetime_Session,
    MFVirtualCameraLifetime_System, MFVirtualCameraType_SoftwareCameraSource, MF_VERSION,
};
use windows::Win32::System::Com::{
    CoInitializeEx, CoTaskMemFree, COINIT_APARTMENTTHREADED, COINIT_DISABLE_OLE1DDE,
    COINIT_MULTITHREADED,
};
use windows::Win32::System::Registry::{
    RegCloseKey, RegCreateKeyExW, RegDeleteTreeW, RegOpenKeyExW, RegQueryValueExW, RegSetValueExW,
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

pub struct VirtualCameraRegistration {
    camera: IMFVirtualCamera,
    _mf: MfInit,
    _com: ComInit,
}

/// Owns the explicit-repair camera session on the dedicated thread that created it.
pub struct VirtualCameraSessionHost {
    shutdown_tx: mpsc::Sender<()>,
    thread: Option<thread::JoinHandle<()>>,
}

impl Drop for VirtualCameraSessionHost {
    fn drop(&mut self) {
        let _ = self.shutdown_tx.send(());
        // Dropping a JoinHandle detaches it. The worker wakes immediately and
        // releases Camera → MF → COM without ever blocking the GPUI thread.
        let _ = self.thread.take();
    }
}

impl VirtualCameraRegistration {
    /// Register and start the Picoo Camera virtual camera for the current user session.
    pub fn register_and_start() -> Result<Self, String> {
        ensure_com_server_registered()?;
        create_virtual_camera(MFVirtualCameraLifetime_Session)
    }

    /// Register a system-lifetime virtual camera (survives process exit; used by MSI install).
    pub fn register_system() -> Result<Self, String> {
        {
            let _com = ShellComInit::new()?;
            ensure_installed_com_server_registered()?;
        }
        create_virtual_camera(MFVirtualCameraLifetime_System)
    }

    /// Remove a system-lifetime virtual camera registration (used by MSI uninstall).
    pub fn remove_system() -> Result<(), String> {
        {
            let _com = ShellComInit::new()?;
            ensure_installed_com_server_registered()?;
        }
        let registration = create_virtual_camera(MFVirtualCameraLifetime_System)?;
        registration.remove()?;
        unregister_com_server()
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

/// Run repair and MF activation as one dedicated-thread transaction.
///
/// `IMFVirtualCamera` never crosses threads: the returned host only owns a
/// shutdown channel and join handle, while the worker keeps the COM object.
pub fn repair_and_start_elevated() -> Result<VirtualCameraSessionHost, String> {
    start_session_worker(true)
}

/// Start an already-installed camera on a dedicated thread and keep its COM/MF
/// runtime there for the whole desktop session.
pub fn start_registered_on_worker() -> Result<VirtualCameraSessionHost, String> {
    start_session_worker(false)
}

fn start_session_worker(repair_registration: bool) -> Result<VirtualCameraSessionHost, String> {
    let (ready_tx, ready_rx) = mpsc::sync_channel(1);
    let (shutdown_tx, shutdown_rx) = mpsc::channel();
    let thread = thread::Builder::new()
        .name("picoo-vcam-session".into())
        .spawn(move || {
            let registration_result = if repair_registration {
                repair_system_registration_elevated()
            } else {
                Ok(())
            };
            match registration_result.and_then(|()| create_registered_session()) {
                Ok(registration) => {
                    if ready_tx.send(Ok(())).is_ok() {
                        let _ = shutdown_rx.recv();
                    }
                    drop(registration);
                }
                Err(err) => {
                    let _ = ready_tx.send(Err(err));
                }
            }
        })
        .map_err(|err| format!("无法启动虚拟摄像头修复线程：{err}"))?;

    match ready_rx.recv() {
        Ok(Ok(())) => Ok(VirtualCameraSessionHost {
            shutdown_tx,
            thread: Some(thread),
        }),
        Ok(Err(err)) => {
            let _ = thread.join();
            Err(err)
        }
        Err(_) => {
            let panicked = thread.join().is_err();
            if panicked {
                Err("虚拟摄像头修复线程异常退出".to_string())
            } else {
                Err("虚拟摄像头修复线程未返回结果".to_string())
            }
        }
    }
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

fn create_virtual_camera(
    lifetime: windows::Win32::Media::MediaFoundation::MFVirtualCameraLifetime,
) -> Result<VirtualCameraRegistration, String> {
    let com = ComInit::new()?;
    let mf = MfInit::new()?;
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

    Ok(VirtualCameraRegistration {
        camera,
        _mf: mf,
        _com: com,
    })
}

fn create_registered_session() -> Result<VirtualCameraRegistration, String> {
    if !com_server_registered() {
        return Err(
            "Picoo Camera COM registration is missing or stale after elevated repair".to_string(),
        );
    }
    create_virtual_camera(MFVirtualCameraLifetime_Session)
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
    let key_wide = wide(INPROC_SERVER_KEY);
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

fn unregister_com_server() -> Result<(), String> {
    let key = wide(CLSID_KEY);
    let status = unsafe { RegDeleteTreeW(HKEY_LOCAL_MACHINE, PCWSTR(key.as_ptr())) };
    if status == ERROR_SUCCESS || status.0 == windows::Win32::Foundation::ERROR_FILE_NOT_FOUND.0 {
        Ok(())
    } else {
        Err(format!(
            "failed to remove COM registration ({status:?}); run as Administrator"
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
