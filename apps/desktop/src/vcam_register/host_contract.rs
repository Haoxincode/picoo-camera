//! Installed Windows 11 virtual-camera host contract — REQ-PICOO-VCAM-001/004/012.

use std::path::PathBuf;
use std::time::{Duration, Instant};

use windows::core::{Interface, GUID};
use windows::Win32::Media::MediaFoundation::{IMFMediaSource, IMFMediaSourceEx};
use windows::Win32::System::Com::StructuredStorage::PROPVARIANT;

use super::runtime::{EnumerationComInit, MfInit};
use super::{
    enumerate_registered_camera_activation, paths_equivalent, read_inproc_server_path,
    read_registry_string, resolve_installed_vcam_dll, ENUMERATION_RETRY_INTERVAL,
    ENUMERATION_WAIT_TIMEOUT, PRODUCT_KEY, SYMBOLIC_LINK_VALUE,
};

const ABSENCE_WAIT_TIMEOUT: Duration = Duration::from_secs(20);
const ABSENCE_RETRY_INTERVAL: Duration = Duration::from_millis(100);

#[derive(Debug)]
pub struct InstalledHostContract {
    pub installed_dll: PathBuf,
    pub symbolic_link: String,
}

/// Verify the installed, system-registered camera through the same public
/// Media Foundation activation path used by Frame Server clients.
///
/// This probe is intentionally read-only. MSI registration remains the only
/// authority that creates or repairs machine state.
pub fn verify_installed_host_contract() -> Result<InstalledHostContract, String> {
    let installed_dll = resolve_installed_vcam_dll()?;
    let registered_dll = read_inproc_server_path()
        .ok_or_else(|| "Picoo Camera COM server is not registered in HKLM".to_string())?;
    if !paths_equivalent(&installed_dll, &registered_dll) {
        return Err(format!(
            "Picoo Camera COM server points to `{}` instead of installed DLL `{}`",
            registered_dll.display(),
            installed_dll.display()
        ));
    }

    let expected_link = read_registry_string(PRODUCT_KEY, Some(SYMBOLIC_LINK_VALUE))
        .filter(|value| !value.is_empty())
        .ok_or_else(|| "Picoo Camera system registration identity is missing".to_string())?;

    let _com = EnumerationComInit::new()?;
    let _mf = MfInit::new()?;
    let deadline = Instant::now() + ENUMERATION_WAIT_TIMEOUT;
    let (activation, symbolic_link) = loop {
        match enumerate_registered_camera_activation(&expected_link)? {
            Some(matched) => break matched,
            None if Instant::now() < deadline => {
                std::thread::sleep(ENUMERATION_RETRY_INTERVAL);
            }
            None => {
                return Err(format!(
                    "Picoo Camera identity is persisted but MFEnumDeviceSources could not enumerate it within {} seconds",
                    ENUMERATION_WAIT_TIMEOUT.as_secs()
                ));
            }
        }
    };

    let source: IMFMediaSource = unsafe { activation.ActivateObject() }
        .map_err(|err| format!("IMFActivate::ActivateObject(IMFMediaSource) failed: {err}"))?;
    let _source_ex: IMFMediaSourceEx = source
        .cast()
        .map_err(|err| format!("activated source does not expose IMFMediaSourceEx: {err}"))?;

    let exercise_result = exercise_media_source(&source);
    let shutdown_result = unsafe { source.Shutdown() }
        .map_err(|err| format!("IMFMediaSource::Shutdown failed: {err}"));
    unsafe {
        let _ = activation.ShutdownObject();
    }
    exercise_result?;
    shutdown_result?;

    Ok(InstalledHostContract {
        installed_dll,
        symbolic_link,
    })
}

fn exercise_media_source(source: &IMFMediaSource) -> Result<(), String> {
    let presentation = unsafe { source.CreatePresentationDescriptor() }
        .map_err(|err| format!("CreatePresentationDescriptor failed: {err}"))?;
    unsafe { presentation.SelectStream(0) }
        .map_err(|err| format!("SelectStream(0) failed: {err}"))?;
    let start_position = PROPVARIANT::default();
    unsafe { source.Start(&presentation, &GUID::zeroed(), &start_position) }
        .map_err(|err| format!("IMFMediaSource::Start failed: {err}"))?;
    if let Err(err) = unsafe { source.Stop() } {
        return Err(format!("IMFMediaSource::Stop failed: {err}"));
    }
    Ok(())
}

/// Wait until the exact pre-uninstall symbolic link disappears from Media
/// Foundation enumeration. The caller supplies the saved identity because the
/// MSI correctly removes Picoo's registry value during uninstall.
pub fn verify_camera_absent(expected_link: &str) -> Result<(), String> {
    if expected_link.is_empty() {
        return Err("expected symbolic link must not be empty".to_string());
    }
    let _com = EnumerationComInit::new()?;
    let _mf = MfInit::new()?;
    let deadline = Instant::now() + ABSENCE_WAIT_TIMEOUT;
    loop {
        if enumerate_registered_camera_activation(expected_link)?.is_none() {
            return Ok(());
        }
        if Instant::now() >= deadline {
            return Err(format!(
                "Picoo Camera device `{expected_link}` remains enumerable after uninstall"
            ));
        }
        std::thread::sleep(ABSENCE_RETRY_INTERVAL);
    }
}

#[cfg(test)]
mod tests {
    #[test]
    fn identity_matching_remains_case_insensitive() {
        assert!(super::super::camera_identity_matches(
            "picoo-link",
            Some("PICOO-LINK")
        ));
    }
}
