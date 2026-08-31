//! Build orchestration — REQ-PICOO-STACK-004.

use anyhow::{bail, Result};
use clap::{Parser, Subcommand};
use picoo_session::SenderStatus;
use std::path::{Path, PathBuf};
use xshell::{cmd, Shell};

#[derive(Parser)]
#[command(name = "xtask")]
#[command(about = "Picoo Camera build tasks")]
struct Cli {
    #[command(subcommand)]
    command: Command,
}

#[derive(Subcommand)]
enum Command {
    Build {
        #[arg(value_enum)]
        platform: BuildPlatform,
    },
    Package {
        #[arg(value_enum)]
        platform: PackagePlatform,
    },
    /// Produce a signed and notarized release artifact.
    Release {
        #[arg(value_enum)]
        platform: ReleasePlatform,
    },
    Test {
        #[arg(value_enum)]
        suite: TestSuite,
    },
    /// Generate platform bindings whose numeric ABI is owned by Rust Core.
    Generate {
        #[arg(value_enum)]
        artifact: GeneratedArtifact,
        /// Fail if checked-in output is stale instead of rewriting it.
        #[arg(long)]
        check: bool,
    },
}

#[derive(Clone, Copy, clap::ValueEnum)]
enum BuildPlatform {
    Android,
    Ios,
    Macos,
    Windows,
}

#[derive(Clone, Copy, clap::ValueEnum)]
enum PackagePlatform {
    Android,
    Macos,
    Windows,
}

#[derive(Clone, Copy, clap::ValueEnum)]
enum ReleasePlatform {
    Macos,
}

#[derive(Clone, Copy, clap::ValueEnum)]
enum TestSuite {
    /// Swift/C ABI integration on an installed ARM64 iPhone Simulator runtime.
    Ios,
    /// VideoToolbox, Shared Frame Ring, and Apple product dependency boundaries.
    Macos,
    /// Windows Shared Frame Ring and Media Foundation source boundaries.
    Windows,
    Protocol,
    /// Linux-hostable product gates (WiX scaffold, VCam format, TXT sync, soak smoke).
    Linux,
}

#[derive(Clone, Copy, clap::ValueEnum)]
enum GeneratedArtifact {
    BrandIcons,
    SenderStatus,
}

fn main() -> Result<()> {
    let cli = Cli::parse();
    match cli.command {
        Command::Build { platform } => build(platform),
        Command::Package { platform } => package(platform),
        Command::Release { platform } => release(platform),
        Command::Test { suite } => test_suite(suite),
        Command::Generate { artifact, check } => generate(artifact, check),
    }
}

fn release(platform: ReleasePlatform) -> Result<()> {
    match platform {
        ReleasePlatform::Macos => {
            validate_macos_release_environment()?;
            build(BuildPlatform::Macos)?;
            let sh = Shell::new()?;
            package_macos(&sh, MacosPackageMode::Release)?;
            sign_and_notarize_macos(&sh)
        }
    }
}

fn validate_macos_release_environment() -> Result<()> {
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

fn generate(artifact: GeneratedArtifact, check: bool) -> Result<()> {
    match artifact {
        GeneratedArtifact::BrandIcons => generate_brand_icons(check),
        GeneratedArtifact::SenderStatus => generate_sender_status(check),
    }
}

fn generate_brand_icons(check: bool) -> Result<()> {
    if !cfg!(target_os = "macos") {
        bail!("brand icon generation requires macOS `sips` and `iconutil`");
    }

    let sh = Shell::new()?;
    let root = std::env::current_dir()?;
    let temp = std::env::temp_dir().join(format!("picoo-brand-icons-{}", std::process::id()));
    if temp.exists() {
        std::fs::remove_dir_all(&temp)?;
    }
    std::fs::create_dir_all(&temp)?;

    let result = (|| -> Result<()> {
        let app_svg = root.join("assets/brand/app-icon-master.svg");
        let tray_svg = root.join("assets/brand/tray-icon-master.svg");
        let app_png = temp.join("app-source.png");
        let tray_png = temp.join("tray-source.png");
        cmd!(sh, "sips -s format png {app_svg} --out {app_png}").run()?;
        cmd!(sh, "sips -s format png {tray_svg} --out {tray_png}").run()?;

        let iconset = temp.join("PicooCamera.iconset");
        std::fs::create_dir_all(&iconset)?;
        for (size, name) in [
            (16, "icon_16x16.png"),
            (32, "icon_16x16@2x.png"),
            (32, "icon_32x32.png"),
            (64, "icon_32x32@2x.png"),
            (128, "icon_128x128.png"),
            (256, "icon_128x128@2x.png"),
            (256, "icon_256x256.png"),
            (512, "icon_256x256@2x.png"),
            (512, "icon_512x512.png"),
            (1024, "icon_512x512@2x.png"),
        ] {
            let output = iconset.join(name);
            let size = size.to_string();
            cmd!(sh, "sips -z {size} {size} {app_png} --out {output}").run()?;
        }

        let generated_icns = temp.join("PicooCamera.icns");
        cmd!(sh, "iconutil -c icns {iconset} -o {generated_icns}").run()?;

        let app_ico = temp.join("PicooCamera.ico");
        write_windows_ico(&sh, &temp, &app_png, &app_ico, &[16, 24, 32, 48, 256])?;
        let tray_ico = temp.join("PicooCameraTray.ico");
        write_windows_ico(
            &sh,
            &temp,
            &tray_png,
            &tray_ico,
            &[16, 20, 24, 32, 40, 48, 64],
        )?;

        for (generated, checked_in) in [
            (
                generated_icns,
                root.join("assets/brand/macos/PicooCamera.icns"),
            ),
            (app_ico, root.join("assets/brand/windows/PicooCamera.ico")),
            (
                tray_ico,
                root.join("assets/brand/windows/PicooCameraTray.ico"),
            ),
        ] {
            write_or_check_binary(&generated, &checked_in, check)?;
        }
        Ok(())
    })();

    let cleanup = std::fs::remove_dir_all(&temp);
    result?;
    cleanup?;
    Ok(())
}

fn write_windows_ico(
    sh: &Shell,
    temp: &Path,
    source: &Path,
    output: &Path,
    sizes: &[u32],
) -> Result<()> {
    let mut directory = ico::IconDir::new(ico::ResourceType::Icon);
    for size in sizes {
        let png = temp.join(format!(
            "ico-{size}-{}.png",
            output.file_stem().unwrap().to_string_lossy()
        ));
        let size = size.to_string();
        cmd!(sh, "sips -z {size} {size} {source} --out {png}").run()?;
        let image = ico::IconImage::read_png(std::fs::File::open(&png)?)?;
        directory.add_entry(ico::IconDirEntry::encode(&image)?);
    }
    directory.write(std::fs::File::create(output)?)?;
    Ok(())
}

fn write_or_check_binary(generated: &Path, checked_in: &Path, check: bool) -> Result<()> {
    let expected = std::fs::read(generated)?;
    if check {
        let actual = std::fs::read(checked_in)?;
        if actual != expected {
            bail!(
                "{} is stale; run `cargo xtask generate brand-icons`",
                checked_in.display()
            );
        }
        return Ok(());
    }

    std::fs::create_dir_all(
        checked_in
            .parent()
            .ok_or_else(|| anyhow::anyhow!("brand icon output has no parent"))?,
    )?;
    std::fs::write(checked_in, expected)?;
    Ok(())
}

fn generate_sender_status(check: bool) -> Result<()> {
    let entries = SenderStatus::ALL.map(|status| {
        let (kotlin, swift) = match status {
            SenderStatus::Disconnected => ("DISCONNECTED", "disconnected"),
            SenderStatus::Discovering => ("DISCOVERING", "discovering"),
            SenderStatus::Pairing => ("PAIRING", "pairing"),
            SenderStatus::Connecting => ("CONNECTING", "connecting"),
            SenderStatus::Negotiating => ("NEGOTIATING", "negotiating"),
            SenderStatus::Streaming => ("STREAMING", "streaming"),
            SenderStatus::Reconnecting => ("RECONNECTING", "reconnecting"),
            SenderStatus::PermissionRequired => ("PERMISSION_REQUIRED", "permissionRequired"),
            SenderStatus::NetworkUnstable => ("NETWORK_UNSTABLE", "networkUnstable"),
        };
        (status, kotlin, swift)
    });

    let mut kotlin = String::from(
        "package com.picoo.camera.jni\n\n// @generated by `cargo xtask generate sender-status`; do not edit.\nobject SenderStatusCodes {\n",
    );
    for (status, name, _) in entries {
        kotlin.push_str(&format!("    const val {name} = {}\n", status.as_code()));
    }
    kotlin.push_str("\n    fun label(status: Int): String =\n        when (status) {\n");
    for (status, name, _) in entries {
        if status != SenderStatus::Disconnected {
            kotlin.push_str(&format!(
                "            {name} -> \"{}\"\n",
                status.as_label()
            ));
        }
    }
    kotlin.push_str("            else -> \"Disconnected\"\n        }\n}\n");

    let swift_begin = "// BEGIN GENERATED SENDER STATUS";
    let swift_end = "// END GENERATED SENDER STATUS";
    let mut swift_block = format!(
        "{swift_begin}\n// @generated by `cargo xtask generate sender-status`; do not edit.\nnonisolated enum PicooSenderStatus: Int32, Sendable {{\n"
    );
    for (status, _, name) in entries {
        swift_block.push_str(&format!("    case {name} = {}\n", status.as_code()));
    }
    swift_block.push_str(
        "\n    init(code: Int32) {\n        self = Self(rawValue: code) ?? .disconnected\n    }\n}\n",
    );
    swift_block.push_str(swift_end);

    write_or_check(
        Path::new("apps/android/app/src/main/kotlin/com/picoo/camera/jni/SenderStatusCodes.kt"),
        &kotlin,
        check,
    )?;

    let swift_path = Path::new("apps/ios/PicooCamera/SenderModels.swift");
    let swift = std::fs::read_to_string(swift_path)?;
    let start = swift
        .find(swift_begin)
        .ok_or_else(|| anyhow::anyhow!("{} is missing {swift_begin}", swift_path.display()))?;
    let end = swift[start..]
        .find(swift_end)
        .map(|offset| start + offset + swift_end.len())
        .ok_or_else(|| anyhow::anyhow!("{} is missing {swift_end}", swift_path.display()))?;
    let generated_swift = format!("{}{}{}", &swift[..start], swift_block, &swift[end..]);
    write_or_check(swift_path, &generated_swift, check)
}

fn write_or_check(path: &Path, expected: &str, check: bool) -> Result<()> {
    if check {
        let actual = std::fs::read_to_string(path)?;
        if actual != expected {
            bail!(
                "{} is stale; run `cargo xtask generate sender-status`",
                path.display()
            );
        }
    } else {
        std::fs::write(path, expected)?;
    }
    Ok(())
}

fn build(platform: BuildPlatform) -> Result<()> {
    let sh = Shell::new()?;
    match platform {
        BuildPlatform::Android => {
            cmd!(sh, "cargo test --workspace").run()?;
            if Path::new("apps/android/gradlew").exists() {
                if let Ok(sdk) = std::env::var("ANDROID_HOME") {
                    sh.write_file("apps/android/local.properties", format!("sdk.dir={sdk}\n"))?;
                }
                cmd!(sh, "./apps/android/gradlew -p apps/android assembleDebug").run()?;
            } else {
                eprintln!("android: gradle project not yet configured — workspace tests passed");
            }
        }
        BuildPlatform::Ios => build_ios(&sh)?,
        BuildPlatform::Macos => build_macos(&sh)?,
        BuildPlatform::Windows => {
            cmd!(
                sh,
                "cargo build -p picoo-desktop -p picoo-vcam-ring-reader -p picoo-windows-vcam-source --release --features gpui-ui,windows-vcam"
            )
            .run()?;
            cmd!(
                sh,
                "cargo build -p picoo-media-decode --release --features windows-mf"
            )
            .run()?;
        }
    }
    Ok(())
}

fn package(platform: PackagePlatform) -> Result<()> {
    match platform {
        PackagePlatform::Windows => {
            build(BuildPlatform::Windows)?;
            let sh = Shell::new()?;
            let build_number = std::env::var("PICOO_BUILD_NUMBER").ok();
            let msi_version =
                windows_msi_version(env!("CARGO_PKG_VERSION"), build_number.as_deref())?;
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
        PackagePlatform::Android => {
            let sh = Shell::new()?;
            if !Path::new("apps/android/gradlew").exists() {
                bail!("android gradle project missing");
            }
            if let Ok(sdk) = std::env::var("ANDROID_HOME") {
                sh.write_file("apps/android/local.properties", format!("sdk.dir={sdk}\n"))?;
            }
            // REQ-PICOO-STACK-005 / TRANSPORT-005: release APK + AAB (debug-signed, 签名前可用).
            cmd!(
                sh,
                "./apps/android/gradlew -p apps/android assembleRelease bundleRelease"
            )
            .run()?;
            // Xiaomi 15 / Android 15: cold-start .so must be 16 KB page-aligned.
            cmd!(sh, "bash scripts/check_android_so_16k.sh").run()?;
            eprintln!(
                "android package: APK=apps/android/app/build/outputs/apk/release/ \
                 AAB=apps/android/app/build/outputs/bundle/release/"
            );
            Ok(())
        }
        PackagePlatform::Macos => {
            build(BuildPlatform::Macos)?;
            let sh = Shell::new()?;
            package_macos(&sh, MacosPackageMode::Unsigned)
        }
    }
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

const MACOS_APP_BUNDLE_ID: &str = "com.haoxincode.picoo-camera";
const MACOS_EXTENSION_BUNDLE_ID: &str = "com.haoxincode.picoo-camera.camera-extension";
const MACOS_EXTENSION_BUNDLE_NAME: &str =
    "com.haoxincode.picoo-camera.camera-extension.systemextension";
const MACOS_APP_GROUP_ID: &str = "group.com.haoxincode.picoo-camera";
const MACOS_APP_GROUP_PLACEHOLDER: &str = "@PICOO_APP_GROUP_IDENTIFIER@";
const MACOS_TEAM_IDENTIFIER_PLACEHOLDER: &str = "@PICOO_TEAM_IDENTIFIER@";
const MACOS_APPLICATION_IDENTIFIER_PLACEHOLDER: &str = "@PICOO_APPLICATION_IDENTIFIER@";
const MACOS_MARKETING_VERSION_PLACEHOLDER: &str = "@PICOO_MARKETING_VERSION@";
const MACOS_BUILD_NUMBER_PLACEHOLDER: &str = "@PICOO_BUILD_NUMBER@";
const MACOS_UNSIGNED_BUILD_PLACEHOLDER: &str = "@PICOO_UNSIGNED_DEVELOPMENT_BUILD@";
const MACOS_UNSIGNED_TEAM_PREFIX: &str = "UNSIGNED.";

fn build_ios(sh: &Shell) -> Result<()> {
    if !cfg!(target_os = "macos") {
        bail!("iOS Core must be built on a macOS host with Xcode");
    }

    const DEVICE_TARGET: &str = "aarch64-apple-ios";
    const ARM64_SIM_TARGET: &str = "aarch64-apple-ios-sim";
    let _deployment_target = sh.push_env("IPHONEOS_DEPLOYMENT_TARGET", "18.0");

    for target in [DEVICE_TARGET, ARM64_SIM_TARGET] {
        cmd!(
            sh,
            "cargo rustc -p picoo-ffi --release --target {target} --lib -- --crate-type staticlib"
        )
        .run()?;
    }

    let cargo_target_dir = cargo_target_dir(sh)?;
    // Keep final Apple products at a repository-stable path so Xcode and CI do
    // not need to reproduce Cargo's potentially host-specific target setting.
    let apple_dir = std::env::current_dir()?.join("target/apple");
    let include_dir = apple_dir.join("include");
    let simulator_dir = apple_dir.join("ios-simulator");
    let xcframework = apple_dir.join("PicooCore.xcframework");
    std::fs::create_dir_all(&include_dir)?;
    std::fs::create_dir_all(&simulator_dir)?;

    let header = Path::new("crates/picoo-ffi/picoo_camera.h");
    if !header.is_file() {
        bail!("cbindgen did not produce {}", header.display());
    }
    std::fs::copy(header, include_dir.join("picoo_camera.h"))?;
    std::fs::write(
        include_dir.join("module.modulemap"),
        "module PicooCore {\n  header \"picoo_camera.h\"\n  export *\n}\n",
    )?;

    let device_lib = static_library(&cargo_target_dir, DEVICE_TARGET);
    let arm64_sim_lib = static_library(&cargo_target_dir, ARM64_SIM_TARGET);
    let simulator_lib = simulator_dir.join("libpicoo_ffi.a");
    for library in [&device_lib, &arm64_sim_lib] {
        if !library.is_file() {
            bail!("missing iOS static library: {}", library.display());
        }
    }

    std::fs::copy(&arm64_sim_lib, &simulator_lib)?;

    let smoke_source = Path::new("scripts/apple_ffi_smoke.c");
    let smoke_binary = simulator_dir.join("picoo-ffi-smoke");
    cmd!(
        sh,
        "xcrun --sdk iphonesimulator clang -target arm64-apple-ios18.0-simulator {smoke_source} -I {include_dir} {simulator_lib} -framework Security -framework SystemConfiguration -o {smoke_binary}"
    )
    .run()?;

    if xcframework.exists() {
        std::fs::remove_dir_all(&xcframework)?;
    }
    cmd!(
        sh,
        "xcodebuild -create-xcframework -library {device_lib} -headers {include_dir} -library {simulator_lib} -headers {include_dir} -output {xcframework}"
    )
    .run()?;
    validate_ios_xcframework(sh, &xcframework)?;

    let ios_app_dir = apple_dir.join("ios-app");
    let ios_obj_dir = apple_dir.join("ios-obj");
    let ios_products_dir = apple_dir.join("ios-products");
    let project = Path::new("apps/ios/PicooCamera.xcodeproj");
    if !project.is_dir() {
        bail!("iOS Xcode project is missing {}", project.display());
    }
    let (marketing_version, build_number) = apple_bundle_versions()?;
    cmd!(
        sh,
        "xcodebuild -project {project} -target PicooCamera -configuration Debug -sdk iphonesimulator -arch arm64 CODE_SIGNING_ALLOWED=NO MARKETING_VERSION={marketing_version} CURRENT_PROJECT_VERSION={build_number} CONFIGURATION_BUILD_DIR={ios_app_dir} OBJROOT={ios_obj_dir} SYMROOT={ios_products_dir} PICOO_CORE_XCFRAMEWORK_PATH={xcframework} build"
    )
    .run()?;

    let xcframework_archive = apple_dir.join("PicooCore.xcframework.zip");
    let app_archive = apple_dir.join("PicooCamera.app.zip");
    archive_apple_bundle(sh, &xcframework, &xcframework_archive)?;
    archive_apple_bundle(sh, &ios_app_dir.join("PicooCamera.app"), &app_archive)?;

    eprintln!("ios core: {}", xcframework.display());
    eprintln!("ios app (unsigned): {}", ios_app_dir.display());
    eprintln!(
        "ios artifacts: {}, {}",
        xcframework_archive.display(),
        app_archive.display()
    );
    Ok(())
}

fn build_macos(sh: &Shell) -> Result<()> {
    if !cfg!(target_os = "macos") {
        bail!("macOS Receiver and Camera Extension must be built on a macOS host");
    }

    let _deployment_target = sh.push_env("MACOSX_DEPLOYMENT_TARGET", "15.0");
    cmd!(
        sh,
        "cargo build -p picoo-desktop --release --features gpui-ui"
    )
    .run()?;
    let receiver = cargo_target_dir(sh)?.join("release/picoo-desktop");
    if !receiver.is_file() {
        bail!("macOS Receiver was not produced at {}", receiver.display());
    }

    let project = Path::new("extensions/macos-camera-extension/PicooCameraExtension.xcodeproj");
    if !project.is_dir() {
        bail!(
            "macOS Camera Extension Xcode project is missing {}",
            project.display()
        );
    }

    let apple_dir = std::env::current_dir()?.join("target/apple");
    let extension_dir = apple_dir.join("macos");
    let object_dir = apple_dir.join("macos-obj");
    let products_dir = apple_dir.join("macos-products");
    let team_prefix = macos_team_identifier_prefix()?;
    let (marketing_version, build_number) = apple_bundle_versions()?;
    if extension_dir.exists() {
        std::fs::remove_dir_all(&extension_dir)?;
    }
    std::fs::create_dir_all(&extension_dir)?;
    cmd!(
        sh,
        "xcodebuild -project {project} -target PicooCameraExtension -configuration Release -sdk macosx -arch arm64 CODE_SIGNING_ALLOWED=NO TeamIdentifierPrefix={team_prefix} MARKETING_VERSION={marketing_version} CURRENT_PROJECT_VERSION={build_number} CONFIGURATION_BUILD_DIR={extension_dir} OBJROOT={object_dir} SYMROOT={products_dir} build"
    )
    .run()?;

    let extension = extension_dir.join(MACOS_EXTENSION_BUNDLE_NAME);
    validate_macos_camera_extension(sh, &extension)?;

    eprintln!("macOS receiver: {}", receiver.display());
    eprintln!("macOS Camera Extension (unsigned): {}", extension.display());
    Ok(())
}

#[derive(Clone, Copy, PartialEq, Eq)]
enum MacosPackageMode {
    Unsigned,
    Release,
}

fn package_macos(sh: &Shell, mode: MacosPackageMode) -> Result<()> {
    if !cfg!(target_os = "macos") {
        bail!("macOS app must be packaged on a macOS host");
    }

    let root = std::env::current_dir()?;
    let apple_dir = root.join("target/apple");
    let app = apple_dir.join("macos/Picoo Camera.app");
    let executable = app.join("Contents/MacOS/picoo-desktop");
    let embedded_extension = app
        .join("Contents/Library/SystemExtensions")
        .join(MACOS_EXTENSION_BUNDLE_NAME);
    let built_extension = apple_dir.join("macos").join(MACOS_EXTENSION_BUNDLE_NAME);
    let receiver = cargo_target_dir(sh)?.join("release/picoo-desktop");
    let info_template = root.join("installers/macos/Info.plist");
    let app_icon = root.join("assets/brand/macos/PicooCamera.icns");
    let entitlements_template = root.join("installers/macos/PicooCamera.entitlements");
    let extension_entitlements_template =
        root.join("extensions/macos-camera-extension/PicooCameraExtension.entitlements");
    let rendered_entitlements = apple_dir.join("PicooCamera-macOS.entitlements");
    let rendered_extension_entitlements = apple_dir.join("PicooCameraExtension-macOS.entitlements");

    for required in [
        &receiver,
        &built_extension,
        &info_template,
        &app_icon,
        &entitlements_template,
        &extension_entitlements_template,
    ] {
        if !required.exists() {
            bail!("macOS package input is missing {}", required.display());
        }
    }

    if app.exists() {
        std::fs::remove_dir_all(&app)?;
    }
    std::fs::create_dir_all(executable.parent().expect("app executable parent"))?;
    std::fs::create_dir_all(app.join("Contents/Resources"))?;
    std::fs::create_dir_all(
        embedded_extension
            .parent()
            .expect("system extension parent"),
    )?;
    std::fs::copy(&receiver, &executable)?;
    std::fs::copy(&app_icon, app.join("Contents/Resources/PicooCamera.icns"))?;
    cmd!(sh, "ditto {built_extension} {embedded_extension}").run()?;

    let extension_group = macos_extension_app_group(sh, &embedded_extension)?;
    if extension_group != MACOS_APP_GROUP_ID {
        bail!(
            "macOS Camera Extension App Group `{extension_group}` does not match `{MACOS_APP_GROUP_ID}`"
        );
    }
    let (marketing_version, build_number) = apple_bundle_versions()?;
    let (signing_team_id, signing_team_prefix) = macos_signing_identifiers()?;
    // Packaging mode, not the presence of a Team ID, owns this runtime
    // behavior. `package macos` is always unsigned; only `release macos`
    // clears the marker immediately before codesigning the same bundle.
    let unsigned_build = mode == MacosPackageMode::Unsigned;
    let info = render_macos_host_info(
        &std::fs::read_to_string(&info_template)?,
        &extension_group,
        &marketing_version,
        &build_number,
        unsigned_build,
    )?;
    std::fs::write(app.join("Contents/Info.plist"), info)?;
    let entitlements = render_macos_entitlements(
        &std::fs::read_to_string(&entitlements_template)?,
        &extension_group,
        &signing_team_id,
        &signing_team_prefix,
        MACOS_APP_BUNDLE_ID,
        "Host entitlements",
    )?;
    std::fs::write(&rendered_entitlements, entitlements)?;
    let extension_entitlements = render_macos_entitlements(
        &std::fs::read_to_string(&extension_entitlements_template)?,
        &extension_group,
        &signing_team_id,
        &signing_team_prefix,
        MACOS_EXTENSION_BUNDLE_ID,
        "Extension entitlements",
    )?;
    std::fs::write(&rendered_extension_entitlements, extension_entitlements)?;

    validate_macos_host_app(
        sh,
        &app,
        &rendered_entitlements,
        &rendered_extension_entitlements,
        unsigned_build,
    )?;
    let unsigned_archive = apple_dir.join("PicooCamera-macOS-unsigned.zip");
    if mode == MacosPackageMode::Unsigned {
        archive_apple_bundle(sh, &app, &unsigned_archive)?;
        eprintln!("macOS app (unsigned): {}", app.display());
        eprintln!("macOS app artifact: {}", unsigned_archive.display());
    } else {
        // A release staging bundle has marker=false but is not safe to launch
        // until the immediately following signing step. Never persist it as
        // an unsigned distributable, and remove a stale package artifact.
        if unsigned_archive.exists() {
            std::fs::remove_file(&unsigned_archive)?;
        }
        eprintln!("macOS release app staging: {}", app.display());
    }
    Ok(())
}

fn sign_and_notarize_macos(sh: &Shell) -> Result<()> {
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

fn validate_signed_macos_bundle(
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

fn required_env(name: &str) -> Result<String> {
    std::env::var(name)
        .ok()
        .filter(|value| !value.trim().is_empty())
        .ok_or_else(|| anyhow::anyhow!("{name} is required"))
}

fn render_macos_host_info(
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

fn render_macos_entitlements(
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

fn apple_bundle_versions() -> Result<(String, String)> {
    let marketing_version =
        std::env::var("PICOO_RELEASE_VERSION").unwrap_or_else(|_| env!("CARGO_PKG_VERSION").into());
    let build_number = std::env::var("PICOO_RELEASE_BUILD_NUMBER")
        .or_else(|_| std::env::var("PICOO_BUILD_NUMBER"))
        .unwrap_or_else(|_| "2".into());
    validate_macos_marketing_version(&marketing_version)?;
    validate_macos_build_number(&build_number)?;
    Ok((marketing_version, build_number))
}

fn validate_macos_marketing_version(version: &str) -> Result<()> {
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

fn validate_macos_build_number(version: &str) -> Result<()> {
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

fn macos_team_identifier_prefix() -> Result<String> {
    let Ok(team_id) = std::env::var("PICOO_APPLE_TEAM_ID") else {
        return Ok(MACOS_UNSIGNED_TEAM_PREFIX.into());
    };
    macos_team_identifier_prefix_for(&team_id)
}

fn macos_signing_identifiers() -> Result<(String, String)> {
    match std::env::var("PICOO_APPLE_TEAM_ID") {
        Ok(team_id) => {
            let prefix = macos_team_identifier_prefix_for(&team_id)?;
            Ok((team_id, prefix))
        }
        Err(_) => Ok(("UNSIGNED".into(), MACOS_UNSIGNED_TEAM_PREFIX.into())),
    }
}

fn macos_team_identifier_prefix_for(team_id: &str) -> Result<String> {
    if team_id.len() != 10
        || !team_id
            .bytes()
            .all(|byte| byte.is_ascii_uppercase() || byte.is_ascii_digit())
    {
        bail!("PICOO_APPLE_TEAM_ID must be a 10-character uppercase Apple Team ID");
    }
    Ok(format!("{team_id}."))
}

fn macos_extension_app_group(sh: &Shell, extension: &Path) -> Result<String> {
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

fn validate_macos_host_app(
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

fn validate_macos_camera_extension(sh: &Shell, extension: &Path) -> Result<()> {
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

fn validate_ios_xcframework(sh: &Shell, xcframework: &Path) -> Result<()> {
    let info_plist = xcframework.join("Info.plist");
    if !info_plist.is_file() {
        bail!("XCFramework is missing {}", info_plist.display());
    }

    let plist_json = cmd!(sh, "plutil -convert json -o - {info_plist}").read()?;
    let plist: serde_json::Value = serde_json::from_str(&plist_json)?;
    let Some(libraries) = plist
        .get("AvailableLibraries")
        .and_then(|value| value.as_array())
    else {
        bail!("XCFramework Info.plist has no AvailableLibraries");
    };

    let mut has_device_arm64 = false;
    let mut has_arm64_simulator = false;
    for library in libraries {
        if library
            .get("SupportedPlatform")
            .and_then(|value| value.as_str())
            != Some("ios")
        {
            continue;
        }
        let architectures = library
            .get("SupportedArchitectures")
            .and_then(|value| value.as_array())
            .into_iter()
            .flatten()
            .filter_map(|value| value.as_str())
            .collect::<Vec<_>>();
        match library
            .get("SupportedPlatformVariant")
            .and_then(|value| value.as_str())
        {
            Some("simulator") => {
                has_arm64_simulator = architectures == ["arm64"];
            }
            None => has_device_arm64 = architectures.contains(&"arm64"),
            _ => {}
        }
    }

    if !has_device_arm64 || !has_arm64_simulator {
        bail!("XCFramework must contain ARM64-only iOS device and simulator slices");
    }
    Ok(())
}

fn cargo_target_dir(sh: &Shell) -> Result<PathBuf> {
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

fn static_library(target_dir: &Path, target: &str) -> PathBuf {
    target_dir
        .join(target)
        .join("release")
        .join("libpicoo_ffi.a")
}

fn archive_apple_bundle(sh: &Shell, source: &Path, archive: &Path) -> Result<()> {
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

fn test_suite(suite: TestSuite) -> Result<()> {
    let sh = Shell::new()?;
    match suite {
        TestSuite::Ios => test_ios(&sh)?,
        TestSuite::Macos => test_macos(&sh)?,
        TestSuite::Windows => {
            if !cfg!(target_os = "windows") {
                bail!("Windows tests must run on a Windows host");
            }
            cmd!(
                sh,
                "cargo clippy -p picoo-frame-hub -p picoo-windows-vcam-source --all-targets -- -D warnings"
            )
            .run()?;
            cmd!(
                sh,
                "cargo test -p picoo-frame-hub -p picoo-windows-vcam-source"
            )
            .run()?;
        }
        TestSuite::Protocol => {
            cmd!(
                sh,
                "cargo test -p picoo-protocol -p picoo-packet -p picoo-transport -p picoo-testkit"
            )
            .run()?;
        }
        TestSuite::Linux => {
            // REQ-PICOO-VCAM-004 / DISCOVERY-005 / SESSION-005..007 / PAIRING-003 — no Win11 GUI.
            cmd!(sh, "bash scripts/validate_wix_scaffold.sh").run()?;
            cmd!(sh, "cargo test -p picoo-windows-vcam-source").run()?;
            cmd!(sh, "bash scripts/check_discovery_txt_keys.sh").run()?;
            cmd!(
                sh,
                "cargo test -p picoo-receiver --lib soak_harness_smoke_five_seconds"
            )
            .run()?;
            cmd!(
                sh,
                "cargo test -p picoo-receiver --lib unpaired_video_keeps_shared_ring_on_placeholder"
            )
            .run()?;
            cmd!(
                sh,
                "cargo test -p picoo-receiver --lib paired_loopback_remains_usable_under_five_percent_loss"
            )
            .run()?;
            cmd!(
                sh,
                "cargo test -p picoo-receiver --lib paired_loopback_e2e_latency_p50_under_budget"
            )
            .run()?;
            cmd!(
                sh,
                "cargo test -p picoo-receiver --lib paired_openh264_remains_usable_under_five_percent_loss"
            )
            .run()?;
            cmd!(
                sh,
                "cargo test -p picoo-receiver --lib paired_openh264_e2e_latency_p50_under_budget"
            )
            .run()?;
            cmd!(
                sh,
                "cargo test -p picoo-receiver --lib mismatched_protocol_version_rejects_client_hello"
            )
            .run()?;
            cmd!(
                sh,
                "cargo test -p picoo-receiver --lib capabilities_720_only_are_applied_before_sender_stream_config"
            )
            .run()?;
            cmd!(
                sh,
                "cargo test -p picoo-receiver --lib manual_endpoint_connects_to_streaming"
            )
            .run()?;
            cmd!(
                sh,
                "cargo test -p picoo-receiver --lib reconnect_churn_smoke_five_rounds"
            )
            .run()?;
            cmd!(
                sh,
                "cargo test -p picoo-receiver --lib reconnect_churn_fifteen_rounds"
            )
            .run()?;
            cmd!(
                sh,
                "cargo test -p picoo-receiver --lib public_key_change_rejects_auto_connect"
            )
            .run()?;
            cmd!(
                sh,
                "cargo test -p picoo-receiver --lib unpaired_start_stream_is_rejected"
            )
            .run()?;
            cmd!(
                sh,
                "cargo test -p picoo-receiver --lib unpaired_stop_stream_is_ignored_without_teardown"
            )
            .run()?;
            cmd!(
                sh,
                "cargo test -p picoo-receiver --lib paired_start_stop_stream_and_camera_command_roundtrip"
            )
            .run()?;
            cmd!(
                sh,
                "cargo test -p picoo-discovery --lib synthetic_advertise_to_list_p50_under_two_seconds"
            )
            .run()?;
            cmd!(
                sh,
                "cargo test -p picoo-ffi --lib export_diagnostics_with_session_includes_redacted_host"
            )
            .run()?;
        }
    }
    Ok(())
}

fn test_macos(sh: &Shell) -> Result<()> {
    if !cfg!(target_os = "macos") {
        bail!("macOS tests must run on a macOS host");
    }

    let _deployment_target = sh.push_env("MACOSX_DEPLOYMENT_TARGET", "15.0");
    let reader_harness = build_macos_shared_ring_reader_harness(sh)?;
    let _reader_harness = sh.push_env("PICOO_MACOS_RING_READER_HARNESS", &reader_harness);
    cmd!(sh, "cargo test -p picoo-frame-hub --lib").run()?;
    cmd!(
        sh,
        "cargo test -p picoo-frame-hub --lib shared_ring::tests::macos_rust_swift_cross_process_ring_contract -- --ignored --exact"
    )
    .run()?;
    cmd!(sh, "cargo test -p picoo-media-decode").run()?;
    cmd!(
        sh,
        "cargo test -p picoo-receiver --lib paired_avcc_length_prefixed_au_reaches_frame_hub"
    )
    .run()?;
    cmd!(
        sh,
        "cargo test -p picoo-receiver --lib macos_videotoolbox_abr_epoch_resolution_recovery"
    )
    .run()?;

    // REQ-PICOO-MEDIA-012 / STACK-001: the Apple product must not regain
    // OpenH264's native build chain after moving decode to VideoToolbox.
    let dependency_trees = [
        (
            "product",
            cmd!(
                sh,
                "cargo tree -p picoo-desktop --target aarch64-apple-darwin --features gpui-ui"
            )
            .read()?,
        ),
        (
            "test",
            cmd!(
                sh,
                "cargo tree -p picoo-receiver --target aarch64-apple-darwin --edges normal,build,dev"
            )
            .read()?,
        ),
    ];
    for (tree_name, tree) in dependency_trees {
        for forbidden in ["openh264 v", "openh264-sys", "cmake v"] {
            if tree.contains(forbidden) {
                bail!("macOS {tree_name} dependency tree contains forbidden `{forbidden}`");
            }
        }
    }
    Ok(())
}

fn build_macos_shared_ring_reader_harness(sh: &Shell) -> Result<PathBuf> {
    let source_dir = Path::new("extensions/macos-camera-extension");
    let atomic_source = source_dir.join("SharedRingAtomic.c");
    let atomic_header = source_dir.join("SharedRingAtomic.h");
    let reader_source = source_dir.join("SharedRingReader.swift");
    let harness_source = source_dir.join("tests/SharedRingReaderHarness.swift");
    for source in [
        &atomic_source,
        &atomic_header,
        &reader_source,
        &harness_source,
    ] {
        if !source.is_file() {
            bail!(
                "macOS Shared Frame Ring test source is missing {}",
                source.display()
            );
        }
    }

    let output_dir = std::env::current_dir()?.join("target/apple/macos-tests");
    std::fs::create_dir_all(&output_dir)?;
    let atomic_object = output_dir.join("SharedRingAtomic.o");
    let harness = output_dir.join("picoo-shared-ring-reader-harness");
    cmd!(
        sh,
        "xcrun --sdk macosx clang -std=c17 -Wall -Wextra -Werror -arch arm64 -mmacosx-version-min=15.0 -c {atomic_source} -o {atomic_object}"
    )
    .run()?;
    cmd!(
        sh,
        "xcrun --sdk macosx swiftc -parse-as-library -swift-version 6 -strict-concurrency=complete -warnings-as-errors -target arm64-apple-macos15.0 -import-objc-header {atomic_header} {reader_source} {harness_source} {atomic_object} -framework CoreVideo -o {harness}"
    )
    .run()?;
    if !harness.is_file() {
        bail!("macOS Shared Frame Ring reader harness was not produced");
    }
    Ok(harness)
}

fn test_ios(sh: &Shell) -> Result<()> {
    if !cfg!(target_os = "macos") {
        bail!("iOS tests must run on a macOS host with an iPhone Simulator runtime");
    }

    let apple_dir = std::env::current_dir()?.join("target/apple");
    let xcframework = apple_dir.join("PicooCore.xcframework");
    if !xcframework.is_dir() {
        bail!(
            "iOS XCFramework is missing {}; run `cargo xtask build ios` first",
            xcframework.display()
        );
    }

    let simulator_json = cmd!(sh, "xcrun simctl list devices available --json").read()?;
    let simulator_data: serde_json::Value = serde_json::from_str(&simulator_json)?;
    let mut simulators = simulator_data
        .get("devices")
        .and_then(|value| value.as_object())
        .into_iter()
        .flat_map(|runtimes| runtimes.iter())
        .flat_map(|(runtime, devices)| {
            devices
                .as_array()
                .into_iter()
                .flatten()
                .filter(|device| {
                    device.get("isAvailable").and_then(|value| value.as_bool()) == Some(true)
                        && device
                            .get("name")
                            .and_then(|value| value.as_str())
                            .is_some_and(|name| name.starts_with("iPhone"))
                })
                .filter_map(|device| {
                    Some((
                        ios_runtime_version(runtime),
                        runtime.to_owned(),
                        device.get("name")?.as_str()?.to_owned(),
                        device.get("udid")?.as_str()?.to_owned(),
                    ))
                })
        })
        .collect::<Vec<_>>();
    simulators.sort();
    let Some((_version, runtime, name, udid)) = simulators.pop() else {
        bail!("no available iPhone Simulator runtime is installed");
    };

    let project = Path::new("apps/ios/PicooCamera.xcodeproj");
    let derived_data = apple_dir.join("ios-derived-data");
    let destination = format!("platform=iOS Simulator,id={udid}");
    eprintln!("ios tests: {name} ({runtime}, {udid})");
    cmd!(
        sh,
        "xcodebuild -project {project} -scheme PicooCamera -configuration Debug -destination {destination} -derivedDataPath {derived_data} CODE_SIGNING_ALLOWED=NO PICOO_CORE_XCFRAMEWORK_PATH={xcframework} test"
    )
    .run()?;
    Ok(())
}

fn ios_runtime_version(identifier: &str) -> Vec<u32> {
    identifier
        .rsplit('.')
        .next()
        .and_then(|suffix| suffix.strip_prefix("iOS-"))
        .into_iter()
        .flat_map(|version| version.split('-'))
        .filter_map(|component| component.parse().ok())
        .collect()
}

#[cfg(test)]
mod tests {
    use super::{
        ios_runtime_version, macos_team_identifier_prefix_for, render_macos_entitlements,
        render_macos_host_info, validate_macos_build_number, validate_macos_marketing_version,
        windows_msi_version, MACOS_APPLICATION_IDENTIFIER_PLACEHOLDER, MACOS_APP_BUNDLE_ID,
        MACOS_APP_GROUP_ID, MACOS_APP_GROUP_PLACEHOLDER, MACOS_BUILD_NUMBER_PLACEHOLDER,
        MACOS_MARKETING_VERSION_PLACEHOLDER, MACOS_TEAM_IDENTIFIER_PLACEHOLDER,
        MACOS_UNSIGNED_BUILD_PLACEHOLDER,
    };
    use std::path::Path;

    fn ico_sizes(path: &Path) -> Vec<(u32, u32)> {
        let directory =
            ico::IconDir::read(std::fs::File::open(path).expect("open ICO")).expect("parse ICO");
        directory
            .entries()
            .iter()
            .map(|entry| (entry.width(), entry.height()))
            .collect()
    }

    fn workspace_asset(relative: &str) -> std::path::PathBuf {
        Path::new(env!("CARGO_MANIFEST_DIR"))
            .parent()
            .expect("xtask has workspace parent")
            .join(relative)
    }

    #[test]
    fn checked_in_brand_icons_cover_platform_sizes() {
        assert_eq!(
            ico_sizes(&workspace_asset("assets/brand/windows/PicooCamera.ico")),
            [(16, 16), (24, 24), (32, 32), (48, 48), (256, 256)]
        );
        assert_eq!(
            ico_sizes(&workspace_asset("assets/brand/windows/PicooCameraTray.ico",)),
            [
                (16, 16),
                (20, 20),
                (24, 24),
                (32, 32),
                (40, 40),
                (48, 48),
                (64, 64),
            ]
        );
        let icns = std::fs::read(workspace_asset("assets/brand/macos/PicooCamera.icns"))
            .expect("read ICNS");
        assert!(icns.starts_with(b"icns"));
        assert!(icns.len() > 100_000, "ICNS should include Retina variants");
    }

    #[test]
    fn ios_runtime_versions_sort_numerically() {
        assert!(
            ios_runtime_version("com.apple.CoreSimulator.SimRuntime.iOS-26-10")
                > ios_runtime_version("com.apple.CoreSimulator.SimRuntime.iOS-26-9")
        );
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
