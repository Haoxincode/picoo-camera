use anyhow::{bail, Result};
use std::path::{Path, PathBuf};
use xshell::{cmd, Shell};

use super::macos_validate::validate_signed_macos_bundle;
use super::{
    archive_apple_bundle, macos_team_identifier_prefix_for, required_env,
    validate_macos_build_number, validate_macos_marketing_version, MACOS_APP_BUNDLE_ID,
    MACOS_APP_GROUP_ID, MACOS_EXTENSION_BUNDLE_ID, MACOS_EXTENSION_BUNDLE_NAME,
};

pub(crate) fn validate_macos_release_environment() -> Result<()> {
    for name in [
        "PICOO_APPLE_TEAM_ID",
        "PICOO_RELEASE_VERSION",
        "PICOO_RELEASE_BUILD_NUMBER",
        "PICOO_MACOS_SIGNING_IDENTITY",
        "PICOO_NOTARY_KEY_PATH",
        "PICOO_NOTARY_KEY_ID",
        "PICOO_NOTARY_ISSUER_ID",
        "PICOO_MACOS_HOST_PROFILE_PATH",
        "PICOO_MACOS_EXTENSION_PROFILE_PATH",
    ] {
        required_env(name)?;
    }
    let team_id = required_env("PICOO_APPLE_TEAM_ID")?;
    macos_team_identifier_prefix_for(&team_id)?;
    validate_macos_marketing_version(&required_env("PICOO_RELEASE_VERSION")?)?;
    validate_macos_build_number(&required_env("PICOO_RELEASE_BUILD_NUMBER")?)?;
    let identity = required_env("PICOO_MACOS_SIGNING_IDENTITY")?;
    if !identity.starts_with("Developer ID Application:") {
        bail!("PICOO_MACOS_SIGNING_IDENTITY must name a Developer ID Application identity");
    }
    for name in [
        "PICOO_NOTARY_KEY_PATH",
        "PICOO_MACOS_HOST_PROFILE_PATH",
        "PICOO_MACOS_EXTENSION_PROFILE_PATH",
    ] {
        let path = PathBuf::from(required_env(name)?);
        if !path.is_file() {
            bail!("macOS release input is missing {}", path.display());
        }
    }
    Ok(())
}

pub(crate) fn sign_and_notarize_macos(sh: &Shell) -> Result<()> {
    if !cfg!(target_os = "macos") {
        bail!("macOS release must be signed and notarized on a macOS host");
    }

    let team_id = required_env("PICOO_APPLE_TEAM_ID")?;
    macos_team_identifier_prefix_for(&team_id)?;
    let identity = required_env("PICOO_MACOS_SIGNING_IDENTITY")?;
    if !identity.starts_with("Developer ID Application:") {
        bail!("PICOO_MACOS_SIGNING_IDENTITY must name a Developer ID Application identity");
    }
    let identity_fingerprint = validate_macos_signing_identity(sh, &identity, &team_id)?;
    let notary_key = PathBuf::from(required_env("PICOO_NOTARY_KEY_PATH")?);
    let notary_key_id = required_env("PICOO_NOTARY_KEY_ID")?;
    let notary_issuer_id = required_env("PICOO_NOTARY_ISSUER_ID")?;
    let host_profile = PathBuf::from(required_env("PICOO_MACOS_HOST_PROFILE_PATH")?);
    let extension_profile = PathBuf::from(required_env("PICOO_MACOS_EXTENSION_PROFILE_PATH")?);
    for secret_file in [&notary_key, &host_profile, &extension_profile] {
        if !secret_file.is_file() {
            bail!("macOS release input is missing {}", secret_file.display());
        }
    }

    let root = std::env::current_dir()?;
    let apple_dir = root.join("target/apple");
    let app = apple_dir.join("macos/Picoo Camera.app");
    let extension = app
        .join("Contents/Library/SystemExtensions")
        .join(MACOS_EXTENSION_BUNDLE_NAME);
    let host_entitlements = apple_dir.join("PicooCamera-macOS.entitlements");
    let extension_entitlements = apple_dir.join("PicooCameraExtension-macOS.entitlements");
    for input in [
        &app,
        &extension,
        &host_entitlements,
        &extension_entitlements,
    ] {
        if !input.exists() {
            bail!("macOS release package input is missing {}", input.display());
        }
    }

    let app_group = MACOS_APP_GROUP_ID;
    embed_and_validate_profile(
        sh,
        &host_profile,
        &app.join("Contents/embedded.provisionprofile"),
        &host_profile.with_extension("decoded.plist"),
        &team_id,
        &identity_fingerprint,
        MACOS_APP_BUNDLE_ID,
        app_group,
        true,
    )?;
    embed_and_validate_profile(
        sh,
        &extension_profile,
        &extension.join("Contents/embedded.provisionprofile"),
        &extension_profile.with_extension("decoded.plist"),
        &team_id,
        &identity_fingerprint,
        MACOS_EXTENSION_BUNDLE_ID,
        app_group,
        false,
    )?;

    // Sign nested code first. Apple requires the host and System Extension to
    // share the same Team ID; hardened runtime and secure timestamp are part
    // of the notarization contract.
    cmd!(
        sh,
        "codesign --force --timestamp --options runtime --sign {identity_fingerprint} --entitlements {extension_entitlements} {extension}"
    )
    .run()?;
    cmd!(
        sh,
        "codesign --force --timestamp --options runtime --sign {identity_fingerprint} --entitlements {host_entitlements} {app}"
    )
    .run()?;
    validate_signed_macos_bundle(
        sh,
        &extension,
        &team_id,
        MACOS_EXTENSION_BUNDLE_ID,
        false,
        &apple_dir.join("signed-extension-entitlements.plist"),
    )?;
    validate_signed_macos_bundle(
        sh,
        &app,
        &team_id,
        MACOS_APP_BUNDLE_ID,
        true,
        &apple_dir.join("signed-host-entitlements.plist"),
    )?;
    cmd!(sh, "codesign --verify --deep --strict --verbose=4 {app}").run()?;

    let notary_upload = apple_dir.join("PicooCamera-macOS-notary-upload.zip");
    archive_apple_bundle(sh, &app, &notary_upload)?;
    cmd!(
        sh,
        "xcrun notarytool submit {notary_upload} --key {notary_key} --key-id {notary_key_id} --issuer {notary_issuer_id} --wait"
    )
    .run()?;
    cmd!(sh, "xcrun stapler staple {app}").run()?;
    cmd!(sh, "xcrun stapler validate {app}").run()?;
    cmd!(sh, "spctl --assess --type execute --verbose=4 {app}").run()?;

    let release_archive = apple_dir.join("PicooCamera-macOS.zip");
    archive_apple_bundle(sh, &app, &release_archive)?;
    eprintln!("macOS signed and notarized app: {}", app.display());
    eprintln!("macOS release artifact: {}", release_archive.display());
    Ok(())
}

#[allow(clippy::too_many_arguments)]
fn embed_and_validate_profile(
    sh: &Shell,
    source: &Path,
    destination: &Path,
    decoded: &Path,
    team_id: &str,
    signing_fingerprint: &str,
    bundle_id: &str,
    app_group: &str,
    requires_system_extension_install: bool,
) -> Result<()> {
    std::fs::copy(source, destination)?;
    cmd!(sh, "security cms -D -i {source} -o {decoded}").run()?;
    let json = cmd!(sh, "plutil -convert json -o - {decoded}").read()?;
    let profile: serde_json::Value = serde_json::from_str(&json)?;
    let entitlements = profile
        .get("Entitlements")
        .and_then(|value| value.as_object())
        .ok_or_else(|| anyhow::anyhow!("provisioning profile has no Entitlements dictionary"))?;
    let expected_application_identifier = format!("{team_id}.{bundle_id}");
    let team_matches = profile
        .get("TeamIdentifier")
        .and_then(|value| value.as_array())
        .is_some_and(|ids| ids.iter().any(|id| id == team_id));
    let application_matches = entitlements
        .get("com.apple.application-identifier")
        .or_else(|| entitlements.get("application-identifier"))
        .and_then(|value| value.as_str())
        == Some(expected_application_identifier.as_str());
    let entitlement_team_matches = entitlements
        .get("com.apple.developer.team-identifier")
        .and_then(|value| value.as_str())
        == Some(team_id);
    let app_group_matches = entitlements
        .get("com.apple.security.application-groups")
        .and_then(|value| value.as_array())
        .is_some_and(|groups| groups.iter().any(|group| group == app_group));
    let install_matches = !requires_system_extension_install
        || entitlements
            .get("com.apple.developer.system-extension.install")
            .and_then(|value| value.as_bool())
            == Some(true);
    let distribution_matches = profile
        .get("ProvisionsAllDevices")
        .and_then(|value| value.as_bool())
        == Some(true)
        && profile
            .get("Platform")
            .and_then(|value| value.as_array())
            .is_some_and(|platforms| platforms.iter().any(|platform| platform == "OSX"));
    let expiration_matches = profile_expiration_is_future(sh, &profile)?;
    let certificate_matches =
        profile_authorizes_certificate(sh, &profile, decoded, signing_fingerprint)?;
    if !team_matches
        || !application_matches
        || !entitlement_team_matches
        || !app_group_matches
        || !install_matches
        || !distribution_matches
        || !expiration_matches
        || !certificate_matches
    {
        bail!(
            "Developer ID profile for `{bundle_id}` is expired, has the wrong distribution type, does not authorize the signing certificate, or has mismatched Team/Bundle/App Group/System Extension entitlements"
        );
    }
    Ok(())
}

fn validate_macos_signing_identity(sh: &Shell, identity: &str, team_id: &str) -> Result<String> {
    if !identity.ends_with(&format!(" ({team_id})")) {
        bail!("macOS signing identity does not belong to PICOO_APPLE_TEAM_ID");
    }
    let identities = cmd!(sh, "security find-identity -v -p codesigning").read()?;
    let quoted_identity = format!("\"{identity}\"");
    let line = identities
        .lines()
        .find(|line| line.contains(&quoted_identity))
        .ok_or_else(|| anyhow::anyhow!("configured Developer ID identity is not available"))?;
    let fingerprint = line
        .split_whitespace()
        .nth(1)
        .filter(|value| value.len() == 40 && value.bytes().all(|byte| byte.is_ascii_hexdigit()))
        .ok_or_else(|| anyhow::anyhow!("could not read Developer ID certificate fingerprint"))?;
    Ok(fingerprint.to_ascii_uppercase())
}

fn profile_expiration_is_future(sh: &Shell, profile: &serde_json::Value) -> Result<bool> {
    let expiration = profile
        .get("ExpirationDate")
        .and_then(|value| value.as_str())
        .ok_or_else(|| anyhow::anyhow!("provisioning profile has no ExpirationDate"))?;
    let normalized = expiration
        .split_once('.')
        .map(|(prefix, _)| format!("{prefix}Z"))
        .unwrap_or_else(|| expiration.to_owned());
    let epoch = cmd!(sh, "date -j -f %Y-%m-%dT%H:%M:%SZ {normalized} +%s")
        .read()?
        .parse::<u64>()?;
    let now = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)?
        .as_secs();
    Ok(epoch > now)
}

fn profile_authorizes_certificate(
    sh: &Shell,
    profile: &serde_json::Value,
    decoded_profile: &Path,
    signing_fingerprint: &str,
) -> Result<bool> {
    let certificates = profile
        .get("DeveloperCertificates")
        .and_then(|value| value.as_array())
        .ok_or_else(|| anyhow::anyhow!("provisioning profile has no DeveloperCertificates"))?;
    for (index, certificate) in certificates.iter().enumerate() {
        let encoded = certificate
            .as_str()
            .ok_or_else(|| anyhow::anyhow!("profile certificate is not base64 data"))?;
        let encoded_path = decoded_profile.with_extension(format!("cert-{index}.b64"));
        let der_path = decoded_profile.with_extension(format!("cert-{index}.der"));
        std::fs::write(&encoded_path, encoded)?;
        let decode_result = cmd!(
            sh,
            "openssl base64 -d -A -in {encoded_path} -out {der_path}"
        )
        .run();
        let fingerprint_result = decode_result.and_then(|_| {
            cmd!(
                sh,
                "openssl x509 -inform DER -in {der_path} -noout -fingerprint -sha1"
            )
            .read()
        });
        let _ = std::fs::remove_file(&encoded_path);
        let _ = std::fs::remove_file(&der_path);
        let fingerprint = fingerprint_result?
            .split_once('=')
            .map(|(_, value)| value.replace(':', "").to_ascii_uppercase())
            .ok_or_else(|| anyhow::anyhow!("could not read profile certificate fingerprint"))?;
        if fingerprint == signing_fingerprint {
            return Ok(true);
        }
    }
    Ok(false)
}
