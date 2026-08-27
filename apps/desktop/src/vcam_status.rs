//! Virtual camera install detection — REQ-PICOO-VCAM-001 / PUC-004.

use std::path::PathBuf;

use crate::model::VirtualCameraStatus;

/// Probe whether Picoo Camera virtual camera appears installed on this machine.
pub fn detect_vcam_status() -> VirtualCameraStatus {
    if vcam_dll_present() {
        return VirtualCameraStatus::Installed;
    }

    #[cfg(target_os = "windows")]
    if vcam_registry_present() {
        return VirtualCameraStatus::Installed;
    }

    #[cfg(not(target_os = "windows"))]
    {
        // Linux CI: ring reader validates consumer path; treat as unknown until MF lands.
        return VirtualCameraStatus::Unknown;
    }

    #[cfg(target_os = "windows")]
    VirtualCameraStatus::NotInstalled
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

#[cfg(target_os = "windows")]
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
            "请运行 Windows 安装程序（MSI）以注册 Picoo Camera 虚拟摄像头，或在开发环境中执行 installers/windows/stage.ps1。"
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
