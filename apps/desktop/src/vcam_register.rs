//! Windows virtual camera registration — REQ-PICOO-VCAM-002.

use std::ffi::OsStr;
use std::os::windows::ffi::OsStrExt;

use windows::core::{GUID, PCWSTR};
use windows::Win32::Media::MediaFoundation::{
    IMFVirtualCamera, MFCreateVirtualCamera, MFShutdown, MFStartup,
    MFVirtualCameraAccess_CurrentUser, MFVirtualCameraLifetime_Session,
    MFVirtualCameraType_SoftwareCameraSource, MF_VERSION,
};
use windows::Win32::System::Com::{CoInitializeEx, COINIT_MULTITHREADED};

/// CLSID for `PicooVirtualCameraSource.dll` — must match `picoo_vcam_ids.h`.
pub const PICOO_VCAM_CLSID: GUID = GUID::from_u128(0xa7c4e2f1_8b3d_4c6a_9e5f_1d2c3b4a5e6f);

const FRIENDLY_NAME: &str = "Picoo Camera";

pub struct VirtualCameraRegistration {
    camera: IMFVirtualCamera,
}

impl VirtualCameraRegistration {
    /// Register and start the Picoo Camera virtual camera for the current user session.
    pub fn register_and_start() -> Result<Self, String> {
        let _com = ComInit::new()?;
        let _mf = MfInit::new()?;

        let friendly = wide(FRIENDLY_NAME);
        let clsid = wide(&format!("{{{}}}", guid_string(PICOO_VCAM_CLSID)));

        let camera = unsafe {
            MFCreateVirtualCamera(
                MFVirtualCameraType_SoftwareCameraSource,
                MFVirtualCameraLifetime_Session,
                MFVirtualCameraAccess_CurrentUser,
                PCWSTR(friendly.as_ptr()),
                PCWSTR(clsid.as_ptr()),
                None,
            )
        }
        .map_err(|e| format!("MFCreateVirtualCamera failed: {e}"))?;

        unsafe {
            camera
                .Start(None)
                .map_err(|e| format!("IMFVirtualCamera::Start failed: {e}"))?;
        }

        Ok(Self { camera })
    }

    /// Remove the virtual camera registration from the system.
    pub fn remove(self) -> Result<(), String> {
        unsafe {
            self.camera
                .Remove()
                .map_err(|e| format!("IMFVirtualCamera::Remove failed: {e}"))
        }
    }
}

struct ComInit;

impl ComInit {
    fn new() -> Result<Self, String> {
        unsafe {
            CoInitializeEx(None, COINIT_MULTITHREADED)
                .ok()
                .map_err(|e| format!("CoInitializeEx failed: {e}"))?;
        }
        Ok(Self)
    }
}

impl Drop for ComInit {
    fn drop(&mut self) {
        unsafe {
            windows::Win32::System::Com::CoUninitialize();
        }
    }
}

struct MfInit;

impl MfInit {
    fn new() -> Result<Self, String> {
        unsafe {
            MFStartup(MF_VERSION, Default::default())
                .map_err(|e| format!("MFStartup failed: {e}"))?;
        }
        Ok(Self)
    }
}

impl Drop for MfInit {
    fn drop(&mut self) {
        unsafe {
            let _ = MFShutdown();
        }
    }
}

fn wide(text: &str) -> Vec<u16> {
    OsStr::new(text).encode_wide().chain(Some(0)).collect()
}

fn guid_string(guid: GUID) -> String {
    format!(
        "{:08X}-{:04X}-{:04X}-{:02X}{:02X}-{:02X}{:02X}{:02X}{:02X}{:02X}{:02X}",
        guid.data1,
        guid.data2,
        guid.data3,
        guid.data4[0],
        guid.data4[1],
        guid.data4[2],
        guid.data4[3],
        guid.data4[4],
        guid.data4[5],
        guid.data4[6],
        guid.data4[7],
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn clsid_matches_cpp_header() {
        assert_eq!(
            guid_string(PICOO_VCAM_CLSID),
            "A7C4E2F1-8B3D-4C6A-9E5F-1D2C3B4A5E6F"
        );
    }
}
