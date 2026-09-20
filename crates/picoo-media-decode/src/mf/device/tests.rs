use super::*;
use windows::Win32::Foundation::HMODULE;
use windows::Win32::Graphics::Direct3D::{D3D_DRIVER_TYPE_WARP, D3D_FEATURE_LEVEL_11_0};
use windows::Win32::Graphics::Direct3D11::{
    D3D11CreateDevice, D3D11_CREATE_DEVICE_BGRA_SUPPORT, D3D11_SDK_VERSION,
};

#[test]
fn decoder_manager_returns_the_original_device() {
    let _runtime = super::super::MfRuntimeGuard::start().unwrap();
    // WARP tests only the official manager association, not hardware admission.
    let mut device = None;
    unsafe {
        D3D11CreateDevice(
            None,
            D3D_DRIVER_TYPE_WARP,
            HMODULE::default(),
            D3D11_CREATE_DEVICE_BGRA_SUPPORT,
            Some(&[D3D_FEATURE_LEVEL_11_0]),
            D3D11_SDK_VERSION,
            Some(&mut device),
            None,
            None,
        )
        .unwrap();
    }
    let device = device.unwrap();
    let manager = create_manager(&device).unwrap();
    unsafe {
        let handle = manager.OpenDeviceHandle().unwrap();
        let mut raw = std::ptr::null_mut();
        let result = manager.GetVideoService(handle, &ID3D11Device::IID, &mut raw);
        manager.CloseDeviceHandle(handle).unwrap();
        result.unwrap();
        assert!(!raw.is_null());
        let associated = ID3D11Device::from_raw(raw);
        assert_eq!(associated, device);
    }
}
