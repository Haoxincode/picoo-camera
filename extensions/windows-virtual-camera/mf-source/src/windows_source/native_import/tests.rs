use super::*;
use windows::Win32::Foundation::HMODULE;
use windows::Win32::Graphics::Direct3D::{D3D_DRIVER_TYPE_WARP, D3D_FEATURE_LEVEL_11_0};
use windows::Win32::Graphics::Direct3D11::{
    D3D11CreateDevice, D3D11_CREATE_DEVICE_BGRA_SUPPORT, D3D11_SDK_VERSION,
};
use windows::Win32::Graphics::Dxgi::Common::DXGI_SAMPLE_DESC;
use windows::Win32::Media::MediaFoundation::{
    IMFDXGIBuffer, MFShutdown, MFStartup, MFSTARTUP_FULL, MF_VERSION,
};
use windows::Win32::System::Com::{CoInitializeEx, CoUninitialize, COINIT_MULTITHREADED};

// REQ-PICOO-VCAM-012: exercise the production DXGI sample boundary using
// Windows' software device; this does not assert Frame Server compatibility.
#[test]
fn native_nv12_sample_exposes_valid_length_and_original_surface() {
    unsafe {
        CoInitializeEx(None, COINIT_MULTITHREADED).unwrap();
        MFStartup(MF_VERSION, MFSTARTUP_FULL).unwrap();
        {
            let mut device = None;
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
            let device = device.unwrap();
            for (width, height) in [(1280, 720), (1920, 1080)] {
                let description = D3D11_TEXTURE2D_DESC {
                    Width: width,
                    Height: height,
                    MipLevels: 1,
                    ArraySize: 1,
                    Format: DXGI_FORMAT_NV12,
                    SampleDesc: DXGI_SAMPLE_DESC {
                        Count: 1,
                        Quality: 0,
                    },
                    Usage: D3D11_USAGE_DEFAULT,
                    MiscFlags: (D3D11_RESOURCE_MISC_SHARED_NTHANDLE
                        | D3D11_RESOURCE_MISC_SHARED_KEYEDMUTEX)
                        .0 as u32,
                    ..Default::default()
                };
                let mut texture = None;
                device
                    .CreateTexture2D(&description, None, Some(&mut texture))
                    .unwrap();
                let texture = texture.unwrap();
                let lease = make_native_sample(
                    ImportedNativeSurface {
                        texture: texture.clone(),
                        mutex: texture.cast().unwrap(),
                    },
                    1_000_000,
                    333_333,
                    None,
                )
                .unwrap();
                let buffer = lease.sample.GetBufferByIndex(0).unwrap();
                let expected = width * height * 3 / 2;
                assert_eq!(buffer.GetCurrentLength().unwrap(), expected);
                assert_eq!(lease.sample.GetTotalLength().unwrap(), expected);
                assert_eq!(lease.sample.GetSampleTime().unwrap(), 1_000_000);
                assert_eq!(lease.sample.GetSampleDuration().unwrap(), 333_333);
                let dxgi: IMFDXGIBuffer = buffer.cast().unwrap();
                let mut resource = std::ptr::null_mut();
                dxgi.GetResource(&ID3D11Texture2D::IID, &mut resource)
                    .unwrap();
                let retained = ID3D11Texture2D::from_raw(resource);
                assert_eq!(
                    retained.cast::<IUnknown>().unwrap(),
                    texture.cast::<IUnknown>().unwrap()
                );
            }
        }
        MFShutdown().unwrap();
        CoUninitialize();
    }
}
