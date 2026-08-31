//! Virtual camera install detection — REQ-PICOO-VCAM-001 / REQ-PICOO-VCAM-006 / PUC-004.

#[cfg(not(target_os = "macos"))]
use std::path::PathBuf;

use crate::model::VirtualCameraStatus;

/// Probe whether Picoo Camera virtual camera appears installed on this machine.
pub fn detect_vcam_status() -> VirtualCameraStatus {
    #[cfg(target_os = "macos")]
    {
        // The CMIO extension ships inside the Host app. Presence means it can be
        // activated by a signed release, not that macOS has approved it already.
        if macos_camera_extension_present() {
            VirtualCameraStatus::Bundled
        } else {
            VirtualCameraStatus::NotInstalled
        }
    }

    #[cfg(not(target_os = "macos"))]
    detect_non_macos_vcam_status()
}

/// Query macOS' SystemExtensions registry without blocking the GPUI thread.
#[cfg(target_os = "macos")]
pub fn query_macos_vcam_status() -> Result<VirtualCameraStatus, String> {
    use crate::macos_system_extension::InstalledState;

    let state = crate::macos_system_extension::query_installed_state()?;
    Ok(match state {
        InstalledState::Active => VirtualCameraStatus::Active,
        InstalledState::Missing if !macos_camera_extension_present() => {
            VirtualCameraStatus::NotInstalled
        }
        InstalledState::Missing | InstalledState::Bundled => VirtualCameraStatus::Bundled,
        InstalledState::AwaitingApproval => VirtualCameraStatus::AwaitingApproval,
        InstalledState::Uninstalling => VirtualCameraStatus::Uninstalling,
    })
}

#[cfg(not(target_os = "macos"))]
fn detect_non_macos_vcam_status() -> VirtualCameraStatus {
    if !vcam_dll_present() {
        return VirtualCameraStatus::NotInstalled;
    }

    #[cfg(all(windows, feature = "windows-vcam"))]
    if !crate::vcam_register::com_server_registered() {
        // DLL on disk but COM missing → Start fails with 0x80040154 (REGDB_E_CLASSNOTREG).
        return VirtualCameraStatus::NotInstalled;
    }

    #[cfg(not(all(windows, feature = "windows-vcam")))]
    {
        // Linux CI: ring reader validates consumer path; treat as unknown until MF lands.
        VirtualCameraStatus::Unknown
    }

    #[cfg(all(windows, feature = "windows-vcam"))]
    match crate::vcam_register::registered_camera_symbolic_link() {
        Ok(Some(symbolic_link)) => {
            tracing::debug!(%symbolic_link, "Picoo Camera is visible to Media Foundation");
            VirtualCameraStatus::Active
        }
        Ok(None) => VirtualCameraStatus::NotInstalled,
        Err(err) => {
            tracing::warn!("Media Foundation camera enumeration failed: {err}");
            VirtualCameraStatus::Unknown
        }
    }
}

#[cfg(target_os = "macos")]
fn macos_camera_extension_present() -> bool {
    const EXTENSION_BUNDLE: &str = "com.haoxincode.picoo-camera.camera-extension.systemextension";

    std::env::current_exe()
        .ok()
        .and_then(|exe| {
            exe.parent()?.parent().map(|contents| {
                contents
                    .join("Library/SystemExtensions")
                    .join(EXTENSION_BUNDLE)
            })
        })
        .is_some_and(|path| path.is_dir())
}

#[cfg(not(target_os = "macos"))]
fn vcam_dll_present() -> bool {
    for path in candidate_vcam_dll_paths() {
        if path.is_file() {
            return true;
        }
    }
    false
}

#[cfg(not(target_os = "macos"))]
fn candidate_vcam_dll_paths() -> Vec<PathBuf> {
    #[cfg(all(windows, feature = "windows-vcam"))]
    {
        return crate::vcam_register::candidate_vcam_dll_paths();
    }
    #[cfg(not(all(windows, feature = "windows-vcam")))]
    {
        let mut paths = Vec::new();
        if let Ok(exe) = std::env::current_exe() {
            if let Some(dir) = exe.parent() {
                paths.push(dir.join("PicooVirtualCameraSource.dll"));
            }
        }
        paths.push(PathBuf::from(
            "extensions/windows-virtual-camera/mf-source/build/PicooVirtualCameraSource.dll",
        ));
        paths
    }
}

/// Human-readable repair hint for settings / first launch.
pub fn vcam_repair_hint(status: VirtualCameraStatus) -> &'static str {
    vcam_repair_hint_for(current_platform(), status)
}

/// Platform-correct label for the explicit setup or repair action.
pub fn vcam_setup_action_label() -> &'static str {
    match current_platform() {
        VcamPlatform::Windows => "安装或修复…",
        VcamPlatform::Macos => "激活 Camera Extension…",
        VcamPlatform::Other => "检查虚拟摄像头",
    }
}

/// Explain why the current platform adapter cannot complete activation yet.
#[cfg(not(any(target_os = "macos", all(windows, feature = "windows-vcam"))))]
pub fn vcam_setup_unavailable_message() -> &'static str {
    match current_platform() {
        VcamPlatform::Macos => {
            "当前应用无法提交 Camera Extension 系统请求。请确认从完整的 Picoo Camera.app 启动，并使用已签名的发布构建。"
        }
        VcamPlatform::Other => "当前平台构建不提供系统虚拟摄像头激活能力。",
        VcamPlatform::Windows => "请重新运行 Windows 虚拟摄像头修复。",
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum VcamPlatform {
    Windows,
    Macos,
    Other,
}

fn current_platform() -> VcamPlatform {
    if cfg!(target_os = "windows") {
        VcamPlatform::Windows
    } else if cfg!(target_os = "macos") {
        VcamPlatform::Macos
    } else {
        VcamPlatform::Other
    }
}

fn vcam_repair_hint_for(platform: VcamPlatform, status: VirtualCameraStatus) -> &'static str {
    match platform {
        VcamPlatform::Windows => match status {
            VirtualCameraStatus::Installed | VirtualCameraStatus::Active => {
                "虚拟摄像头已注册。若会议软件中不可见，请重启会议应用或重新运行安装程序。"
            }
            VirtualCameraStatus::Bundled
            | VirtualCameraStatus::AwaitingApproval
            | VirtualCameraStatus::RestartRequired
            | VirtualCameraStatus::Uninstalling
            | VirtualCameraStatus::NotInstalled => {
                "未检测到 Picoo Camera 系统注册。若已安装，请点下方「安装或修复…」并在 Windows 用户账户控制中允许；若组件缺失，请重新运行 PicooCamera.msi。"
            }
            VirtualCameraStatus::Unknown => {
                "Windows 无法完成摄像头枚举。请点下方「安装或修复…」；若仍失败，请重新运行 PicooCamera.msi。"
            }
        },
        VcamPlatform::Macos => match status {
            VirtualCameraStatus::Bundled => {
                "Camera Extension 已随应用提供。点下方「激活 Camera Extension…」，并按 macOS 提示在系统设置中批准。"
            }
            VirtualCameraStatus::AwaitingApproval => {
                "Camera Extension 正在等待批准。请在系统设置的“登录项与扩展”中允许 Picoo Camera。"
            }
            VirtualCameraStatus::RestartRequired => {
                "Camera Extension 已获批准，将在重新启动 Mac 后生效。"
            }
            VirtualCameraStatus::Uninstalling => {
                "macOS 正在移除 Camera Extension；若系统要求，请重新启动 Mac。"
            }
            VirtualCameraStatus::Installed | VirtualCameraStatus::Active => {
                "Camera Extension 已激活。若会议软件中不可见，请重启会议应用。"
            }
            VirtualCameraStatus::NotInstalled => {
                "当前应用包未包含 Camera Extension，请重新构建或安装完整的 Picoo Camera.app。"
            }
            VirtualCameraStatus::Unknown => "正在检测 Camera Extension 状态…",
        },
        VcamPlatform::Other => match status {
            VirtualCameraStatus::Bundled => "虚拟摄像头组件已随应用提供，但尚未激活。",
            VirtualCameraStatus::AwaitingApproval => "虚拟摄像头组件正在等待系统批准。",
            VirtualCameraStatus::RestartRequired => "虚拟摄像头将在系统重启后生效。",
            VirtualCameraStatus::Uninstalling => "系统正在移除虚拟摄像头组件。",
            VirtualCameraStatus::Installed | VirtualCameraStatus::Active => {
                "虚拟摄像头已就绪。"
            }
            VirtualCameraStatus::NotInstalled => "当前平台构建未提供虚拟摄像头组件。",
            VirtualCameraStatus::Unknown => "当前平台无法检测虚拟摄像头状态。",
        },
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn detect_returns_known_variant() {
        let status = detect_vcam_status();
        assert!(matches!(
            status,
            VirtualCameraStatus::Unknown
                | VirtualCameraStatus::Bundled
                | VirtualCameraStatus::AwaitingApproval
                | VirtualCameraStatus::RestartRequired
                | VirtualCameraStatus::Uninstalling
                | VirtualCameraStatus::Installed
                | VirtualCameraStatus::NotInstalled
                | VirtualCameraStatus::Active
        ));
    }

    #[test]
    fn macos_copy_describes_camera_extension_without_windows_installer_terms() {
        for status in [
            VirtualCameraStatus::Unknown,
            VirtualCameraStatus::Bundled,
            VirtualCameraStatus::AwaitingApproval,
            VirtualCameraStatus::RestartRequired,
            VirtualCameraStatus::Uninstalling,
            VirtualCameraStatus::Installed,
            VirtualCameraStatus::NotInstalled,
            VirtualCameraStatus::Active,
        ] {
            let hint = vcam_repair_hint_for(VcamPlatform::Macos, status);
            assert!(!hint.contains("Windows"));
            assert!(!hint.contains("MSI"));
            assert!(!hint.contains("COM/MF"));
        }
    }

    #[test]
    fn bundled_component_is_not_described_as_already_active() {
        let hint = vcam_repair_hint_for(VcamPlatform::Macos, VirtualCameraStatus::Bundled);
        assert!(hint.contains("已随应用提供"));
        assert!(hint.contains("激活 Camera Extension"));
        assert!(!hint.contains("已激活"));
    }

    #[test]
    fn windows_repair_copy_matches_the_current_virtual_camera_page() {
        let hint = vcam_repair_hint_for(VcamPlatform::Windows, VirtualCameraStatus::NotInstalled);
        assert!(hint.contains("安装或修复"));
        assert!(hint.contains("Windows 用户账户控制"));
        assert!(!hint.contains("设置页"));
    }
}
