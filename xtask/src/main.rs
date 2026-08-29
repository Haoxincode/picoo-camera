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
    Windows,
}

#[derive(Clone, Copy, clap::ValueEnum)]
enum TestSuite {
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
        BuildPlatform::Macos => {
            if !cfg!(target_os = "macos") {
                bail!("macOS Receiver must be built on a macOS host");
            }
            let _deployment_target = sh.push_env("MACOSX_DEPLOYMENT_TARGET", "15.0");
            cmd!(
                sh,
                "cargo build -p picoo-desktop --release --features gpui-ui"
            )
            .run()?;
        }
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
    }
}

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

    let target_dir = cargo_target_dir(sh)?;
    let apple_dir = target_dir.join("apple");
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

    let device_lib = static_library(&target_dir, DEVICE_TARGET);
    let arm64_sim_lib = static_library(&target_dir, ARM64_SIM_TARGET);
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

    eprintln!("ios core: {}", xcframework.display());
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

fn test_suite(suite: TestSuite) -> Result<()> {
    let sh = Shell::new()?;
    match suite {
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
