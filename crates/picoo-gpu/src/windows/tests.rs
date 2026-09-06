use super::*;
use std::sync::{mpsc, Arc};
use std::time::Duration;
use windows::Win32::Graphics::Dxgi::{
    CreateDXGIFactory2, IDXGIFactory4, DXGI_CREATE_FACTORY_FLAGS,
};
use windows::Win32::Media::MediaFoundation::{MFShutdown, MFStartup, MFSTARTUP_FULL, MF_VERSION};
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
fn manager_returns_the_bound_device_and_panics_release_context_protection() {
    // WARP reaches the private device binding only to test platform ownership.
    // The public production constructor continues rejecting this adapter.
    let _runtime = Runtime::start();
    let context = diagnostic_context();
    let device = context.device.clone();
    assert!(unsafe { context.protection.GetMultithreadProtected() }.as_bool());
    unsafe {
        let manager = context.device_manager();
        let handle = manager.OpenDeviceHandle().unwrap();
        let mut raw = std::ptr::null_mut();
        let result = manager.GetVideoService(handle, &ID3D11Device::IID, &mut raw);
        manager.CloseDeviceHandle(handle).unwrap();
        result.unwrap();
        assert!(!raw.is_null());
        let associated = ID3D11Device::from_raw(raw);
        assert_eq!(associated, device, "MF must use the source GPU device");
    }
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
            MFStartup(MF_VERSION, MFSTARTUP_FULL).unwrap();
        }
        Self
    }
}
impl Drop for Runtime {
    fn drop(&mut self) {
        unsafe {
            let _ = MFShutdown();
            CoUninitialize();
        }
    }
}

pub(super) fn diagnostic_context() -> Arc<WindowsGpuContext> {
    let adapter = warp_adapter();
    let description = unsafe { adapter.GetDesc1() }.unwrap();
    let mut device = None;
    let mut immediate = None;
    unsafe {
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
        )
        .unwrap();
    }
    let device = device.unwrap();
    Arc::new(
        WindowsGpuContext::bind_device(
            WindowsAdapterId {
                low: description.AdapterLuid.LowPart,
                high: description.AdapterLuid.HighPart,
            },
            device.clone(),
            immediate.unwrap(),
        )
        .unwrap(),
    )
}
