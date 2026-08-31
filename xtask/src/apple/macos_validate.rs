use anyhow::{bail, Result};
use std::path::Path;
use xshell::{cmd, Shell};

use super::{
    macos_extension_app_group, MACOS_APPLICATION_IDENTIFIER_PLACEHOLDER, MACOS_APP_BUNDLE_ID,
    MACOS_APP_GROUP_ID, MACOS_APP_GROUP_PLACEHOLDER, MACOS_EXTENSION_BUNDLE_ID,
    MACOS_EXTENSION_BUNDLE_NAME, MACOS_TEAM_IDENTIFIER_PLACEHOLDER,
};

pub(crate) fn validate_signed_macos_bundle(
    sh: &Shell,
    bundle: &Path,
    team_id: &str,
    bundle_id: &str,
    requires_system_extension_install: bool,
    extracted_entitlements: &Path,
) -> Result<()> {
    let details = cmd!(sh, "codesign -dvv --verbose=4 {bundle}").read_stderr()?;
    if !details
        .lines()
        .any(|line| line == format!("TeamIdentifier={team_id}"))
        || !details
            .lines()
            .any(|line| line.starts_with("Authority=Developer ID Application:"))
    {
        bail!("signed `{bundle_id}` has the wrong Team ID or signing authority");
    }

    let entitlement_xml = cmd!(sh, "codesign -d --entitlements - --xml {bundle}").read()?;
    std::fs::write(extracted_entitlements, entitlement_xml)?;
    let entitlement_json = cmd!(sh, "plutil -convert json -o - {extracted_entitlements}").read()?;
    let entitlements: serde_json::Value = serde_json::from_str(&entitlement_json)?;
    let application_identifier = format!("{team_id}.{bundle_id}");
    let team_matches = entitlements
        .get("com.apple.developer.team-identifier")
        .and_then(|value| value.as_str())
        == Some(team_id);
    let application_matches = entitlements
        .get("com.apple.application-identifier")
        .or_else(|| entitlements.get("application-identifier"))
        .and_then(|value| value.as_str())
        == Some(application_identifier.as_str());
    let app_group_matches = entitlements
        .get("com.apple.security.application-groups")
        .and_then(|value| value.as_array())
        .is_some_and(|groups| groups.iter().any(|group| group == MACOS_APP_GROUP_ID));
    let install_matches = !requires_system_extension_install
        || entitlements
            .get("com.apple.developer.system-extension.install")
            .and_then(|value| value.as_bool())
            == Some(true);
    if !team_matches || !application_matches || !app_group_matches || !install_matches {
        bail!("signed `{bundle_id}` has mismatched effective entitlements");
    }
    Ok(())
}

pub(crate) fn validate_macos_host_app(
    sh: &Shell,
    app: &Path,
    entitlements: &Path,
    extension_entitlements: &Path,
    expected_unsigned_build: bool,
) -> Result<()> {
    let info_plist = app.join("Contents/Info.plist");
    let executable = app.join("Contents/MacOS/picoo-desktop");
    let app_icon = app.join("Contents/Resources/PicooCamera.icns");
    let embedded_extension = app
        .join("Contents/Library/SystemExtensions")
        .join(MACOS_EXTENSION_BUNDLE_NAME);
    if !info_plist.is_file()
        || !executable.is_file()
        || !app_icon.is_file()
        || !embedded_extension.is_dir()
    {
        bail!("incomplete macOS Host app bundle: {}", app.display());
    }

    let plist_json = cmd!(sh, "plutil -convert json -o - {info_plist}").read()?;
    let plist: serde_json::Value = serde_json::from_str(&plist_json)?;
    let host_group = plist
        .get("PicooAppGroupIdentifier")
        .and_then(|value| value.as_str());
    let host_marketing_version = plist
        .get("CFBundleShortVersionString")
        .and_then(|value| value.as_str());
    let host_build_number = plist
        .get("CFBundleVersion")
        .and_then(|value| value.as_str());
    if plist
        .get("CFBundleIdentifier")
        .and_then(|value| value.as_str())
        != Some(MACOS_APP_BUNDLE_ID)
        || plist
            .get("CFBundlePackageType")
            .and_then(|value| value.as_str())
            != Some("APPL")
        || plist
            .get("CFBundleExecutable")
            .and_then(|value| value.as_str())
            != Some("picoo-desktop")
        || plist
            .get("CFBundleIconFile")
            .and_then(|value| value.as_str())
            != Some("PicooCamera.icns")
        || plist
            .get("LSMinimumSystemVersion")
            .and_then(|value| value.as_str())
            != Some("15.0")
        || plist
            .get("NSSystemExtensionUsageDescription")
            .and_then(|value| value.as_str())
            .is_none_or(str::is_empty)
        || plist
            .get("NSLocalNetworkUsageDescription")
            .and_then(|value| value.as_str())
            .is_none_or(str::is_empty)
        || !plist
            .get("NSBonjourServices")
            .and_then(|value| value.as_array())
            .is_some_and(|services| services.iter().any(|service| service == "_picoocam._udp"))
        || plist
            .get("PicooUnsignedDevelopmentBuild")
            .and_then(|value| value.as_bool())
            != Some(expected_unsigned_build)
    {
        bail!("macOS Host Info.plist is missing its product or System Extension identity");
    }

    let extension_group = macos_extension_app_group(sh, &embedded_extension)?;
    if host_group != Some(extension_group.as_str()) {
        bail!("macOS Host and Camera Extension must use the same App Group");
    }

    let extension_info = embedded_extension.join("Contents/Info.plist");
    let extension_json = cmd!(sh, "plutil -convert json -o - {extension_info}").read()?;
    let extension_plist: serde_json::Value = serde_json::from_str(&extension_json)?;
    if extension_plist
        .get("CFBundleIdentifier")
        .and_then(|value| value.as_str())
        != Some(MACOS_EXTENSION_BUNDLE_ID)
    {
        bail!("embedded macOS Camera Extension has the wrong bundle identifier");
    }
    if extension_plist
        .get("CFBundleShortVersionString")
        .and_then(|value| value.as_str())
        != host_marketing_version
        || extension_plist
            .get("CFBundleVersion")
            .and_then(|value| value.as_str())
            != host_build_number
        || host_marketing_version.is_none_or(str::is_empty)
        || host_build_number.is_none_or(str::is_empty)
    {
        bail!("macOS Host and Camera Extension must use the same non-empty bundle versions");
    }

    let entitlements_json = cmd!(sh, "plutil -convert json -o - {entitlements}").read()?;
    let entitlements: serde_json::Value = serde_json::from_str(&entitlements_json)?;
    let expected_group = extension_group;
    if entitlements
        .get("com.apple.security.app-sandbox")
        .and_then(|value| value.as_bool())
        != Some(true)
        || entitlements
            .get("com.apple.security.network.client")
            .and_then(|value| value.as_bool())
            != Some(true)
        || entitlements
            .get("com.apple.security.network.server")
            .and_then(|value| value.as_bool())
            != Some(true)
        || entitlements
            .get("com.apple.developer.system-extension.install")
            .and_then(|value| value.as_bool())
            != Some(true)
        || !entitlements
            .get("com.apple.security.application-groups")
            .and_then(|value| value.as_array())
            .is_some_and(|groups| groups.iter().any(|group| group == &expected_group))
    {
        bail!(
            "macOS Host signing input is missing sandbox, network, System Extension, or App Group capability"
        );
    }

    let extension_entitlements_json =
        cmd!(sh, "plutil -convert json -o - {extension_entitlements}").read()?;
    let extension_entitlements: serde_json::Value =
        serde_json::from_str(&extension_entitlements_json)?;
    if !extension_entitlements
        .get("com.apple.security.application-groups")
        .and_then(|value| value.as_array())
        .is_some_and(|groups| groups.iter().any(|group| group == MACOS_APP_GROUP_ID))
        || extension_entitlements
            .get("com.apple.developer.team-identifier")
            .and_then(|value| value.as_str())
            .is_none_or(str::is_empty)
        || !extension_entitlements
            .get("com.apple.application-identifier")
            .and_then(|value| value.as_str())
            .is_some_and(|value| value.ends_with(MACOS_EXTENSION_BUNDLE_ID))
    {
        bail!("macOS Extension signing input has mismatched effective identities");
    }

    let binary = cmd!(sh, "file {executable}").read()?;
    if !binary.contains("arm64") || binary.contains("x86_64") {
        bail!("macOS Host app must be ARM64-only: {binary}");
    }
    validate_macos_camera_extension(sh, &embedded_extension)
}

pub(crate) fn validate_macos_camera_extension(sh: &Shell, extension: &Path) -> Result<()> {
    let info_plist = extension.join("Contents/Info.plist");
    if !info_plist.is_file() {
        bail!(
            "incomplete macOS Camera Extension bundle: {}",
            extension.display()
        );
    }

    let plist_json = cmd!(sh, "plutil -convert json -o - {info_plist}").read()?;
    let plist: serde_json::Value = serde_json::from_str(&plist_json)?;
    let executable_name = plist
        .get("CFBundleExecutable")
        .and_then(|value| value.as_str())
        .filter(|value| !value.is_empty())
        .ok_or_else(|| anyhow::anyhow!("macOS Camera Extension has no executable identity"))?;
    let executable = extension.join("Contents/MacOS").join(executable_name);
    if !executable.is_file() {
        bail!("macOS Camera Extension executable is missing");
    }
    let app_group = plist
        .get("PicooAppGroupIdentifier")
        .and_then(|value| value.as_str())
        .filter(|value| !value.is_empty());
    let mach_service = plist
        .pointer("/CMIOExtension/CMIOExtensionMachServiceName")
        .and_then(|value| value.as_str())
        .filter(|value| !value.is_empty());
    if plist
        .get("CFBundleIdentifier")
        .and_then(|value| value.as_str())
        != Some(MACOS_EXTENSION_BUNDLE_ID)
        || extension.file_name().and_then(|value| value.to_str())
            != Some(MACOS_EXTENSION_BUNDLE_NAME)
        || plist
            .get("CFBundleDisplayName")
            .and_then(|value| value.as_str())
            != Some("Picoo Camera")
        || plist
            .get("CFBundlePackageType")
            .and_then(|value| value.as_str())
            != Some("SYSX")
        || plist
            .get("NSSystemExtensionUsageDescription")
            .and_then(|value| value.as_str())
            .is_none_or(str::is_empty)
        || app_group.is_none()
        || mach_service.is_none()
    {
        bail!("macOS Camera Extension Info.plist is missing the CMIO product identity");
    }
    if app_group != Some(MACOS_APP_GROUP_ID)
        || !mach_service.is_some_and(|service| {
            service.ends_with(MACOS_EXTENSION_BUNDLE_ID)
                && service.len() > MACOS_EXTENSION_BUNDLE_ID.len()
        })
    {
        bail!("macOS Camera Extension has an invalid App Group or Mach service identity");
    }

    let entitlements =
        Path::new("extensions/macos-camera-extension/PicooCameraExtension.entitlements");
    let entitlements_json = cmd!(sh, "plutil -convert json -o - {entitlements}").read()?;
    let entitlements: serde_json::Value = serde_json::from_str(&entitlements_json)?;
    let expected_group = MACOS_APP_GROUP_PLACEHOLDER;
    if !entitlements
        .get("com.apple.security.application-groups")
        .and_then(|value| value.as_array())
        .is_some_and(|groups| groups.iter().any(|group| group == expected_group))
    {
        bail!("macOS Camera Extension entitlement is missing `{expected_group}`");
    }
    for placeholder in [
        MACOS_TEAM_IDENTIFIER_PLACEHOLDER,
        MACOS_APPLICATION_IDENTIFIER_PLACEHOLDER,
    ] {
        if !entitlements.to_string().contains(placeholder) {
            bail!("macOS Camera Extension entitlement is missing `{placeholder}`");
        }
    }

    let binary = cmd!(sh, "file {executable}").read()?;
    if !binary.contains("arm64") || binary.contains("x86_64") {
        bail!("macOS Camera Extension must be ARM64-only: {binary}");
    }

    let linked = cmd!(sh, "otool -L {executable}").read()?;
    for forbidden in ["Network.framework", "VideoToolbox.framework", "libpicoo"] {
        if linked.contains(forbidden) {
            bail!("macOS Camera Extension links forbidden product dependency `{forbidden}`");
        }
    }
    Ok(())
}
