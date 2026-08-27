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
            bail!("use `xtask build android` for APK until package target is defined")
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
    }
    Ok(())
}
