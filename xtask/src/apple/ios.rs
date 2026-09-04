use anyhow::{bail, Result};
use std::path::{Path, PathBuf};
use xshell::{cmd, Shell};

use super::{apple_bundle_versions, archive_apple_bundle, cargo_target_dir};

pub(crate) fn build_ios(sh: &Shell) -> Result<()> {
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
        "xcrun --sdk iphonesimulator clang -target arm64-apple-ios18.0-simulator {smoke_source} -I {include_dir} {simulator_lib} -framework CoreFoundation -framework Security -framework SystemConfiguration -o {smoke_binary}"
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

fn static_library(target_dir: &Path, target: &str) -> PathBuf {
    target_dir
        .join(target)
        .join("release")
        .join("libpicoo_ffi.a")
}

pub(crate) fn test_ios(sh: &Shell) -> Result<()> {
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
    use super::ios_runtime_version;

    #[test]
    fn ios_runtime_versions_sort_numerically() {
        assert!(
            ios_runtime_version("com.apple.CoreSimulator.SimRuntime.iOS-26-10")
                > ios_runtime_version("com.apple.CoreSimulator.SimRuntime.iOS-26-9")
        );
    }
}
