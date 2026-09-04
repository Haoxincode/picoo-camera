use anyhow::{bail, Result};
use std::path::{Path, PathBuf};
use xshell::{cmd, Shell};

use super::{
    apple_bundle_versions, required_env, validate_macos_build_number,
    validate_macos_marketing_version,
};

const IOS_APP_BUNDLE_ID: &str = "com.picoo.camera";

pub(super) fn validate_ios_release_environment() -> Result<()> {
    for name in [
        "PICOO_APPLE_TEAM_ID",
        "PICOO_RELEASE_VERSION",
        "PICOO_RELEASE_BUILD_NUMBER",
        "PICOO_IOS_SIGNING_IDENTITY",
        "PICOO_IOS_PROFILE_PATH",
        "PICOO_IOS_PROFILE_NAME",
    ] {
        required_env(name)?;
    }
    let team_id = required_env("PICOO_APPLE_TEAM_ID")?;
    let marketing_version = required_env("PICOO_RELEASE_VERSION")?;
    let build_number = required_env("PICOO_RELEASE_BUILD_NUMBER")?;
    let identity = required_env("PICOO_IOS_SIGNING_IDENTITY")?;
    validate_ios_release_metadata(&team_id, &marketing_version, &build_number, &identity)?;
    let profile = PathBuf::from(required_env("PICOO_IOS_PROFILE_PATH")?);
    if !profile.is_file() {
        bail!("iOS App Store profile is missing {}", profile.display());
    }
    Ok(())
}

fn validate_ios_release_metadata(
    team_id: &str,
    marketing_version: &str,
    build_number: &str,
    identity: &str,
) -> Result<()> {
    super::macos_team_identifier_prefix_for(team_id)?;
    validate_macos_marketing_version(marketing_version)?;
    validate_macos_build_number(build_number)?;
    if !identity.starts_with("Apple Distribution:") || !identity.ends_with(&format!(" ({team_id})"))
    {
        bail!("PICOO_IOS_SIGNING_IDENTITY must name an Apple Distribution identity for PICOO_APPLE_TEAM_ID");
    }
    Ok(())
}

pub(super) fn archive_sign_and_export_ios(sh: &Shell) -> Result<()> {
    if !cfg!(target_os = "macos") {
        bail!("iOS release must be signed on a macOS host with Xcode");
    }
    validate_ios_release_environment()?;

    let team_id = required_env("PICOO_APPLE_TEAM_ID")?;
    let identity = required_env("PICOO_IOS_SIGNING_IDENTITY")?;
    let identity_fingerprint = signing_identity_fingerprint(sh, &identity, &team_id)?;
    let profile_path = PathBuf::from(required_env("PICOO_IOS_PROFILE_PATH")?);
    let profile_name = required_env("PICOO_IOS_PROFILE_NAME")?;
    let profile =
        validate_distribution_profile(sh, &profile_path, &team_id, &identity_fingerprint)?;
    if profile.name != profile_name {
        bail!(
            "installed iOS profile name `{profile_name}` does not match signed profile `{}`",
            profile.name
        );
    }

    let root = std::env::current_dir()?;
    let apple_dir = root.join("target/apple");
    let xcframework = apple_dir.join("PicooCore.xcframework");
    let project = root.join("apps/ios/PicooCamera.xcodeproj");
    let archive = apple_dir.join("PicooCamera-iOS.xcarchive");
    let export_dir = apple_dir.join("ios-release-export");
    let extracted_dir = apple_dir.join("ios-release-verified");
    let export_options = apple_dir.join("PicooCamera-iOS-ExportOptions.plist");
    for path in [&archive, &export_dir, &extracted_dir] {
        if path.exists() {
            std::fs::remove_dir_all(path)?;
        }
    }
    if !xcframework.is_dir() {
        bail!(
            "iOS release XCFramework is missing {}",
            xcframework.display()
        );
    }

    let (marketing_version, build_number) = apple_bundle_versions()?;
    cmd!(
        sh,
        "xcodebuild archive -project {project} -scheme PicooCamera -configuration Release -sdk iphoneos -destination generic/platform=iOS -archivePath {archive} CODE_SIGN_STYLE=Manual DEVELOPMENT_TEAM={team_id} CODE_SIGN_IDENTITY={identity} PROVISIONING_PROFILE_SPECIFIER={profile_name} MARKETING_VERSION={marketing_version} CURRENT_PROJECT_VERSION={build_number} PICOO_CORE_XCFRAMEWORK_PATH={xcframework}"
    )
    .run()?;

    write_export_options(
        sh,
        &export_options,
        &team_id,
        &identity_fingerprint,
        &profile_name,
    )?;
    std::fs::create_dir_all(&export_dir)?;
    cmd!(
        sh,
        "xcodebuild -exportArchive -archivePath {archive} -exportPath {export_dir} -exportOptionsPlist {export_options}"
    )
    .run()?;

    let mut ipa_files = std::fs::read_dir(&export_dir)?
        .filter_map(|entry| entry.ok().map(|entry| entry.path()))
        .filter(|path| path.extension().and_then(|value| value.to_str()) == Some("ipa"))
        .collect::<Vec<_>>();
    if ipa_files.len() != 1 {
        bail!(
            "iOS export must produce exactly one IPA, found {}",
            ipa_files.len()
        );
    }
    let exported_ipa = ipa_files.pop().expect("exactly one IPA");
    let release_ipa = apple_dir.join("PicooCamera-iOS.ipa");
    if release_ipa.exists() {
        std::fs::remove_file(&release_ipa)?;
    }
    std::fs::rename(&exported_ipa, &release_ipa)?;

    std::fs::create_dir_all(&extracted_dir)?;
    cmd!(sh, "ditto -x -k {release_ipa} {extracted_dir}").run()?;
    let app = extracted_dir.join("Payload/PicooCamera.app");
    validate_exported_app(
        sh,
        &app,
        &team_id,
        &identity_fingerprint,
        &profile.uuid,
        &marketing_version,
        &build_number,
    )?;
    eprintln!("iOS signed App Store IPA: {}", release_ipa.display());
    Ok(())
}

fn write_export_options(
    sh: &Shell,
    path: &Path,
    team_id: &str,
    signing_fingerprint: &str,
    profile_name: &str,
) -> Result<()> {
    let options = export_options_value(team_id, signing_fingerprint, profile_name);
    std::fs::write(path, serde_json::to_vec_pretty(&options)?)?;
    cmd!(sh, "plutil -convert xml1 {path}").run()?;
    Ok(())
}

fn export_options_value(
    team_id: &str,
    signing_fingerprint: &str,
    profile_name: &str,
) -> serde_json::Value {
    let mut provisioning_profiles = serde_json::Map::new();
    provisioning_profiles.insert(
        IOS_APP_BUNDLE_ID.to_owned(),
        serde_json::Value::String(profile_name.to_owned()),
    );
    serde_json::json!({
        "method": "app-store-connect",
        "destination": "export",
        "signingStyle": "manual",
        "teamID": team_id,
        "signingCertificate": signing_fingerprint,
        "manageAppVersionAndBuildNumber": false,
        "provisioningProfiles": provisioning_profiles,
    })
}

struct DistributionProfile {
    name: String,
    uuid: String,
}

fn validate_distribution_profile(
    sh: &Shell,
    profile_path: &Path,
    team_id: &str,
    signing_fingerprint: &str,
) -> Result<DistributionProfile> {
    let decoded = profile_path.with_extension("decoded.plist");
    cmd!(sh, "security cms -D -i {profile_path} -o {decoded}").run()?;
    let json = cmd!(sh, "plutil -convert json -o - {decoded}").read()?;
    let profile: serde_json::Value = serde_json::from_str(&json)?;
    let entitlements = profile
        .get("Entitlements")
        .and_then(|value| value.as_object())
        .ok_or_else(|| anyhow::anyhow!("iOS profile has no Entitlements dictionary"))?;
    let expected_application_id = format!("{team_id}.{IOS_APP_BUNDLE_ID}");
    let team_matches = profile
        .get("TeamIdentifier")
        .and_then(|value| value.as_array())
        .is_some_and(|ids| ids.iter().any(|id| id == team_id));
    let platform_matches = profile
        .get("Platform")
        .and_then(|value| value.as_array())
        .is_some_and(|platforms| platforms.iter().any(|platform| platform == "iOS"));
    let application_matches = entitlements
        .get("application-identifier")
        .or_else(|| entitlements.get("com.apple.application-identifier"))
        .and_then(|value| value.as_str())
        == Some(expected_application_id.as_str());
    let team_entitlement_matches = entitlements
        .get("com.apple.developer.team-identifier")
        .and_then(|value| value.as_str())
        == Some(team_id);
    let distribution_matches = entitlements
        .get("get-task-allow")
        .and_then(|value| value.as_bool())
        == Some(false)
        && profile.get("ProvisionedDevices").is_none()
        && profile
            .get("ProvisionsAllDevices")
            .and_then(|value| value.as_bool())
            != Some(true);
    let expiration_matches = profile_expiration_is_future(sh, &profile)?;
    let certificate_matches =
        profile_authorizes_certificate(sh, &profile, &decoded, signing_fingerprint)?;
    if !team_matches
        || !platform_matches
        || !application_matches
        || !team_entitlement_matches
        || !distribution_matches
        || !expiration_matches
        || !certificate_matches
    {
        bail!(
            "iOS profile is expired, is not App Store distribution, does not authorize the signing certificate, or has mismatched Team/Bundle entitlements"
        );
    }
    let name = required_profile_string(&profile, "Name")?;
    let uuid = required_profile_string(&profile, "UUID")?;
    Ok(DistributionProfile { name, uuid })
}

fn validate_exported_app(
    sh: &Shell,
    app: &Path,
    team_id: &str,
    signing_fingerprint: &str,
    profile_uuid: &str,
    marketing_version: &str,
    build_number: &str,
) -> Result<()> {
    let info_plist = app.join("Info.plist");
    let embedded_profile = app.join("embedded.mobileprovision");
    if !info_plist.is_file() || !embedded_profile.is_file() {
        bail!("exported iOS app is incomplete: {}", app.display());
    }
    cmd!(sh, "codesign --verify --deep --strict --verbose=4 {app}").run()?;
    let details = cmd!(sh, "codesign -dvv --verbose=4 {app}").read_stderr()?;
    if !details
        .lines()
        .any(|line| line == format!("TeamIdentifier={team_id}"))
        || !details
            .lines()
            .any(|line| line.starts_with("Authority=Apple Distribution:"))
    {
        bail!("exported iOS app has the wrong Team ID or signing authority");
    }

    let certificate_prefix = app
        .parent()
        .ok_or_else(|| anyhow::anyhow!("exported app has no parent"))?
        .join("picoo-ios-signing-certificate");
    cmd!(
        sh,
        "codesign -d --extract-certificates {certificate_prefix} {app}"
    )
    .run()?;
    let mut leaf_certificate = certificate_prefix.into_os_string();
    leaf_certificate.push("0");
    let leaf_certificate = PathBuf::from(leaf_certificate);
    let fingerprint = cmd!(
        sh,
        "openssl x509 -inform DER -in {leaf_certificate} -noout -fingerprint -sha1"
    )
    .read()?
    .split_once('=')
    .map(|(_, value)| value.replace(':', "").trim().to_ascii_uppercase())
    .ok_or_else(|| anyhow::anyhow!("could not read exported iOS signing certificate"))?;
    if fingerprint != signing_fingerprint {
        bail!("exported iOS app is not signed by the configured distribution identity");
    }

    let info_json = cmd!(sh, "plutil -convert json -o - {info_plist}").read()?;
    let info: serde_json::Value = serde_json::from_str(&info_json)?;
    let executable_name = info
        .get("CFBundleExecutable")
        .and_then(|value| value.as_str())
        .ok_or_else(|| anyhow::anyhow!("iOS Info.plist has no executable"))?;
    if info
        .get("CFBundleIdentifier")
        .and_then(|value| value.as_str())
        != Some(IOS_APP_BUNDLE_ID)
        || info
            .get("CFBundleShortVersionString")
            .and_then(|value| value.as_str())
            != Some(marketing_version)
        || info.get("CFBundleVersion").and_then(|value| value.as_str()) != Some(build_number)
    {
        bail!("exported iOS app has mismatched bundle ID or version");
    }
    let architectures = cmd!(sh, "lipo -archs {app}/{executable_name}").read()?;
    if architectures.split_whitespace().collect::<Vec<_>>() != ["arm64"] {
        bail!("exported iOS app must contain only the arm64 device slice");
    }

    let entitlements_path = app
        .parent()
        .expect("exported app parent")
        .join("picoo-ios-effective-entitlements.plist");
    let entitlement_xml = cmd!(sh, "codesign -d --entitlements - --xml {app}").read()?;
    std::fs::write(&entitlements_path, entitlement_xml)?;
    let entitlement_json = cmd!(sh, "plutil -convert json -o - {entitlements_path}").read()?;
    let entitlements: serde_json::Value = serde_json::from_str(&entitlement_json)?;
    let expected_application_id = format!("{team_id}.{IOS_APP_BUNDLE_ID}");
    if entitlements
        .get("application-identifier")
        .or_else(|| entitlements.get("com.apple.application-identifier"))
        .and_then(|value| value.as_str())
        != Some(expected_application_id.as_str())
        || entitlements
            .get("com.apple.developer.team-identifier")
            .and_then(|value| value.as_str())
            != Some(team_id)
        || entitlements
            .get("get-task-allow")
            .and_then(|value| value.as_bool())
            != Some(false)
    {
        bail!("exported iOS app has mismatched effective distribution entitlements");
    }

    let embedded_decoded = embedded_profile.with_extension("decoded.plist");
    cmd!(
        sh,
        "security cms -D -i {embedded_profile} -o {embedded_decoded}"
    )
    .run()?;
    let embedded_json = cmd!(sh, "plutil -convert json -o - {embedded_decoded}").read()?;
    let embedded: serde_json::Value = serde_json::from_str(&embedded_json)?;
    if embedded.get("UUID").and_then(|value| value.as_str()) != Some(profile_uuid) {
        bail!("exported iOS app embedded a different provisioning profile");
    }
    Ok(())
}

fn signing_identity_fingerprint(sh: &Shell, identity: &str, team_id: &str) -> Result<String> {
    if !identity.starts_with("Apple Distribution:") || !identity.ends_with(&format!(" ({team_id})"))
    {
        bail!("iOS signing identity does not belong to PICOO_APPLE_TEAM_ID");
    }
    let identities = cmd!(sh, "security find-identity -v -p codesigning").read()?;
    let quoted = format!("\"{identity}\"");
    let matches = identities
        .lines()
        .filter(|line| line.contains(&quoted))
        .filter_map(|line| line.split_whitespace().nth(1))
        .filter(|value| value.len() == 40 && value.bytes().all(|byte| byte.is_ascii_hexdigit()))
        .collect::<Vec<_>>();
    if matches.len() != 1 {
        bail!(
            "configured Apple Distribution identity must resolve to exactly one valid certificate"
        );
    }
    Ok(matches[0].to_ascii_uppercase())
}

fn profile_expiration_is_future(sh: &Shell, profile: &serde_json::Value) -> Result<bool> {
    let expiration = required_profile_string(profile, "ExpirationDate")?;
    let normalized = expiration
        .split_once('.')
        .map(|(prefix, _)| format!("{prefix}Z"))
        .unwrap_or(expiration);
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
        .ok_or_else(|| anyhow::anyhow!("iOS profile has no DeveloperCertificates"))?;
    for (index, certificate) in certificates.iter().enumerate() {
        let encoded = certificate
            .as_str()
            .ok_or_else(|| anyhow::anyhow!("iOS profile certificate is not base64 data"))?;
        let encoded_path = decoded_profile.with_extension(format!("cert-{index}.b64"));
        let der_path = decoded_profile.with_extension(format!("cert-{index}.der"));
        std::fs::write(&encoded_path, encoded)?;
        let result = cmd!(
            sh,
            "openssl base64 -d -A -in {encoded_path} -out {der_path}"
        )
        .run()
        .and_then(|_| {
            cmd!(
                sh,
                "openssl x509 -inform DER -in {der_path} -noout -fingerprint -sha1"
            )
            .read()
        });
        let _ = std::fs::remove_file(&encoded_path);
        let _ = std::fs::remove_file(&der_path);
        let fingerprint = result?
            .split_once('=')
            .map(|(_, value)| value.replace(':', "").trim().to_ascii_uppercase())
            .ok_or_else(|| anyhow::anyhow!("could not read iOS profile certificate"))?;
        if fingerprint == signing_fingerprint {
            return Ok(true);
        }
    }
    Ok(false)
}

fn required_profile_string(profile: &serde_json::Value, key: &str) -> Result<String> {
    profile
        .get(key)
        .and_then(|value| value.as_str())
        .filter(|value| !value.is_empty())
        .map(str::to_owned)
        .ok_or_else(|| anyhow::anyhow!("iOS profile has no {key}"))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn ios_release_metadata_requires_distribution_identity_for_team() {
        assert!(validate_ios_release_metadata(
            "ABCDEFGHIJ",
            "1.2.3",
            "42",
            "Apple Distribution: Picoo Camera (ABCDEFGHIJ)",
        )
        .is_ok());
        assert!(validate_ios_release_metadata(
            "ABCDEFGHIJ",
            "1.2.3",
            "42",
            "Apple Development: Picoo Camera (ABCDEFGHIJ)",
        )
        .is_err());
        assert!(validate_ios_release_metadata(
            "ABCDEFGHIJ",
            "1.2.3",
            "42",
            "Apple Distribution: Picoo Camera (ZZZZZZZZZZ)",
        )
        .is_err());
    }

    #[test]
    fn export_options_bind_the_real_bundle_identifier_to_the_profile() {
        let options = export_options_value("ABCDEFGHIJ", "001122", "Picoo App Store");
        assert_eq!(
            options.pointer("/provisioningProfiles/com.picoo.camera"),
            Some(&serde_json::Value::String("Picoo App Store".into()))
        );
        assert!(options
            .pointer("/provisioningProfiles/IOS_APP_BUNDLE_ID")
            .is_none());
    }
}
