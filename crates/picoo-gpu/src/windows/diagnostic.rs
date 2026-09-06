//! Explicit test-support device factory, never a production admission fallback.
use super::*;
use windows::Win32::Graphics::Dxgi::{
    CreateDXGIFactory2, IDXGIFactory4, DXGI_CREATE_FACTORY_FLAGS,
};

impl WindowsGpuContext {
    /// A WARP resource/completion fixture. Product factories still reject software.
    pub fn diagnostic() -> Result<Self, WindowsDeviceError> {
        unsafe {
            let factory: IDXGIFactory4 = CreateDXGIFactory2(DXGI_CREATE_FACTORY_FLAGS(0))?;
            let adapter: IDXGIAdapter1 = factory.EnumWarpAdapter()?;
            let description = adapter.GetDesc1()?;
            let mut device = None;
            let mut immediate = None;
            D3D11CreateDevice(
                &adapter,
                D3D_DRIVER_TYPE_UNKNOWN,
                HMODULE::default(),
                D3D11_CREATE_DEVICE_BGRA_SUPPORT,
                Some(&[D3D_FEATURE_LEVEL_11_0]),
                D3D11_SDK_VERSION,
                Some(&mut device),
                None,
                Some(&mut immediate),
            )?;
            Self::bind_device(
                WindowsAdapterId {
                    low: description.AdapterLuid.LowPart,
                    high: description.AdapterLuid.HighPart,
                },
                device.ok_or(WindowsDeviceError::MissingDevice)?,
                immediate.ok_or(WindowsDeviceError::MissingDevice)?,
            )
        }
    }
}
