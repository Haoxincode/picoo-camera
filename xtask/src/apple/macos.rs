use anyhow::{bail, Result};
use std::path::{Path, PathBuf};
use xshell::{cmd, Shell};

use super::macos_sign::{sign_and_notarize_macos, validate_macos_release_environment};
use super::macos_validate::{validate_macos_camera_extension, validate_macos_host_app};
use super::{
    apple_bundle_versions, archive_apple_bundle, cargo_target_dir, macos_extension_app_group,
    macos_signing_identifiers, macos_team_identifier_prefix, render_macos_entitlements,
    render_macos_host_info, MACOS_APP_BUNDLE_ID, MACOS_APP_GROUP_ID, MACOS_EXTENSION_BUNDLE_ID,
    MACOS_EXTENSION_BUNDLE_NAME,
};

pub(crate) fn release() -> Result<()> {
    validate_macos_release_environment()?;
    let sh = Shell::new()?;
    build_macos(&sh)?;
    let sh = Shell::new()?;
    package_macos(&sh, MacosPackageMode::Release)?;
    sign_and_notarize_macos(&sh)
}

pub(crate) fn build_macos(sh: &Shell) -> Result<()> {
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
pub(crate) enum MacosPackageMode {
    Unsigned,
    Release,
}

pub(crate) fn package_macos(sh: &Shell, mode: MacosPackageMode) -> Result<()> {
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

pub(crate) fn test_macos(sh: &Shell) -> Result<()> {
    if !cfg!(target_os = "macos") {
        bail!("macOS tests must run on a macOS host");
    }

    let _deployment_target = sh.push_env("MACOSX_DEPLOYMENT_TARGET", "15.0");
    let reader_harness = build_macos_shared_ring_reader_harness(sh)?;
    let _reader_harness = sh.push_env("PICOO_MACOS_RING_READER_HARNESS", &reader_harness);
    test_macos_system_identity_store(sh)?;
    cmd!(sh, "cargo test -p picoo-frame-hub --lib").run()?;
    cmd!(
        sh,
        "cargo test -p picoo-frame-hub --lib shared_ring::tests::macos::macos_rust_swift_cross_process_ring_contract -- --ignored --exact"
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

fn test_macos_system_identity_store(sh: &Shell) -> Result<()> {
    let original_default = cmd!(sh, "security default-keychain -d user")
        .read()?
        .trim()
        .trim_matches('"')
        .to_owned();
    let original_search = cmd!(sh, "security list-keychains -d user")
        .read()?
        .lines()
        .map(|line| line.trim().trim_matches('"').to_owned())
        .filter(|path| !path.is_empty())
        .collect::<Vec<_>>();
    if original_default.is_empty() || original_search.is_empty() {
        bail!("macOS user Keychain configuration is unavailable");
    }

    let keychain_dir = cargo_target_dir(sh)?.join("apple/macos-tests");
    std::fs::create_dir_all(&keychain_dir)?;
    let keychain = keychain_dir.join(format!(
        "picoo-identity-contract-{}.keychain-db",
        std::process::id()
    ));
    let password = format!("picoo-identity-contract-{}", std::process::id());
    if keychain.exists() {
        let _ = cmd!(sh, "security delete-keychain {keychain}").run();
        let _ = std::fs::remove_file(&keychain);
    }

    let test_result = (|| -> Result<()> {
        cmd!(sh, "security create-keychain -p {password} {keychain}").run()?;
        cmd!(sh, "security set-keychain-settings -lut 21600 {keychain}").run()?;
        cmd!(sh, "security unlock-keychain -p {password} {keychain}").run()?;
        cmd!(sh, "security default-keychain -d user -s {keychain}").run()?;
        cmd!(sh, "security list-keychains -d user -s {keychain}").run()?;
        cmd!(
            sh,
            "cargo test -p picoo-pairing --lib identity::tests::system_store_persists_and_fails_closed -- --ignored --exact"
        )
        .run()?;
        Ok(())
    })();

    let restore_default = cmd!(
        sh,
        "security default-keychain -d user -s {original_default}"
    )
    .run();
    let restore_search = cmd!(
        sh,
        "security list-keychains -d user -s {original_search...}"
    )
    .run();
    let delete_test_keychain = cmd!(sh, "security delete-keychain {keychain}").run();

    test_result?;
    restore_default?;
    restore_search?;
    delete_test_keychain?;
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
