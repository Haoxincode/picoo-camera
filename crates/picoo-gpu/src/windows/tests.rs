use super::*;
use std::sync::{mpsc, Arc};
use std::time::Duration;
use windows::Win32::Graphics::Dxgi::{
    CreateDXGIFactory2, IDXGIFactory4, DXGI_CREATE_FACTORY_FLAGS,
};
use windows::Win32::System::Com::{CoInitializeEx, CoUninitialize, COINIT_MULTITHREADED};

pub(super) static GPU_WORK_TEST_LOCK: std::sync::Mutex<()> = std::sync::Mutex::new(());

fn warp_adapter() -> IDXGIAdapter1 {
    let factory: IDXGIFactory4 =
        unsafe { CreateDXGIFactory2(DXGI_CREATE_FACTORY_FLAGS(0)) }.unwrap();
    unsafe { factory.EnumWarpAdapter() }.unwrap()
}

#[test]
fn production_context_rejects_the_actual_warp_adapter() {
    assert!(matches!(
        WindowsGpuContext::for_adapter(&warp_adapter()),
        Err(WindowsDeviceError::SoftwareAdapter)
    ));
}

#[test]
fn bound_context_keeps_device_identity_and_panics_release_protection() {
    // WARP reaches the private device binding only to test platform ownership.
    // The public production constructor continues rejecting this adapter.
    let _runtime = Runtime::start();
    let context = diagnostic_context();
    let device = context.device.clone();
    assert!(unsafe { context.protection.GetMultithreadProtected() }.as_bool());
    assert_eq!(unsafe { context.immediate.GetDevice() }.unwrap(), device);
    let failed = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| unsafe {
        context.with_immediate_context(|_| panic!("command-group cancellation fixture"));
    }));
    assert!(failed.is_err());
    let (send, received) = mpsc::sync_channel(1);
    let worker_context = Arc::clone(&context);
    let worker = std::thread::spawn(move || unsafe {
        worker_context.with_immediate_context(|_| send.send(()).unwrap());
    });
    received
        .recv_timeout(Duration::from_secs(3))
        .expect("panic leaked the native context lock");
    worker.join().unwrap();
}

pub(super) struct Runtime;
impl Runtime {
    pub(super) fn start() -> Self {
        unsafe {
            CoInitializeEx(None, COINIT_MULTITHREADED).ok().unwrap();
        }
        Self
    }
}
impl Drop for Runtime {
    fn drop(&mut self) {
        unsafe {
            CoUninitialize();
        }
    }
}

pub(super) fn diagnostic_context() -> Arc<WindowsGpuContext> {
    Arc::new(WindowsGpuContext::diagnostic().unwrap())
}

#[test]
fn adopting_an_existing_software_device_does_not_bypass_production_admission() {
    let context = diagnostic_context();
    assert!(matches!(
        unsafe { WindowsGpuContext::from_existing_device(context.device.clone()) },
        Err(WindowsDeviceError::SoftwareAdapter)
    ));
}

#[test]
fn adopting_a_single_threaded_device_is_rejected_before_protection_changes() {
    let mut device = None;
    unsafe {
        D3D11CreateDevice(
            None,
            windows::Win32::Graphics::Direct3D::D3D_DRIVER_TYPE_WARP,
            HMODULE::default(),
            D3D11_CREATE_DEVICE_BGRA_SUPPORT | D3D11_CREATE_DEVICE_SINGLETHREADED,
            Some(&[D3D_FEATURE_LEVEL_11_0]),
            D3D11_SDK_VERSION,
            Some(&mut device),
            None,
            None,
        )
        .unwrap();
        assert!(matches!(
            WindowsGpuContext::from_existing_device(device.unwrap()),
            Err(WindowsDeviceError::MissingThreadProtection)
        ));
    }
}
