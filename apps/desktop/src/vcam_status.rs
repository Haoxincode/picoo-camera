//! Virtual camera install detection — REQ-PICOO-VCAM-001 / PUC-004.

use crate::model::VirtualCameraStatus;

/// Probe whether Picoo Camera virtual camera appears installed on this machine.
pub fn detect_vcam_status() -> VirtualCameraStatus {
    // REQ-PICOO-UI-010: Linux GPUI is a preview host, not a product Receiver.
    #[cfg(not(all(windows, feature = "windows-vcam")))]
    {
        return VirtualCameraStatus::Unsupported;
    }

    #[cfg(all(windows, feature = "windows-vcam"))]
    if !vcam_dll_present() {
        return VirtualCameraStatus::NotInstalled;
    }

    #[cfg(all(windows, feature = "windows-vcam"))]
    if !crate::vcam_register::com_server_registered() {
        // DLL on disk but COM missing → Start fails with 0x80040154 (REGDB_E_CLASSNOTREG).
        return VirtualCameraStatus::NotInstalled;
    }

    #[cfg(all(windows, feature = "windows-vcam"))]
    if vcam_registry_present() {
        return VirtualCameraStatus::Installed;
    }

    #[cfg(all(windows, feature = "windows-vcam"))]
    {
        VirtualCameraStatus::Installed
    }
}

#[cfg(all(windows, feature = "windows-vcam"))]
fn vcam_dll_present() -> bool {
    for path in candidate_vcam_dll_paths() {
        if path.is_file() {
            return true;
        }
    }
    false
}

#[cfg(all(windows, feature = "windows-vcam"))]
fn candidate_vcam_dll_paths() -> Vec<std::path::PathBuf> {
    crate::vcam_register::candidate_vcam_dll_paths()
}

#[cfg(all(windows, feature = "windows-vcam"))]
fn vcam_registry_present() -> bool {
    use std::process::Command;

    // Best-effort: MF virtual camera registration leaves a friendly name key on Win11.
    let output = Command::new("reg")
        .args([
            "query",
            r"HKLM\SOFTWARE\Microsoft\Windows Media Foundation\Platform",
            "/s",
            "/f",
            "Picoo",
        ])
        .output();
    match output {
        Ok(out) => out.status.success() && !out.stdout.is_empty(),
        Err(_) => false,
    }
}

/// Human-readable repair hint for settings / first launch.
pub fn vcam_repair_hint(status: VirtualCameraStatus) -> &'static str {
    match status {
        VirtualCameraStatus::Installed | VirtualCameraStatus::Active => {
            "虚拟摄像头已注册。若会议软件中不可见，请重启会议应用或重新运行安装程序。"
        }
        VirtualCameraStatus::NotInstalled => {
            "请运行 Windows 安装程序（MSI）以注册 Picoo Camera；若已安装，在设置页点「安装/激活虚拟摄像头」（需管理员）或执行：regsvr32 PicooVirtualCameraSource.dll"
        }
        VirtualCameraStatus::Unknown => "正在检测虚拟摄像头状态…",
        VirtualCameraStatus::Unsupported => {
            "虚拟摄像头仅支持 Windows 11 / macOS。Linux 是 GPUI 预览面，不注册会议软件摄像头。"
        }
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
                | VirtualCameraStatus::Installed
                | VirtualCameraStatus::NotInstalled
                | VirtualCameraStatus::Active
                | VirtualCameraStatus::Unsupported
        ));
    }

    #[cfg(not(all(windows, feature = "windows-vcam")))]
    #[test]
    fn linux_preview_host_reports_unsupported() {
        assert_eq!(detect_vcam_status(), VirtualCameraStatus::Unsupported);
        assert!(vcam_repair_hint(VirtualCameraStatus::Unsupported).contains("Linux"));
    }
}
