use super::*;
use crate::windows::render::pool::OutputPool;
use crate::windows::tests::{diagnostic_context, Runtime, GPU_WORK_TEST_LOCK};
use crate::{CpuExporter, OutputColor, OutputFormat, RenderSpec, Rotation};
use std::os::windows::io::AsRawHandle;
use windows::Win32::Foundation::HANDLE;
use windows::Win32::Graphics::Direct3D11::*;
use windows::Win32::Graphics::Dxgi::Common::DXGI_FORMAT_B8G8R8A8_UNORM;

struct Writer {
    surface: Arc<Surface>,
    access: Option<SharedAccess>,
    view: ID3D11RenderTargetView,
}
// SAFETY: Free-threaded D3D11 views plus pool/access leases; released after GPU completion.
unsafe impl Send for Writer {}

struct Reader {
    _image: RenderedImage,
    mutex: IDXGIKeyedMutex,
    imported: ID3D11Texture2D,
    staging: ID3D11Texture2D,
}
// SAFETY: Same completed-owner contract as Writer, now on the importing device.
unsafe impl Send for Reader {}
impl Drop for Reader {
    fn drop(&mut self) {
        unsafe { self.mutex.ReleaseSync(0).unwrap() };
    }
}

#[test]
fn bgra_nt_target_crosses_devices_without_cpu_upload_and_keeps_pool_lease() {
    let _serial = GPU_WORK_TEST_LOCK.lock().unwrap();
    let _runtime = Runtime::start();
    let producer = diagnostic_context();
    let consumer = diagnostic_context();
    let spec = RenderSpec {
        width: 64,
        height: 32,
        rotation: Rotation::None,
        mirror: false,
        color: OutputColor::RgbFullG22Bt709,
        format: OutputFormat::Bgra8,
    };
    assert!(matches!(
        CpuExporter::new(producer.clone(), spec),
        Err(RenderError::UnsupportedOutputFormat)
    ));
    let mut pool = OutputPool::new(spec);
    unsafe {
        let surface = pool.acquire(&producer.device).unwrap();
        let identity = surface.texture.as_raw();
        let access = SharedAccess::acquire(&surface).unwrap();
        let device: ID3D11Device1 = consumer.device.cast().unwrap();
        let imported: ID3D11Texture2D = device
            .OpenSharedResource1(HANDLE(surface.shared.as_ref().unwrap().as_raw_handle()))
            .unwrap();
        let mutex: IDXGIKeyedMutex = imported.cast().unwrap();
        // WAIT_TIMEOUT is a positive HRESULT. The real acquisition helper must
        // reject it while the producer holds key 0, not use Result::is_ok.
        assert!(acquire_mutex(&mutex).is_err());
        let mut view = None;
        producer
            .device
            .CreateRenderTargetView(&surface.texture, None, Some(&mut view))
            .unwrap();
        let writer = producer
            .submit_owned(
                Writer {
                    surface,
                    access,
                    view: view.unwrap(),
                },
                |gpu, work| {
                    gpu.with_immediate_context(|context| {
                        context.ClearRenderTargetView(&work.view, &[1.0, 0.0, 0.0, 1.0])
                    });
                    Ok(())
                },
            )
            .unwrap()
            .wait_on_worker()
            .unwrap();
        drop(writer.access);
        let image = RenderedImage {
            surface: writer.surface,
            spec,
        };
        assert!(image.shared_bgra_handle().is_ok());
        acquire_mutex(&mutex).unwrap();
        let mut description = D3D11_TEXTURE2D_DESC::default();
        imported.GetDesc(&mut description);
        assert_eq!(description.Format, DXGI_FORMAT_B8G8R8A8_UNORM);
        description.Usage = D3D11_USAGE_STAGING;
        description.BindFlags = 0;
        description.MiscFlags = 0;
        description.CPUAccessFlags = D3D11_CPU_ACCESS_READ.0 as u32;
        let mut staging = None;
        consumer
            .device
            .CreateTexture2D(&description, None, Some(&mut staging))
            .unwrap();
        let copy = consumer
            .submit_owned(
                Reader {
                    _image: image,
                    mutex,
                    imported,
                    staging: staging.unwrap(),
                },
                |gpu, work| {
                    gpu.with_immediate_context(|context| {
                        context.CopyResource(&work.staging, &work.imported)
                    });
                    Ok(())
                },
            )
            .unwrap();
        let second = pool.acquire(&producer.device).unwrap();
        let third = pool.acquire(&producer.device).unwrap();
        assert!(matches!(
            pool.acquire(&producer.device),
            Err(RenderError::PoolFull)
        ));
        let copied = copy.wait_on_worker().unwrap();
        let mut mapped = D3D11_MAPPED_SUBRESOURCE::default();
        consumer
            .with_immediate_context(|context| {
                context.Map(
                    &copied.staging,
                    0,
                    D3D11_MAP_READ,
                    D3D11_MAP_FLAG_DO_NOT_WAIT.0 as u32,
                    Some(&mut mapped),
                )
            })
            .unwrap();
        for row in 0..32usize {
            let bytes = std::slice::from_raw_parts(
                mapped
                    .pData
                    .cast::<u8>()
                    .add(row * mapped.RowPitch as usize),
                64 * 4,
            );
            assert!(bytes.chunks_exact(4).all(|pixel| pixel == [0, 0, 255, 255]));
        }
        consumer.with_immediate_context(|context| context.Unmap(&copied.staging, 0));
        drop(copied);
        assert_eq!(
            pool.acquire(&producer.device).unwrap().texture.as_raw(),
            identity
        );
        drop((second, third));
    }
}
