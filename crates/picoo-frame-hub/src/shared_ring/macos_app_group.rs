use std::path::PathBuf;

use objc2_foundation::{NSBundle, NSFileManager, NSNumber, NSString};

use super::SharedRingError;

pub const MACOS_APP_GROUP_INFO_KEY: &str = "PicooAppGroupIdentifier";
pub const MACOS_UNSIGNED_BUILD_INFO_KEY: &str = "PicooUnsignedDevelopmentBuild";

pub fn macos_app_group_ring_path(name: &str) -> Result<PathBuf, SharedRingError> {
    if name.is_empty()
        || !name
            .bytes()
            .all(|byte| byte.is_ascii_alphanumeric() || matches!(byte, b'-' | b'_' | b'.'))
    {
        return Err(SharedRingError::AppGroupUnavailable(
            "invalid ring name".into(),
        ));
    }
    // An unsigned development bundle has no valid App Group entitlement.
    // Asking LaunchServices for that synthetic container before GPUI starts
    // can block the app's main thread indefinitely. The embedded unsigned
    // extension cannot be activated anyway, so keep host previews functional
    // in a user-owned fallback directory until a real Team ID is injected.
    if macos_is_unsigned_development_bundle() {
        let home = std::env::var_os("HOME").ok_or_else(|| {
            SharedRingError::AppGroupUnavailable(
                "HOME is unavailable for unsigned macOS fallback".into(),
            )
        })?;
        return Ok(PathBuf::from(home)
            .join("Library")
            .join("Application Support")
            .join("Picoo Camera")
            .join("SharedFrameRing")
            .join(format!("{name}.ring")));
    }
    let identifier = macos_app_group_identifier()?;
    let manager = NSFileManager::defaultManager();
    let group = NSString::from_str(&identifier);
    let container = manager
        .containerURLForSecurityApplicationGroupIdentifier(&group)
        .ok_or_else(|| SharedRingError::AppGroupUnavailable(identifier.clone()))?;
    let path = container.path().ok_or_else(|| {
        SharedRingError::AppGroupUnavailable("container URL has no file path".into())
    })?;
    Ok(PathBuf::from(path.to_string()).join(format!("{name}.ring")))
}

fn macos_is_unsigned_development_bundle() -> bool {
    let key = NSString::from_str(MACOS_UNSIGNED_BUILD_INFO_KEY);
    NSBundle::mainBundle()
        .objectForInfoDictionaryKey(&key)
        .and_then(|value| value.downcast::<NSNumber>().ok())
        .is_some_and(|value| value.as_bool())
}

pub fn macos_app_group_identifier() -> Result<String, SharedRingError> {
    if let Ok(identifier) = std::env::var("PICOO_APP_GROUP_IDENTIFIER") {
        if !identifier.trim().is_empty() {
            return Ok(identifier);
        }
    }

    let key = NSString::from_str(MACOS_APP_GROUP_INFO_KEY);
    let value = NSBundle::mainBundle()
        .objectForInfoDictionaryKey(&key)
        .and_then(|value| value.downcast::<NSString>().ok())
        .map(|value| value.to_string());
    value
        .filter(|identifier| !identifier.is_empty())
        .ok_or_else(|| {
            SharedRingError::AppGroupUnavailable(format!(
                "{MACOS_APP_GROUP_INFO_KEY} is absent from the host app Info.plist"
            ))
        })
}
