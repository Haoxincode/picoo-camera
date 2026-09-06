//! Windows platform device ownership; codec admission remains the Decoder's job.
use windows::core::Interface;
use windows::Win32::Foundation::HMODULE;
use windows::Win32::Graphics::Direct3D::{
    D3D_DRIVER_TYPE_UNKNOWN, D3D_FEATURE_LEVEL_11_0, D3D_FEATURE_LEVEL_11_1,
};
use windows::Win32::Graphics::Direct3D11::{
    D3D11CreateDevice, ID3D11Device, ID3D11DeviceContext, ID3D11Multithread,
    D3D11_CREATE_DEVICE_BGRA_SUPPORT, D3D11_CREATE_DEVICE_VIDEO_SUPPORT, D3D11_SDK_VERSION,
};
use windows::Win32::Graphics::Dxgi::{IDXGIAdapter1, DXGI_ADAPTER_FLAG_SOFTWARE};
use windows::Win32::Media::MediaFoundation::{IMFDXGIDeviceManager, MFCreateDXGIDeviceManager};

#[derive(Debug, thiserror::Error)]
pub enum WindowsDeviceError {
    #[error("software adapters cannot run the production native media pipeline")]
    SoftwareAdapter,
    #[error("D3D11 did not create all required device objects")]
    MissingDevice,
    #[error("D3D11 context multithread protection is unavailable")]
    MissingThreadProtection,
    #[error("Windows GPU device creation failed: {0}")]
    Platform(#[from] windows::core::Error),
}

/// Identifies the selected adapter, not a resource generation or a process handle.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct WindowsAdapterId {
    low: u32,
    high: i32,
}

impl WindowsAdapterId {
    pub fn low(&self) -> u32 {
        self.low
    }
    pub fn high(&self) -> i32 {
        self.high
    }
}

/// REQ-PICOO-GPU-004: one immutable device/manager association per source context.
/// The platform owner starts and stops MF/COM on its own worker; this type never
/// transfers an apartment's CoUninitialize obligation to another thread.
pub struct WindowsGpuContext {
    manager: IMFDXGIDeviceManager,
    protection: ID3D11Multithread,
    immediate: ID3D11DeviceContext,
    device: ID3D11Device,
    adapter: WindowsAdapterId,
}

// SAFETY: These are native free-threaded D3D11/MF objects, with multithread
// protection enabled before publication. Raw mutation is confined to unsafe APIs.
unsafe impl Send for WindowsGpuContext {}
unsafe impl Sync for WindowsGpuContext {}

impl WindowsGpuContext {
    pub fn for_adapter(adapter: &IDXGIAdapter1) -> Result<Self, WindowsDeviceError> {
        let description = unsafe { adapter.GetDesc1()? };
        if description.Flags & DXGI_ADAPTER_FLAG_SOFTWARE.0 as u32 != 0 {
            return Err(WindowsDeviceError::SoftwareAdapter);
        }
        let mut device = None;
        let mut immediate = None;
        unsafe {
            D3D11CreateDevice(
                adapter,
                D3D_DRIVER_TYPE_UNKNOWN,
                HMODULE::default(),
                D3D11_CREATE_DEVICE_BGRA_SUPPORT | D3D11_CREATE_DEVICE_VIDEO_SUPPORT,
                Some(&[D3D_FEATURE_LEVEL_11_1, D3D_FEATURE_LEVEL_11_0]),
                D3D11_SDK_VERSION,
                Some(&mut device),
                None,
                Some(&mut immediate),
            )?;
        }
        let device = device.ok_or(WindowsDeviceError::MissingDevice)?;
        let immediate = immediate.ok_or(WindowsDeviceError::MissingDevice)?;
        let protection: ID3D11Multithread = immediate.cast()?;
        unsafe {
            // The return value is the previous state, not a success code.
            let _ = protection.SetMultithreadProtected(true);
        }
        if !unsafe { protection.GetMultithreadProtected() }.as_bool() {
            return Err(WindowsDeviceError::MissingThreadProtection);
        }
        let mut reset_token = 0;
        let mut manager = None;
        unsafe {
            MFCreateDXGIDeviceManager(&mut reset_token, &mut manager)?;
        }
        let manager = manager.ok_or(WindowsDeviceError::MissingDevice)?;
        unsafe {
            manager.ResetDevice(&device, reset_token)?;
        }
        Ok(Self {
            manager,
            protection,
            immediate,
            device,
            adapter: WindowsAdapterId {
                low: description.AdapterLuid.LowPart,
                high: description.AdapterLuid.HighPart,
            },
        })
    }

    pub fn adapter_id(&self) -> WindowsAdapterId {
        self.adapter
    }

    /// # Safety
    /// Keep this context alive while a transform uses its manager. Never call
    /// ResetDevice or otherwise change the device association through this borrow.
    pub unsafe fn device_manager(&self) -> &IMFDXGIDeviceManager {
        &self.manager
    }

    /// # Safety
    /// Do not disable context protection. Immediate-context command groups must
    /// use `with_immediate_context`; keep all referenced image leases until GPU completion.
    pub unsafe fn device(&self) -> &ID3D11Device {
        &self.device
    }

    /// Execute a command group without interleaving another user of the immediate context.
    /// This does not wait for GPU completion.
    ///
    /// # Safety
    /// Do not let the context escape, retain mutable source aliases, or release
    /// image owners before GPU completion. Do not wait on another worker while locked.
    pub unsafe fn with_immediate_context<R>(
        &self,
        submit: impl FnOnce(&ID3D11DeviceContext) -> R,
    ) -> R {
        self.protection.Enter();
        struct Guard<'a>(&'a ID3D11Multithread);
        impl Drop for Guard<'_> {
            fn drop(&mut self) {
                unsafe {
                    self.0.Leave();
                }
            }
        }
        let _guard = Guard(&self.protection);
        submit(&self.immediate)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use windows::Win32::Graphics::Dxgi::{
        CreateDXGIFactory2, IDXGIFactory4, DXGI_CREATE_FACTORY_FLAGS,
    };

    #[test]
    fn production_context_rejects_the_actual_warp_adapter() {
        let factory: IDXGIFactory4 =
            unsafe { CreateDXGIFactory2(DXGI_CREATE_FACTORY_FLAGS(0)) }.unwrap();
        let warp: IDXGIAdapter1 = unsafe { factory.EnumWarpAdapter() }.unwrap();
        assert!(matches!(
            WindowsGpuContext::for_adapter(&warp),
            Err(WindowsDeviceError::SoftwareAdapter)
        ));
    }
}
