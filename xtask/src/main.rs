//! Build orchestration — REQ-PICOO-STACK-004.

use anyhow::{bail, Result};
use clap::{Parser, Subcommand};
use std::path::Path;
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
        platform: Platform,
    },
    Package {
        #[arg(value_enum)]
        platform: Platform,
    },
    Test {
        #[arg(value_enum)]
        suite: TestSuite,
    },
}

#[derive(Clone, Copy, clap::ValueEnum)]
enum Platform {
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

fn build(platform: Platform) -> Result<()> {
    let sh = Shell::new()?;
    match platform {
        Platform::Android => {
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
        Platform::Windows => {
            cmd!(
                sh,
                "cargo build -p picoo-desktop -p picoo-vcam-ring-reader --release --features gpui-ui,windows-vcam"
            )
            .run()?;
            cmd!(
                sh,
                "cargo build -p picoo-media-decode --release --features windows-mf"
            )
            .run()?;
            build_vcam_dll(&sh)?;
        }
    }
    Ok(())
}

fn build_vcam_dll(sh: &Shell) -> Result<()> {
    let cmake_lists = Path::new("extensions/windows-virtual-camera/mf-source/CMakeLists.txt");
    if !cmake_lists.exists() {
        eprintln!("windows: mf-source CMake project not found");
        return Ok(());
    }
    let build_dir = "target/vcam-build";
    cmd!(
        sh,
        "cmake -S extensions/windows-virtual-camera/mf-source -B {build_dir}"
    )
    .run()?;
    cmd!(sh, "cmake --build {build_dir} --config Release").run()?;
    eprintln!("windows: built PicooVirtualCameraSource.dll scaffold");
    Ok(())
}

fn package(platform: Platform) -> Result<()> {
    match platform {
        Platform::Windows => {
            build(Platform::Windows)?;
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
        Platform::Android => {
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
            eprintln!(
                "android package: APK=apps/android/app/build/outputs/apk/release/ \
                 AAB=apps/android/app/build/outputs/bundle/release/"
            );
            Ok(())
        }
    }
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
            cmd!(sh, "bash scripts/test_vcam_format.sh").run()?;
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
                "cargo test -p picoo-receiver --lib qr_json_payload_connects_to_streaming"
            )
            .run()?;
            cmd!(
                sh,
                "cargo test -p picoo-receiver --lib reconnect_churn_smoke_five_rounds"
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
