//! Build orchestration — REQ-PICOO-STACK-004.
//!
//! Not a product engine; see ARCH-PICOO-STACK-001.

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
    /// Build a platform target
    Build {
        #[arg(value_enum)]
        platform: Platform,
    },
    /// Package release artifacts
    Package {
        #[arg(value_enum)]
        platform: Platform,
    },
    /// Run protocol/integration tests
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
                "cargo build -p picoo-desktop -p picoo-vcam-ring-reader --release --features gpui-ui"
            )
            .run()?;
            #[cfg(target_os = "windows")]
            {
                cmd!(
                    sh,
                    "cargo build -p picoo-media-decode --release --features windows-mf"
                )
                .run()?;
            }
            eprintln!("windows: MF VCam DLL + MSI packaging pending platform implementation");
        }
    }
    Ok(())
}

fn package(platform: Platform) -> Result<()> {
    match platform {
        Platform::Windows => {
            build(Platform::Windows)?;
            eprintln!("windows package: MSI/installer pipeline not yet implemented");
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
