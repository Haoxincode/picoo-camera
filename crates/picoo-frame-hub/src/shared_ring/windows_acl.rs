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
/// `0x1301BF` is FILE_GENERIC_READ|WRITE|EXECUTE plus DELETE (icacls Modify).
/// SDDL has no `M` rights alias; that string fails ConvertStringSecurityDescriptor.
pub const WINDOWS_SHARED_RING_DIRECTORY_SDDL: &str =
    "D:PAI(A;OICI;FA;;;SY)(A;OICI;FA;;;BA)(A;OICI;FA;;;CO)(A;OICI;0x1301BF;;;LS)(A;OICI;0x1301BF;;;BU)S:P(ML;OICI;NW;;;ME)";

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn sddl_admits_local_service_users_and_medium_integrity() {
        assert!(WINDOWS_SHARED_RING_DIRECTORY_SDDL.contains("0x1301BF;;;LS)"));
        assert!(WINDOWS_SHARED_RING_DIRECTORY_SDDL.contains("0x1301BF;;;BU)"));
        assert!(
            !WINDOWS_SHARED_RING_DIRECTORY_SDDL.contains("M;;;"),
            "icacls Modify alias M is not valid SDDL"
        );
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
