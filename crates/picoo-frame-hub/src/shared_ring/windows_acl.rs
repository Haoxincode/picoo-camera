//! Installer and elevated-repair ACL for the Windows production shared ring.

/// Per-machine directory under `%ProgramData%` for Receiver ↔ Frame Server files.
pub const WINDOWS_SHARED_RING_DIRECTORY: &str = "Picoo Camera";

/// Protected DACL plus a Medium integrity label. PicooCamera.msi and the
/// elevated `--register-vcam` repair path apply this exact SDDL.
///
/// SYSTEM-created ProgramData directories default to High integrity. A Medium
/// interactive Receiver then gets `ERROR_ACCESS_DENIED` (5) on the ring file
/// even when Builtin Users is present in the DACL. The SACL `ML;...;ME` label
/// keeps the directory at Medium so Users and Local Service can map it.
/// Modify (`M`) includes DELETE so a later generation can replace a stale file.
pub const WINDOWS_SHARED_RING_DIRECTORY_SDDL: &str =
    "D:PAI(A;OICI;FA;;;SY)(A;OICI;FA;;;BA)(A;OICI;FA;;;CO)(A;OICI;M;;;LS)(A;OICI;M;;;BU)S:P(ML;OICI;NW;;;ME)";

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn sddl_admits_local_service_users_and_medium_integrity() {
        assert!(WINDOWS_SHARED_RING_DIRECTORY_SDDL.contains("M;;;LS)"));
        assert!(WINDOWS_SHARED_RING_DIRECTORY_SDDL.contains("M;;;BU)"));
        assert!(WINDOWS_SHARED_RING_DIRECTORY_SDDL.contains("S:P(ML;OICI;NW;;;ME)"));
    }

    #[test]
    fn installer_permission_ex_uses_the_same_sddl() {
        let wxs = include_str!("../../../../installers/windows/picoo-camera.wxs");
        assert!(
            wxs.contains(WINDOWS_SHARED_RING_DIRECTORY_SDDL),
            "picoo-camera.wxs SharedRingDirectoryAcl must stay identical to WINDOWS_SHARED_RING_DIRECTORY_SDDL"
        );
    }
}
