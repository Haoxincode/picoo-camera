use super::{
    macos_activation_action_visible, macos_deactivation_action_visible,
    resolve_pending_macos_vcam_status,
};
use crate::model::VirtualCameraStatus;
use crate::prefs::{MacosCameraExtensionIntent, PendingMacosCameraExtension};

#[test]
fn macos_camera_extension_actions_follow_lifecycle_state() {
    for status in [
        VirtualCameraStatus::Unknown,
        VirtualCameraStatus::AwaitingApproval,
        VirtualCameraStatus::RestartRequired,
        VirtualCameraStatus::Uninstalling,
        VirtualCameraStatus::Installed,
        VirtualCameraStatus::NotInstalled,
        VirtualCameraStatus::Active,
    ] {
        assert!(!macos_activation_action_visible(status));
    }
    assert!(macos_activation_action_visible(
        VirtualCameraStatus::Bundled
    ));
    assert!(macos_deactivation_action_visible(
        VirtualCameraStatus::Installed
    ));
    assert!(macos_deactivation_action_visible(
        VirtualCameraStatus::Active
    ));
    assert!(!macos_deactivation_action_visible(
        VirtualCameraStatus::RestartRequired
    ));
    assert!(!macos_deactivation_action_visible(
        VirtualCameraStatus::Uninstalling
    ));
}

#[test]
fn macos_reboot_pending_intent_survives_until_system_state_converges() {
    let activation = PendingMacosCameraExtension {
        intent: MacosCameraExtensionIntent::Activate,
        boot_session: "boot-a".into(),
    };
    let deactivation = PendingMacosCameraExtension {
        intent: MacosCameraExtensionIntent::Deactivate,
        boot_session: "boot-a".into(),
    };
    assert_eq!(
        resolve_pending_macos_vcam_status(
            VirtualCameraStatus::Bundled,
            Some(&activation),
            Some("boot-a")
        ),
        (VirtualCameraStatus::RestartRequired, false, false)
    );
    assert_eq!(
        resolve_pending_macos_vcam_status(
            VirtualCameraStatus::Active,
            Some(&activation),
            Some("boot-a")
        ),
        (VirtualCameraStatus::Active, true, false)
    );
    assert_eq!(
        resolve_pending_macos_vcam_status(
            VirtualCameraStatus::Active,
            Some(&deactivation),
            Some("boot-a")
        ),
        (VirtualCameraStatus::Uninstalling, false, false)
    );
    assert_eq!(
        resolve_pending_macos_vcam_status(
            VirtualCameraStatus::Bundled,
            Some(&deactivation),
            Some("boot-a")
        ),
        (VirtualCameraStatus::Bundled, true, false)
    );
}

#[test]
fn macos_reboot_pending_intent_unlocks_retry_when_system_state_did_not_converge() {
    let activation = PendingMacosCameraExtension {
        intent: MacosCameraExtensionIntent::Activate,
        boot_session: "boot-a".into(),
    };
    assert_eq!(
        resolve_pending_macos_vcam_status(
            VirtualCameraStatus::Bundled,
            Some(&activation),
            Some("boot-b")
        ),
        (VirtualCameraStatus::Bundled, true, true)
    );
}
