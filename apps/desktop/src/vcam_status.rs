//! Virtual camera install detection — REQ-PICOO-VCAM-001 / PUC-004.

use std::path::PathBuf;

use crate::model::VirtualCameraStatus;

/// Probe whether Picoo Camera virtual camera appears installed on this machine.
pub fn detect_vcam_status() -> VirtualCameraStatus {
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

    #[cfg(not(all(windows, feature = "windows-vcam")))]
    {
        // Linux CI: ring reader validates consumer path; treat as unknown until MF lands.
        return VirtualCameraStatus::Unknown;
    }

    #[cfg(all(windows, feature = "windows-vcam"))]
    VirtualCameraStatus::Installed
}

fn vcam_dll_present() -> bool {
    for path in candidate_vcam_dll_paths() {
        if path.is_file() {
            return true;
        }
    }
    false
}

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
            "请运行 Windows 安装程序（MSI）以注册 Picoo Camera；若已安装，在设置页点「安装/激活虚拟摄像头」（需管理员）以修复 COM/MF 注册。"
        }
        VirtualCameraStatus::Unknown => "正在检测虚拟摄像头状态…",
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
        ));
    }
}
