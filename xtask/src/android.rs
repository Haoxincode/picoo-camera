use anyhow::{bail, Result};
use std::path::Path;
use xshell::{cmd, Shell};

pub(crate) fn build(sh: &Shell) -> Result<()> {
    cmd!(sh, "cargo test --workspace").run()?;
    if Path::new("apps/android/gradlew").exists() {
        if let Ok(sdk) = std::env::var("ANDROID_HOME") {
            sh.write_file("apps/android/local.properties", format!("sdk.dir={sdk}\n"))?;
        }
        cmd!(sh, "./apps/android/gradlew -p apps/android assembleDebug").run()?;
    } else {
        eprintln!("android: gradle project not yet configured — workspace tests passed");
    }
    Ok(())
}

pub(crate) fn package() -> Result<()> {
    let sh = Shell::new()?;
    if !Path::new("apps/android/gradlew").exists() {
        bail!("android gradle project missing");
    }
    if let Ok(sdk) = std::env::var("ANDROID_HOME") {
        sh.write_file("apps/android/local.properties", format!("sdk.dir={sdk}\n"))?;
    }
    // REQ-PICOO-STACK-005/008: Release packaging is allowed only with the
    // stable signer environment; Gradle fails closed if it is absent.
    cmd!(
        sh,
        "./apps/android/gradlew -p apps/android --no-daemon assembleRelease bundleRelease"
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
