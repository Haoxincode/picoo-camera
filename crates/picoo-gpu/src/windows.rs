//! Windows platform device ownership; codec admission remains the Decoder's job.
use windows::core::Interface;
use windows::Win32::Foundation::HMODULE;
use windows::Win32::Graphics::Direct3D::{
    D3D_DRIVER_TYPE_UNKNOWN, D3D_FEATURE_LEVEL_11_0, D3D_FEATURE_LEVEL_11_1,
};
use windows::Win32::Graphics::Direct3D11::{
    D3D11CreateDevice, ID3D11Device, ID3D11DeviceContext, ID3D11Multithread,
    D3D11_CREATE_DEVICE_BGRA_SUPPORT, D3D11_CREATE_DEVICE_SINGLETHREADED,
    D3D11_CREATE_DEVICE_VIDEO_SUPPORT, D3D11_SDK_VERSION,
};
use windows::Win32::Graphics::Dxgi::{IDXGIAdapter1, IDXGIDevice, DXGI_ADAPTER_FLAG_SOFTWARE};

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

/// REQ-PICOO-GPU-004: a fixed D3D11 device and protected submission context.
/// MF managers and MF/COM runtime belong to the Decoder, not GPU output workers.
pub struct WindowsGpuContext {
    protection: ID3D11Multithread,
    immediate: ID3D11DeviceContext,
    device: ID3D11Device,
    adapter: WindowsAdapterId,
    completion_slots: std::sync::Arc<std::sync::atomic::AtomicUsize>,
}

// SAFETY: These are native free-threaded D3D11 objects, with multithread
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
        Self::bind_device(
            WindowsAdapterId {
                low: description.AdapterLuid.LowPart,
                high: description.AdapterLuid.HighPart,
            },
            device,
            immediate,
        )
    }

    /// Adopt the exact source device; never create a replacement for a new sink.
    ///
    /// # Safety
    /// The caller must keep all device aliases under multithread protection and
    /// retain image owners through GPU completion. SINGLETHREADED devices are
    /// rejected; never disable protection after a successful adoption.
    pub unsafe fn from_existing_device(device: ID3D11Device) -> Result<Self, WindowsDeviceError> {
        if device.GetCreationFlags() & D3D11_CREATE_DEVICE_SINGLETHREADED.0 != 0 {
            return Err(WindowsDeviceError::MissingThreadProtection);
        }
        let dxgi: IDXGIDevice = device.cast()?;
        let adapter: IDXGIAdapter1 = dxgi.GetAdapter()?.cast()?;
        let description = adapter.GetDesc1()?;
        if description.Flags & DXGI_ADAPTER_FLAG_SOFTWARE.0 as u32 != 0 {
            return Err(WindowsDeviceError::SoftwareAdapter);
        }
        let immediate = device.GetImmediateContext()?;
        Self::bind_device(
            WindowsAdapterId {
                low: description.AdapterLuid.LowPart,
                high: description.AdapterLuid.HighPart,
            },
            device,
            immediate,
        )
    }

    fn bind_device(
        adapter: WindowsAdapterId,
        device: ID3D11Device,
        immediate: ID3D11DeviceContext,
    ) -> Result<Self, WindowsDeviceError> {
        let protection: ID3D11Multithread = immediate.cast()?;
        unsafe {
            // The return value is the previous state, not a success code.
            let _ = protection.SetMultithreadProtected(true);
        }
        if !unsafe { protection.GetMultithreadProtected() }.as_bool() {
            return Err(WindowsDeviceError::MissingThreadProtection);
        }
        Ok(Self {
            protection,
            immediate,
            device,
            adapter,
            completion_slots: std::sync::Arc::new(std::sync::atomic::AtomicUsize::new(0)),
        })
    }

    pub fn adapter_id(&self) -> WindowsAdapterId {
        self.adapter
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
mod tests;

mod completion;
pub use completion::{WindowsCompletionError, WindowsGpuCompletion};

mod render;
pub use render::{RenderedImage, WindowsRenderer};

mod cpu_export;
pub use cpu_export::CpuExporter;
