use anyhow::{bail, Result};
use std::path::{Path, PathBuf};
use xshell::{cmd, Shell};

pub(crate) mod ios;
mod ios_sign;
pub(crate) mod macos;
pub(crate) mod macos_sign;
pub(crate) mod macos_validate;

pub(crate) const MACOS_APP_BUNDLE_ID: &str = "com.haoxincode.picoo-camera";
pub(crate) const MACOS_EXTENSION_BUNDLE_ID: &str = "com.haoxincode.picoo-camera.camera-extension";
pub(crate) const MACOS_EXTENSION_BUNDLE_NAME: &str =
    "com.haoxincode.picoo-camera.camera-extension.systemextension";
pub(crate) const MACOS_APP_GROUP_ID: &str = "group.com.haoxincode.picoo-camera";
pub(crate) const MACOS_APP_GROUP_PLACEHOLDER: &str = "@PICOO_APP_GROUP_IDENTIFIER@";
pub(crate) const MACOS_TEAM_IDENTIFIER_PLACEHOLDER: &str = "@PICOO_TEAM_IDENTIFIER@";
pub(crate) const MACOS_APPLICATION_IDENTIFIER_PLACEHOLDER: &str = "@PICOO_APPLICATION_IDENTIFIER@";
const MACOS_MARKETING_VERSION_PLACEHOLDER: &str = "@PICOO_MARKETING_VERSION@";
const MACOS_BUILD_NUMBER_PLACEHOLDER: &str = "@PICOO_BUILD_NUMBER@";
const MACOS_UNSIGNED_BUILD_PLACEHOLDER: &str = "@PICOO_UNSIGNED_DEVELOPMENT_BUILD@";
const MACOS_UNSIGNED_TEAM_PREFIX: &str = "UNSIGNED.";

pub(crate) fn required_env(name: &str) -> Result<String> {
    std::env::var(name)
        .ok()
        .filter(|value| !value.trim().is_empty())
        .ok_or_else(|| anyhow::anyhow!("{name} is required"))
}

pub(crate) fn render_macos_host_info(
    template: &str,
    app_group: &str,
    marketing_version: &str,
    build_number: &str,
    unsigned_build: bool,
) -> Result<String> {
    validate_macos_marketing_version(marketing_version)?;
    validate_macos_build_number(build_number)?;
    let rendered = render_macos_app_group_template(template, app_group, "Host Info.plist")?;
    let rendered = render_exact_placeholder(
        &render_exact_placeholder(
            &rendered,
            MACOS_MARKETING_VERSION_PLACEHOLDER,
            marketing_version,
            "Host marketing version",
        )?,
        MACOS_BUILD_NUMBER_PLACEHOLDER,
        build_number,
        "Host build number",
    )?;
    render_exact_placeholder(
        &rendered,
        MACOS_UNSIGNED_BUILD_PLACEHOLDER,
        if unsigned_build {
            "<true/>"
        } else {
            "<false/>"
        },
        "Host unsigned development marker",
    )
}

pub(crate) fn render_macos_entitlements(
    template: &str,
    app_group: &str,
    team_id: &str,
    team_prefix: &str,
    bundle_id: &str,
    template_name: &str,
) -> Result<String> {
    let rendered = render_macos_app_group_template(template, app_group, template_name)?;
    let rendered = render_exact_placeholder(
        &rendered,
        MACOS_TEAM_IDENTIFIER_PLACEHOLDER,
        team_id,
        template_name,
    )?;
    render_exact_placeholder(
        &rendered,
        MACOS_APPLICATION_IDENTIFIER_PLACEHOLDER,
        &format!("{team_prefix}{bundle_id}"),
        template_name,
    )
}

fn render_exact_placeholder(
    template: &str,
    placeholder: &str,
    value: &str,
    name: &str,
) -> Result<String> {
    if template.matches(placeholder).count() != 1 {
        bail!("macOS {name} template must contain exactly one {placeholder} token");
    }
    Ok(template.replace(placeholder, value))
}

pub(crate) fn apple_bundle_versions() -> Result<(String, String)> {
    let marketing_version =
        std::env::var("PICOO_RELEASE_VERSION").unwrap_or_else(|_| env!("CARGO_PKG_VERSION").into());
    let build_number = std::env::var("PICOO_RELEASE_BUILD_NUMBER")
        .or_else(|_| std::env::var("PICOO_BUILD_NUMBER"))
        .unwrap_or_else(|_| "2".into());
    validate_macos_marketing_version(&marketing_version)?;
    validate_macos_build_number(&build_number)?;
    Ok((marketing_version, build_number))
}

pub(crate) fn validate_macos_marketing_version(version: &str) -> Result<()> {
    let components = version.split('.').collect::<Vec<_>>();
    if components.is_empty()
        || components.len() > 3
        || components.iter().any(|component| {
            component.is_empty() || !component.bytes().all(|byte| byte.is_ascii_digit())
        })
    {
        bail!("PICOO_RELEASE_VERSION must contain one to three dot-separated integers");
    }
    Ok(())
}

pub(crate) fn validate_macos_build_number(version: &str) -> Result<()> {
    if version.is_empty()
        || !version.bytes().all(|byte| byte.is_ascii_digit())
        || version
            .parse::<u64>()
            .ok()
            .filter(|value| *value > 0)
            .is_none()
    {
        bail!("PICOO_RELEASE_BUILD_NUMBER must be a positive integer");
    }
    Ok(())
}

fn render_macos_app_group_template(
    template: &str,
    app_group: &str,
    template_name: &str,
) -> Result<String> {
    let placeholder_count = template.matches(MACOS_APP_GROUP_PLACEHOLDER).count();
    if placeholder_count != 1 {
        bail!("macOS {template_name} must contain exactly one {MACOS_APP_GROUP_PLACEHOLDER} token");
    }
    if app_group.is_empty()
        || !app_group
            .bytes()
            .all(|byte| byte.is_ascii_alphanumeric() || matches!(byte, b'.' | b'-'))
        || app_group != MACOS_APP_GROUP_ID
    {
        bail!("invalid Picoo Camera App Group identifier `{app_group}`");
    }
    Ok(template.replace(MACOS_APP_GROUP_PLACEHOLDER, app_group))
}

pub(crate) fn macos_team_identifier_prefix() -> Result<String> {
    let Ok(team_id) = std::env::var("PICOO_APPLE_TEAM_ID") else {
        return Ok(MACOS_UNSIGNED_TEAM_PREFIX.into());
    };
    macos_team_identifier_prefix_for(&team_id)
}

pub(crate) fn macos_signing_identifiers() -> Result<(String, String)> {
    match std::env::var("PICOO_APPLE_TEAM_ID") {
        Ok(team_id) => {
            let prefix = macos_team_identifier_prefix_for(&team_id)?;
            Ok((team_id, prefix))
        }
        Err(_) => Ok(("UNSIGNED".into(), MACOS_UNSIGNED_TEAM_PREFIX.into())),
    }
}

pub(crate) fn macos_team_identifier_prefix_for(team_id: &str) -> Result<String> {
    if team_id.len() != 10
        || !team_id
            .bytes()
            .all(|byte| byte.is_ascii_uppercase() || byte.is_ascii_digit())
    {
        bail!("PICOO_APPLE_TEAM_ID must be a 10-character uppercase Apple Team ID");
    }
    Ok(format!("{team_id}."))
}

pub(crate) fn macos_extension_app_group(sh: &Shell, extension: &Path) -> Result<String> {
    let info_plist = extension.join("Contents/Info.plist");
    let plist_json = cmd!(sh, "plutil -convert json -o - {info_plist}").read()?;
    let plist: serde_json::Value = serde_json::from_str(&plist_json)?;
    plist
        .get("PicooAppGroupIdentifier")
        .and_then(|value| value.as_str())
        .filter(|value| !value.is_empty())
        .map(str::to_owned)
        .ok_or_else(|| anyhow::anyhow!("macOS Camera Extension has no App Group identity"))
}

pub(crate) fn cargo_target_dir(sh: &Shell) -> Result<PathBuf> {
    let metadata = cmd!(sh, "cargo metadata --format-version 1 --no-deps").read()?;
    let metadata: serde_json::Value = serde_json::from_str(&metadata)?;
    let Some(target_dir) = metadata
        .get("target_directory")
        .and_then(|value| value.as_str())
    else {
        bail!("cargo metadata did not return target_directory");
    };
    Ok(PathBuf::from(target_dir))
}

pub(crate) fn archive_apple_bundle(sh: &Shell, source: &Path, archive: &Path) -> Result<()> {
    if !source.is_dir() {
        bail!("Apple bundle is missing {}", source.display());
    }
    if archive.exists() {
        std::fs::remove_file(archive)?;
    }
    cmd!(
        sh,
        "ditto -c -k --sequesterRsrc --keepParent {source} {archive}"
    )
    .run()?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::{
        macos_team_identifier_prefix_for, render_macos_entitlements, render_macos_host_info,
        validate_macos_build_number, validate_macos_marketing_version,
        MACOS_APPLICATION_IDENTIFIER_PLACEHOLDER, MACOS_APP_BUNDLE_ID, MACOS_APP_GROUP_ID,
        MACOS_APP_GROUP_PLACEHOLDER, MACOS_BUILD_NUMBER_PLACEHOLDER,
        MACOS_MARKETING_VERSION_PLACEHOLDER, MACOS_TEAM_IDENTIFIER_PLACEHOLDER,
        MACOS_UNSIGNED_BUILD_PLACEHOLDER,
    };

    #[test]
    fn macos_host_info_resolves_the_extension_app_group() {
        let template = format!(
            "<key>PicooAppGroupIdentifier</key><string>{MACOS_APP_GROUP_PLACEHOLDER}</string><string>{MACOS_MARKETING_VERSION_PLACEHOLDER}</string><string>{MACOS_BUILD_NUMBER_PLACEHOLDER}</string>{MACOS_UNSIGNED_BUILD_PLACEHOLDER}"
        );
        let rendered = render_macos_host_info(&template, MACOS_APP_GROUP_ID, "2.3.4", "42", true)
            .expect("render host Info.plist");
        assert!(rendered.contains(MACOS_APP_GROUP_ID));
        assert!(rendered.contains("2.3.4"));
        assert!(rendered.contains("42"));
        assert!(rendered.contains("<true/>"));
        assert!(!rendered.contains(MACOS_APP_GROUP_PLACEHOLDER));
    }

    #[test]
    fn macos_host_info_rejects_a_different_app_group() {
        assert!(render_macos_host_info(
            MACOS_APP_GROUP_PLACEHOLDER,
            "group.com.example.other",
            "1.0.0",
            "1",
            false
        )
        .is_err());
    }

    #[test]
    fn macos_host_entitlements_resolve_the_extension_app_group() {
        let template = format!(
            "<string>{MACOS_APP_GROUP_PLACEHOLDER}</string><string>{MACOS_TEAM_IDENTIFIER_PLACEHOLDER}</string><string>{MACOS_APPLICATION_IDENTIFIER_PLACEHOLDER}</string>"
        );
        let rendered = render_macos_entitlements(
            &template,
            MACOS_APP_GROUP_ID,
            "ABCDEFGHIJ",
            "ABCDEFGHIJ.",
            MACOS_APP_BUNDLE_ID,
            "test entitlements",
        )
        .expect("render host entitlements");
        assert!(rendered.contains(MACOS_APP_GROUP_ID));
        assert!(rendered.contains("ABCDEFGHIJ.com.haoxincode.picoo-camera"));
        assert!(!rendered.contains(MACOS_APP_GROUP_PLACEHOLDER));
    }

    #[test]
    fn macos_extension_entitlements_use_explicit_registered_app_group() {
        let template = format!(
            "<string>{MACOS_APP_GROUP_PLACEHOLDER}</string><string>{MACOS_TEAM_IDENTIFIER_PLACEHOLDER}</string><string>{MACOS_APPLICATION_IDENTIFIER_PLACEHOLDER}</string>"
        );
        let rendered = render_macos_entitlements(
            &template,
            MACOS_APP_GROUP_ID,
            "ABCDEFGHIJ",
            "ABCDEFGHIJ.",
            super::MACOS_EXTENSION_BUNDLE_ID,
            "test extension entitlements",
        )
        .expect("render extension entitlements");
        assert!(rendered.contains(MACOS_APP_GROUP_ID));
        assert!(!rendered.contains("$(TeamIdentifierPrefix)"));
    }

    #[test]
    fn macos_release_versions_follow_apple_bundle_shapes() {
        for version in ["1", "1.2", "1.2.3", "26.0.1"] {
            validate_macos_marketing_version(version).expect("valid marketing version");
        }
        for invalid in ["", "v1.2.3", "1.2.3.4", "1.2-beta"] {
            assert!(validate_macos_marketing_version(invalid).is_err());
        }
        for build in ["1", "42", "10001"] {
            validate_macos_build_number(build).expect("valid build number");
        }
        for invalid in ["", "0", "1.2", "run-1"] {
            assert!(validate_macos_build_number(invalid).is_err());
        }
    }

    #[test]
    fn macos_team_identifier_requires_the_apple_team_id_shape() {
        assert_eq!(
            macos_team_identifier_prefix_for("A1B2C3D4E5").expect("valid Apple Team ID"),
            "A1B2C3D4E5."
        );
        for invalid in ["SHORT", "abcdefghij", "A1B2C3D4E-", "A1B2C3D4E5F"] {
            assert!(macos_team_identifier_prefix_for(invalid).is_err());
        }
    }
}
