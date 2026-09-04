//! Build orchestration — REQ-PICOO-STACK-004.

use anyhow::Result;
use clap::{Parser, Subcommand};
use xshell::Shell;

mod android;
mod apple;
mod generate;
mod test_suite;
mod windows;

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
    Windows,
}

#[derive(Clone, Copy, clap::ValueEnum)]
pub(crate) enum TestSuite {
    /// Swift/C ABI integration on an installed ARM64 iPhone Simulator runtime.
    Ios,
    /// VideoToolbox, Shared Frame Ring, and Apple product dependency boundaries.
    Macos,
    /// Windows Shared Frame Ring and Media Foundation source boundaries.
    Windows,
    Protocol,
    /// Bounded cargo-fuzz campaign for all PCP parser/state targets.
    Fuzz,
    /// Long-running paired loopback memory/network soak.
    Soak,
    /// Strict-provenance Miri checks for raw Shared Ring and C ABI boundaries.
    Miri,
    /// Exhaustive Loom model for the Shared Ring atomic lease protocol.
    Loom,
    /// Linux-hostable product gates (WiX scaffold, VCam format, TXT sync, soak smoke).
    Linux,
}

#[derive(Clone, Copy, clap::ValueEnum)]
pub(crate) enum GeneratedArtifact {
    BrandIcons,
    SenderStatus,
}

fn main() -> Result<()> {
    let cli = Cli::parse();
    match cli.command {
        Command::Build { platform } => build(platform),
        Command::Package { platform } => package(platform),
        Command::Release { platform } => release(platform),
        Command::Test { suite } => test_suite::run(suite),
        Command::Generate { artifact, check } => generate::run(artifact, check),
    }
}

fn build(platform: BuildPlatform) -> Result<()> {
    let sh = Shell::new()?;
    match platform {
        BuildPlatform::Android => android::build(&sh)?,
        BuildPlatform::Ios => apple::ios::build_ios(&sh)?,
        BuildPlatform::Macos => apple::macos::build_macos(&sh)?,
        BuildPlatform::Windows => windows::build(&sh)?,
    }
    Ok(())
}

fn package(platform: PackagePlatform) -> Result<()> {
    match platform {
        PackagePlatform::Windows => windows::package(),
        PackagePlatform::Android => android::package(),
        PackagePlatform::Macos => {
            build(BuildPlatform::Macos)?;
            let sh = Shell::new()?;
            apple::macos::package_macos(&sh, apple::macos::MacosPackageMode::Unsigned)
        }
    }
}

fn release(platform: ReleasePlatform) -> Result<()> {
    match platform {
        ReleasePlatform::Macos => apple::macos::release(),
        ReleasePlatform::Windows => windows::release(),
    }
}
