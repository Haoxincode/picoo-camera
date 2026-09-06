use super::*;
use std::sync::atomic::{AtomicUsize, Ordering};
use windows::core::{implement, GUID};
use windows::Win32::Foundation::HMODULE;
use windows::Win32::Graphics::Direct3D::{D3D_DRIVER_TYPE_WARP, D3D_FEATURE_LEVEL_11_0};
use windows::Win32::Graphics::Direct3D11::{
    D3D11CreateDevice, ID3D11Device, D3D11_CREATE_DEVICE_BGRA_SUPPORT, D3D11_SDK_VERSION,
};
use windows::Win32::Graphics::Dxgi::Common::{
    DXGI_FORMAT, DXGI_FORMAT_B8G8R8A8_UNORM, DXGI_SAMPLE_DESC,
};
use windows::Win32::Media::MediaFoundation::{
    MFCreateDXGISurfaceBuffer, MFCreateMemoryBuffer, MFCreateSample, MFShutdown, MFStartup,
    MFSTARTUP_FULL, MF_VERSION,
};
use windows::Win32::System::Com::{
    CoInitializeEx, CoUninitialize, IAgileObject, IAgileObject_Impl, COINIT_MULTITHREADED,
};

struct Runtime;
impl Runtime {
    fn start() -> Self {
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
            MFShutdown().unwrap();
            CoUninitialize();
        }
    }
}

#[implement(IAgileObject)]
struct SampleMarker(Arc<AtomicUsize>);
impl IAgileObject_Impl for SampleMarker_Impl {}
impl Drop for SampleMarker {
    fn drop(&mut self) {
        self.0.fetch_add(1, Ordering::SeqCst);
    }
}

fn diagnostic_device() -> ID3D11Device {
    let mut device = None;
    // WARP is a resource/COM contract fixture only, never a production device factory.
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
    device.unwrap()
}

fn sample(device: &ID3D11Device, format: DXGI_FORMAT) -> IMFSample {
    let description = D3D11_TEXTURE2D_DESC {
        Width: 64,
        Height: 64,
        MipLevels: 1,
        ArraySize: 1,
        Format: format,
        SampleDesc: DXGI_SAMPLE_DESC {
            Count: 1,
            Quality: 0,
        },
        Usage: D3D11_USAGE_DEFAULT,
        ..Default::default()
    };
    unsafe {
        let mut texture = None;
        device
            .CreateTexture2D(&description, None, Some(&mut texture))
            .unwrap();
        let texture = texture.unwrap();
        let buffer = MFCreateDXGISurfaceBuffer(&ID3D11Texture2D::IID, &texture, 0, false).unwrap();
        let sample = MFCreateSample().unwrap();
        sample.AddBuffer(&buffer).unwrap();
        sample
    }
}

#[test]
fn image_retains_the_mf_sample_until_last_cross_thread_clone() {
    let _runtime = Runtime::start();
    let device = diagnostic_device();
    let sample = sample(&device, DXGI_FORMAT_NV12);
    let released = Arc::new(AtomicUsize::new(0));
    let marker: IAgileObject = SampleMarker(Arc::clone(&released)).into();
    unsafe {
        sample
            .SetUnknown(
                &GUID::from_u128(0x9a3935fdb1ec4499861726fc9b48ed15),
                &marker,
            )
            .unwrap();
    }
    drop(marker);
    // No writes have been submitted and this test never mutates retained storage.
    let image = unsafe { D3D11ImageLease::retain_completed(&sample, &device) }.unwrap();
    let retained = image.clone();
    drop(sample);
    drop(image);
    assert_eq!(released.load(Ordering::SeqCst), 0);
    std::thread::spawn(move || {
        assert_eq!((retained.width(), retained.height()), (64, 64));
        let (_, subresource) = unsafe { retained.texture() };
        assert_eq!(subresource, 0);
        drop(retained);
    })
    .join()
    .unwrap();
    assert_eq!(
        released.load(Ordering::SeqCst),
        1,
        "texture-only retention loses the sample lease"
    );
}

#[test]
fn cpu_buffers_and_non_nv12_surfaces_are_not_native_sources() {
    let _runtime = Runtime::start();
    let device = diagnostic_device();
    let wrong_format = sample(&device, DXGI_FORMAT_B8G8R8A8_UNORM);
    assert!(matches!(
        unsafe { D3D11ImageLease::retain_completed(&wrong_format, &device) },
        Err(NativeImageError::UnsupportedStorage)
    ));
    unsafe {
        let cpu = MFCreateSample().unwrap();
        assert!(D3D11ImageLease::retain_completed(&cpu, &device).is_err());
        cpu.AddBuffer(&MFCreateMemoryBuffer(64 * 64 * 3 / 2).unwrap())
            .unwrap();
        assert!(D3D11ImageLease::retain_completed(&cpu, &device).is_err());
    }
}

#[test]
fn another_device_on_the_same_adapter_is_rejected() {
    let _runtime = Runtime::start();
    let device = diagnostic_device();
    let other_device = diagnostic_device();
    let output = sample(&device, DXGI_FORMAT_NV12);
    assert!(matches!(
        unsafe { D3D11ImageLease::retain_completed(&output, &other_device) },
        Err(NativeImageError::WrongDevice)
    ));
    // Rejection must not consume or modify the producer's sample.
    let image = unsafe { D3D11ImageLease::retain_completed(&output, &device) }.unwrap();
    assert_eq!((image.width(), image.height()), (64, 64));
}
