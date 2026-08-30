//! Build orchestration — REQ-PICOO-STACK-004.

use anyhow::{bail, Result};
use clap::{Parser, Subcommand};
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
    Test {
        #[arg(value_enum)]
        suite: TestSuite,
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

fn main() -> Result<()> {
    let cli = Cli::parse();
    match cli.command {
        Command::Build { platform } => build(platform),
        Command::Package { platform } => package(platform),
        Command::Test { suite } => test_suite(suite),
    }
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
            let stage_script = Path::new("installers/windows/stage.ps1");
            if stage_script.exists() {
                cmd!(
                    sh,
                    "powershell -ExecutionPolicy Bypass -File installers/windows/stage.ps1"
                )
                .run()?;
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
            package_macos(&sh)
        }
    }
}

const MACOS_APP_BUNDLE_ID: &str = "com.haoxincode.picoo-camera";
const MACOS_EXTENSION_BUNDLE_ID: &str = "com.haoxincode.picoo-camera.camera-extension";
const MACOS_EXTENSION_BUNDLE_NAME: &str =
    "com.haoxincode.picoo-camera.camera-extension.systemextension";
const MACOS_APP_GROUP_SUFFIX: &str = "com.haoxincode.picoo-camera";
const MACOS_APP_GROUP_PLACEHOLDER: &str = "@PICOO_APP_GROUP_IDENTIFIER@";
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
    cmd!(
        sh,
        "xcodebuild -project {project} -target PicooCamera -configuration Debug -sdk iphonesimulator -arch arm64 CODE_SIGNING_ALLOWED=NO CONFIGURATION_BUILD_DIR={ios_app_dir} OBJROOT={ios_obj_dir} SYMROOT={ios_products_dir} PICOO_CORE_XCFRAMEWORK_PATH={xcframework} build"
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
    if extension_dir.exists() {
        std::fs::remove_dir_all(&extension_dir)?;
    }
    std::fs::create_dir_all(&extension_dir)?;
    cmd!(
        sh,
        "xcodebuild -project {project} -target PicooCameraExtension -configuration Release -sdk macosx -arch arm64 CODE_SIGNING_ALLOWED=NO TeamIdentifierPrefix={team_prefix} CONFIGURATION_BUILD_DIR={extension_dir} OBJROOT={object_dir} SYMROOT={products_dir} build"
    )
    .run()?;

    let extension = extension_dir.join(MACOS_EXTENSION_BUNDLE_NAME);
    validate_macos_camera_extension(sh, &extension)?;

    eprintln!("macOS receiver: {}", receiver.display());
    eprintln!("macOS Camera Extension (unsigned): {}", extension.display());
    Ok(())
}

fn package_macos(sh: &Shell) -> Result<()> {
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
    let entitlements_template = root.join("installers/macos/PicooCamera.entitlements");
    let rendered_entitlements = apple_dir.join("PicooCamera-macOS.entitlements");

    for required in [
        &receiver,
        &built_extension,
        &info_template,
        &entitlements_template,
    ] {
        if !required.exists() {
            bail!("macOS package input is missing {}", required.display());
        }
    }

    if app.exists() {
        std::fs::remove_dir_all(&app)?;
    }
    std::fs::create_dir_all(executable.parent().expect("app executable parent"))?;
    std::fs::create_dir_all(
        embedded_extension
            .parent()
            .expect("system extension parent"),
    )?;
    std::fs::copy(&receiver, &executable)?;
    cmd!(sh, "ditto {built_extension} {embedded_extension}").run()?;

    let extension_group = macos_extension_app_group(sh, &embedded_extension)?;
    let expected_group = format!(
        "{}{MACOS_APP_GROUP_SUFFIX}",
        macos_team_identifier_prefix()?
    );
    if extension_group != expected_group {
        bail!(
            "macOS Camera Extension App Group `{extension_group}` does not match `{expected_group}`"
        );
    }
    let info = render_macos_host_info(&std::fs::read_to_string(&info_template)?, &extension_group)?;
    std::fs::write(app.join("Contents/Info.plist"), info)?;
    let entitlements = render_macos_host_entitlements(
        &std::fs::read_to_string(&entitlements_template)?,
        &extension_group,
    )?;
    std::fs::write(&rendered_entitlements, entitlements)?;

    validate_macos_host_app(sh, &app, &rendered_entitlements)?;
    let archive = apple_dir.join("PicooCamera-macOS-unsigned.zip");
    archive_apple_bundle(sh, &app, &archive)?;

    eprintln!("macOS app (unsigned): {}", app.display());
    eprintln!("macOS app artifact: {}", archive.display());
    Ok(())
}

fn render_macos_host_info(template: &str, app_group: &str) -> Result<String> {
    render_macos_app_group_template(template, app_group, "Host Info.plist")
}

fn render_macos_host_entitlements(template: &str, app_group: &str) -> Result<String> {
    render_macos_app_group_template(template, app_group, "Host entitlements")
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
        || !app_group.ends_with(MACOS_APP_GROUP_SUFFIX)
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

fn validate_macos_host_app(sh: &Shell, app: &Path, entitlements: &Path) -> Result<()> {
    let info_plist = app.join("Contents/Info.plist");
    let executable = app.join("Contents/MacOS/picoo-desktop");
    let embedded_extension = app
        .join("Contents/Library/SystemExtensions")
        .join(MACOS_EXTENSION_BUNDLE_NAME);
    if !info_plist.is_file() || !executable.is_file() || !embedded_extension.is_dir() {
        bail!("incomplete macOS Host app bundle: {}", app.display());
    }

    let plist_json = cmd!(sh, "plutil -convert json -o - {info_plist}").read()?;
    let plist: serde_json::Value = serde_json::from_str(&plist_json)?;
    let host_group = plist
        .get("PicooAppGroupIdentifier")
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
    if !mach_service.is_some_and(|service| {
        app_group.is_some_and(|group| service.starts_with(&format!("{group}.")))
    }) {
        bail!("macOS Camera Extension Mach service must be namespaced under its App Group");
    }

    let entitlements =
        Path::new("extensions/macos-camera-extension/PicooCameraExtension.entitlements");
    let entitlements_json = cmd!(sh, "plutil -convert json -o - {entitlements}").read()?;
    let entitlements: serde_json::Value = serde_json::from_str(&entitlements_json)?;
    let expected_group = "$(TeamIdentifierPrefix)com.haoxincode.picoo-camera";
    if !entitlements
        .get("com.apple.security.application-groups")
        .and_then(|value| value.as_array())
        .is_some_and(|groups| groups.iter().any(|group| group == expected_group))
    {
        bail!("macOS Camera Extension entitlement is missing `{expected_group}`");
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
                "cargo test -p picoo-receiver --lib capabilities_720_only_clamps_sender_stream_config"
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
        ios_runtime_version, macos_team_identifier_prefix_for, render_macos_host_entitlements,
        render_macos_host_info, MACOS_APP_GROUP_PLACEHOLDER,
    };

    #[test]
    fn ios_runtime_versions_sort_numerically() {
        assert!(
            ios_runtime_version("com.apple.CoreSimulator.SimRuntime.iOS-26-10")
                > ios_runtime_version("com.apple.CoreSimulator.SimRuntime.iOS-26-9")
        );
    }

    #[test]
    fn macos_host_info_resolves_the_extension_app_group() {
        let template = format!(
            "<key>PicooAppGroupIdentifier</key><string>{MACOS_APP_GROUP_PLACEHOLDER}</string>"
        );
        let rendered = render_macos_host_info(&template, "TEAM.com.haoxincode.picoo-camera")
            .expect("render host Info.plist");
        assert!(rendered.contains("TEAM.com.haoxincode.picoo-camera"));
        assert!(!rendered.contains(MACOS_APP_GROUP_PLACEHOLDER));
    }

    #[test]
    fn macos_host_info_rejects_a_different_app_group() {
        assert!(
            render_macos_host_info(MACOS_APP_GROUP_PLACEHOLDER, "TEAM.com.example.other").is_err()
        );
    }

    #[test]
    fn macos_host_entitlements_resolve_the_extension_app_group() {
        let template = format!(
            "<key>com.apple.security.application-groups</key><array><string>{MACOS_APP_GROUP_PLACEHOLDER}</string></array>"
        );
        let rendered =
            render_macos_host_entitlements(&template, "ABCDEFGHIJ.com.haoxincode.picoo-camera")
                .expect("render host entitlements");
        assert!(rendered.contains("ABCDEFGHIJ.com.haoxincode.picoo-camera"));
        assert!(!rendered.contains(MACOS_APP_GROUP_PLACEHOLDER));
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
