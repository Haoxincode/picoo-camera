use anyhow::{bail, Result};
use std::path::Path;
use xshell::{cmd, Shell};

pub(crate) fn build(sh: &Shell) -> Result<()> {
    cmd!(
        sh,
        "cargo build -p picoo-desktop -p picoo-vcam-ring-reader -p picoo-windows-vcam-source --release --features gpui-ui,windows-vcam"
    )
    .run()?;
    Ok(())
}

pub(crate) fn package() -> Result<()> {
    let sh = Shell::new()?;
    build(&sh)?;
    let sh = Shell::new()?;
    let build_number = std::env::var("PICOO_BUILD_NUMBER").ok();
    let msi_version = windows_msi_version(env!("CARGO_PKG_VERSION"), build_number.as_deref())?;
    let stage_script = Path::new("installers/windows/stage.ps1");
    if stage_script.exists() {
        cmd!(
            sh,
            "powershell -ExecutionPolicy Bypass -File installers/windows/stage.ps1"
        )
        .env("PICOO_WINDOWS_MSI_VERSION", &msi_version)
        .run()?;
        eprintln!("windows MSI product version: {msi_version}");
    } else {
        eprintln!("windows package: stage.ps1 not found");
    }
    Ok(())
}

/// Resolve the three-field version Windows Installer actually compares.
///
/// Local packages use the workspace SemVer. CI replaces the third field with
/// GitHub's monotonically increasing workflow run number so every downloaded
/// MSI is a real major upgrade instead of a side-by-side same-version product.
fn windows_msi_version(package_version: &str, build_number: Option<&str>) -> Result<String> {
    let numeric_version = package_version
        .split(['-', '+'])
        .next()
        .unwrap_or(package_version);
    let components = numeric_version.split('.').collect::<Vec<_>>();
    if components.len() != 3 {
        bail!("workspace package version must contain three numeric fields for Windows MSI");
    }

    let major = components[0]
        .parse::<u16>()
        .map_err(|_| anyhow::anyhow!("invalid Windows MSI major version in `{package_version}`"))?;
    let minor = components[1]
        .parse::<u16>()
        .map_err(|_| anyhow::anyhow!("invalid Windows MSI minor version in `{package_version}`"))?;
    let package_patch = components[2]
        .parse::<u16>()
        .map_err(|_| anyhow::anyhow!("invalid Windows MSI patch version in `{package_version}`"))?;
    if major > 255 || minor > 255 {
        bail!("Windows MSI major and minor versions must not exceed 255");
    }

    let build = match build_number {
        Some(value) => {
            let build = value.parse::<u16>().map_err(|_| {
                anyhow::anyhow!("PICOO_BUILD_NUMBER must be an integer from 1 through 65535")
            })?;
            if build == 0 {
                bail!("PICOO_BUILD_NUMBER must be greater than zero");
            }
            if build <= package_patch {
                bail!(
                    "PICOO_BUILD_NUMBER ({build}) must exceed workspace patch version ({package_patch})"
                );
            }
            build
        }
        None => package_patch,
    };

    Ok(format!("{major}.{minor}.{build}"))
}

#[cfg(test)]
mod tests {
    use super::windows_msi_version;
    use std::path::Path;

    fn workspace_asset(relative: &str) -> std::path::PathBuf {
        Path::new(env!("CARGO_MANIFEST_DIR"))
            .parent()
            .expect("xtask has workspace parent")
            .join(relative)
    }

    #[test]
    fn windows_ci_build_number_produces_a_real_msi_upgrade_version() {
        assert_eq!(
            windows_msi_version("0.1.1", Some("472")).expect("valid CI version"),
            "0.1.472"
        );
        assert_eq!(
            windows_msi_version("2.3.4-beta.1", None).expect("valid local version"),
            "2.3.4"
        );
    }

    #[test]
    fn windows_msi_version_rejects_non_increasing_or_out_of_range_builds() {
        for build in ["0", "4", "not-a-number", "65536"] {
            assert!(windows_msi_version("1.2.4", Some(build)).is_err());
        }
        assert!(windows_msi_version("256.1.0", None).is_err());
        assert!(windows_msi_version("1.256.0", None).is_err());
        assert!(windows_msi_version("1.2", None).is_err());
    }

    #[test]
    fn windows_desktop_feature_graph_includes_mf_decoder() {
        let desktop = std::fs::read_to_string(workspace_asset("apps/desktop/Cargo.toml"))
            .expect("read desktop Cargo.toml");
        let receiver = std::fs::read_to_string(workspace_asset("crates/picoo-receiver/Cargo.toml"))
            .expect("read receiver Cargo.toml");
        assert!(
            desktop.contains("\"picoo-receiver/windows-mf\""),
            "Windows desktop feature must propagate Media Foundation into Receiver"
        );
        assert!(
            receiver.contains("windows-mf = [\"picoo-media-decode/windows-mf\"]"),
            "Receiver feature must propagate Media Foundation into media decoder"
        );
    }
}
