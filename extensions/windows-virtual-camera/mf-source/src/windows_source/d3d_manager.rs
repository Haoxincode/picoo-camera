//! Frame Server D3D11 manager admission for the native VCam sink.
//!
//! This module only proves and retains the system-provided device boundary.
//! It does not turn the CPU ring into a GPU surface or claim that a native
//! sample has been produced. A later native adapter must use this exact device.

use windows::core::{Error, IUnknown, Interface, Result};
use windows::Win32::Foundation::E_INVALIDARG;
use windows::Win32::Graphics::Direct3D11::{
    ID3D11Device, ID3D11Multithread, D3D11_CREATE_DEVICE_SINGLETHREADED,
};
use windows::Win32::Graphics::Dxgi::{IDXGIAdapter1, IDXGIDevice, DXGI_ADAPTER_FLAG_SOFTWARE};
use windows::Win32::Media::MediaFoundation::IMFDXGIDeviceManager;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) struct AdapterLuid {
    pub(super) low: u32,
    pub(super) high: i32,
}

/// The exact manager/device pair supplied by Frame Server.
#[derive(Clone)]
pub(super) struct NativeDeviceBinding {
    pub(super) manager: IMFDXGIDeviceManager,
    pub(super) device: ID3D11Device,
    pub(super) adapter: AdapterLuid,
}

impl std::fmt::Debug for NativeDeviceBinding {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("NativeDeviceBinding")
            .field("adapter", &self.adapter)
            .finish_non_exhaustive()
    }
}

impl NativeDeviceBinding {
    /// Resolve the manager's current D3D11 device and reject software or
    /// single-threaded devices before the binding becomes observable.
    pub(super) unsafe fn from_manager(manager: IMFDXGIDeviceManager) -> Result<Self> {
        let handle = manager.OpenDeviceHandle()?;
        let mut raw = std::ptr::null_mut();
        let lock_result = manager.LockDevice(handle, &ID3D11Device::IID, &mut raw, true);
        if let Err(error) = lock_result {
            let _ = manager.CloseDeviceHandle(handle);
            return Err(error);
        }
        let device = if raw.is_null() {
            let _ = manager.UnlockDevice(handle, false);
            let _ = manager.CloseDeviceHandle(handle);
            return Err(Error::from(E_INVALIDARG));
        } else {
            ID3D11Device::from_raw(raw)
        };
        let unlock_result = manager.UnlockDevice(handle, false);
        let close_result = manager.CloseDeviceHandle(handle);
        unlock_result?;
        close_result?;

        if device.GetCreationFlags() & D3D11_CREATE_DEVICE_SINGLETHREADED.0 != 0 {
            return Err(Error::from(E_INVALIDARG));
        }
        let dxgi: IDXGIDevice = device.cast()?;
        let adapter: IDXGIAdapter1 = dxgi.GetAdapter()?.cast()?;
        let description = adapter.GetDesc1()?;
        if description.Flags & DXGI_ADAPTER_FLAG_SOFTWARE.0 as u32 != 0 {
            return Err(Error::from(E_INVALIDARG));
        }
        let immediate = device.GetImmediateContext()?;
        let multithread: ID3D11Multithread = immediate.cast()?;
        if !multithread.GetMultithreadProtected().as_bool() {
            return Err(Error::from(E_INVALIDARG));
        }
        Ok(Self {
            manager,
            device,
            adapter: AdapterLuid {
                low: description.AdapterLuid.LowPart,
                high: description.AdapterLuid.HighPart,
            },
        })
    }

    pub(super) fn same_binding(&self, other: &Self) -> Result<bool> {
        Ok(
            self.manager.cast::<IUnknown>()? == other.manager.cast::<IUnknown>()?
                && self.device.cast::<IUnknown>()? == other.device.cast::<IUnknown>()?,
        )
    }
}
