//! Scoped COM and Media Foundation runtime ownership for VCam maintenance.

use windows::Win32::Foundation::RPC_E_CHANGED_MODE;
use windows::Win32::Media::MediaFoundation::{MFShutdown, MFStartup, MF_VERSION};
use windows::Win32::System::Com::{
    CoInitializeEx, CoUninitialize, COINIT_APARTMENTTHREADED, COINIT_DISABLE_OLE1DDE,
    COINIT_MULTITHREADED,
};

pub(super) struct ComInit;

impl ComInit {
    pub(super) fn new() -> Result<Self, String> {
        unsafe {
            CoInitializeEx(None, COINIT_MULTITHREADED)
                .ok()
                .map_err(|error| format!("CoInitializeEx failed: {error}"))?;
        }
        Ok(Self)
    }
}

impl Drop for ComInit {
    fn drop(&mut self) {
        unsafe { CoUninitialize() };
    }
}

/// Accept an existing GPUI STA apartment, and only uninitialize COM when this
/// guard initialized the caller thread itself.
pub(super) struct EnumerationComInit {
    initialized_here: bool,
}

impl EnumerationComInit {
    pub(super) fn new() -> Result<Self, String> {
        let result = unsafe { CoInitializeEx(None, COINIT_MULTITHREADED) };
        if result.is_ok() {
            Ok(Self {
                initialized_here: true,
            })
        } else if result == RPC_E_CHANGED_MODE {
            Ok(Self {
                initialized_here: false,
            })
        } else {
            let error = windows::core::Error::from_hresult(result);
            Err(format!("摄像头枚举 COM 初始化失败：{error}"))
        }
    }
}

impl Drop for EnumerationComInit {
    fn drop(&mut self) {
        if self.initialized_here {
            unsafe { CoUninitialize() };
        }
    }
}

pub(super) struct ShellComInit;

impl ShellComInit {
    pub(super) fn new() -> Result<Self, String> {
        unsafe {
            CoInitializeEx(None, COINIT_APARTMENTTHREADED | COINIT_DISABLE_OLE1DDE)
                .ok()
                .map_err(|error| format!("Shell COM initialization failed: {error}"))?;
        }
        Ok(Self)
    }
}

impl Drop for ShellComInit {
    fn drop(&mut self) {
        unsafe { CoUninitialize() };
    }
}

pub(super) struct MfInit;

impl MfInit {
    pub(super) fn new() -> Result<Self, String> {
        unsafe {
            MFStartup(MF_VERSION, Default::default())
                .map_err(|error| format!("MFStartup failed: {error}"))?;
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
