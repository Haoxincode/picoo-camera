//! Run Apple Rust tests outside Cargo's large dependency directory.
//! CoreVideo's first IOSurface allocation asks NSBundle to enumerate adjacent
//! resources. A small executable directory keeps that work outside media deadlines.
use anyhow::{bail, Context, Result};
use std::path::PathBuf;
use xshell::{cmd, Shell};

pub(crate) fn run(sh: &Shell, cargo_args: &[&str], test_args: &[&str]) -> Result<()> {
    let messages = cmd!(
        sh,
        "cargo test {cargo_args...} --no-run --message-format=json"
    )
    .read()?;
    let executables = messages
        .lines()
        .map(serde_json::from_str::<serde_json::Value>)
        .collect::<std::result::Result<Vec<_>, _>>()?
        .into_iter()
        .filter(|message| {
            message["reason"] == "compiler-artifact" && message["profile"]["test"] == true
        })
        .filter_map(|message| message["executable"].as_str().map(PathBuf::from))
        .collect::<Vec<_>>();
    if executables.is_empty() {
        bail!("Cargo emitted no native test executables for {cargo_args:?}");
    }
    let directory = std::env::temp_dir().join(format!(
        "picoo-native-tests-{}-{}",
        std::process::id(),
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)?
            .as_nanos()
    ));
    std::fs::create_dir(&directory)?;
    let directory = TestDirectory(directory);
    for executable in executables {
        let name = executable
            .file_name()
            .context("test executable has no filename")?;
        let isolated = directory.0.join(name);
        std::fs::copy(&executable, &isolated)
            .with_context(|| format!("copy native test {}", executable.display()))?;
        cmd!(sh, "{isolated} {test_args...}").run()?;
    }
    Ok(())
}

struct TestDirectory(PathBuf);
impl Drop for TestDirectory {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.0);
    }
}
