use anyhow::{bail, Result};
use picoo_session::SenderStatus;
use std::path::Path;
use xshell::{cmd, Shell};

use crate::GeneratedArtifact;

pub(crate) fn run(artifact: GeneratedArtifact, check: bool) -> Result<()> {
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

#[cfg(test)]
mod tests {
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
}
