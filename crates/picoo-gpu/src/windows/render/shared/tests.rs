use super::*;
use crate::windows::render::pool::OutputPool;
use crate::windows::tests::{diagnostic_context, Runtime, GPU_WORK_TEST_LOCK};
use crate::{CpuExporter, OutputColor, OutputFormat, RenderSpec, Rotation};
use std::os::windows::io::{AsRawHandle, BorrowedHandle};
use windows::Win32::Foundation::{CloseHandle, HANDLE};
use windows::Win32::Graphics::Direct3D11::*;
use windows::Win32::Graphics::Dxgi::Common::DXGI_FORMAT_B8G8R8A8_UNORM;
use windows::Win32::System::Threading::GetCurrentProcess;

struct Nv12Upload {
    surface: Arc<Surface>,
    _access: Option<SharedAccess>,
    pixels: Vec<u8>,
}
// SAFETY: The upload bytes, texture and keyed ownership survive GPU completion.
unsafe impl Send for Nv12Upload {}

#[test]
fn cpu_export_reads_shared_nv12_only_while_it_owns_key_zero() {
    let _serial = GPU_WORK_TEST_LOCK.lock().unwrap();
    let _runtime = Runtime::start();
    let gpu = diagnostic_context();
    let spec = RenderSpec {
        width: 64,
        height: 32,
        rotation: Rotation::None,
        mirror: false,
        color: OutputColor::Bt709Limited,
        format: OutputFormat::Nv12,
    };
    let mut expected = vec![96u8; (spec.width * spec.height) as usize];
    expected.extend((0..spec.width * spec.height / 2).map(|i| if i % 2 == 0 { 80 } else { 190 }));
    let mut pool = OutputPool::new(spec);
    unsafe {
        let surface = pool.acquire(&gpu.device).unwrap();
        let access = SharedAccess::acquire(&surface).unwrap();
        let upload = gpu
            .submit_owned(
                Nv12Upload {
                    surface,
                    _access: access,
                    pixels: expected.clone(),
                },
                |gpu, work| {
                    gpu.with_immediate_context(|context| {
                        context.UpdateSubresource(
                            &work.surface.texture,
                            0,
                            None,
                            work.pixels.as_ptr().cast(),
                            spec.width,
                            work.pixels.len() as u32,
                        );
                    });
                    Ok(())
                },
            )
            .unwrap()
            .wait_on_worker()
            .unwrap();
        let image = RenderedImage {
            surface: Arc::clone(&upload.surface),
            spec,
        };
        drop(upload);
        let mutex: IDXGIKeyedMutex = image.surface.texture.cast().unwrap();
        // Leave the allocation unowned but available only at key 1. An exporter
        // that skips AcquireSync would incorrectly submit a copy and report success.
        acquire_mutex(&mutex).unwrap();
        mutex.ReleaseSync(1).unwrap();
        let mut exporter = CpuExporter::new(gpu, spec).unwrap();
        assert!(matches!(
            exporter.export(&image),
            Err(RenderError::SharedSurfaceBusy)
        ));
        assert_eq!(exporter.exports(), 0);
        mutex.AcquireSync(1, 0).unwrap();
        mutex.ReleaseSync(0).unwrap();
        assert_eq!(exporter.export(&image).unwrap().pixels(), expected);
        assert_eq!(exporter.exports(), 1);
        // Successful export releases its access lease for the next consumer.
        acquire_mutex(&mutex).unwrap();
        mutex.ReleaseSync(0).unwrap();
    }
}

#[test]
fn native_identity_allows_initial_zero_decoder_generation() {
    assert!(WindowsSharedSurfaceIdentity {
        source_connection_generation: 1,
        stream_epoch: 1,
        decoder_generation: 0,
        source_frame_id: 0,
        resource_generation: 1,
        backend_generation: 1,
        output_revision: 0,
    }
    .is_valid());
    assert!(!WindowsSharedSurfaceIdentity {
        source_connection_generation: 1,
        stream_epoch: 1,
        decoder_generation: 0,
        source_frame_id: 0,
        resource_generation: 0,
        backend_generation: 1,
        output_revision: 0,
    }
    .is_valid());
}

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
#[ignore = "requires a hardware D3D11 adapter; the diagnostic WARP device is intentionally rejected"]
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
        let texture_identity = surface.texture.as_raw();
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
        let target_process = BorrowedHandle::borrow_raw(GetCurrentProcess().0);
        let transfer = image
            .duplicate_shared_bgra_handle_into(
                target_process,
                WindowsSharedSurfaceIdentity {
                    source_connection_generation: 1,
                    stream_epoch: 2,
                    decoder_generation: 3,
                    source_frame_id: 4,
                    resource_generation: 5,
                    backend_generation: 6,
                    output_revision: 7,
                },
            )
            .unwrap();
        let handoff_identity = transfer.descriptor().unwrap().identity();
        assert_eq!(handoff_identity.source_frame_id, 4);
        let lease = transfer.commit();
        let descriptor = *lease.descriptor();
        assert_eq!(descriptor.size(), (64, 32));
        assert_eq!(descriptor.keyed_mutex_key(), 0);
        assert_eq!(descriptor.format(), WindowsSharedSurfaceFormat::Bgra8);
        let duplicated_handle = HANDLE(descriptor.handle_value() as usize as *mut std::ffi::c_void);
        let duplicated_texture: ID3D11Texture2D = consumer
            .device
            .cast::<ID3D11Device1>()
            .unwrap()
            .OpenSharedResource1(duplicated_handle)
            .unwrap();
        CloseHandle(duplicated_handle).unwrap();
        let mut duplicated_description = D3D11_TEXTURE2D_DESC::default();
        duplicated_texture.GetDesc(&mut duplicated_description);
        assert_eq!(duplicated_description.Format, DXGI_FORMAT_B8G8R8A8_UNORM);
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
            assert!(bytes
                .as_chunks::<4>()
                .0
                .iter()
                .all(|pixel| *pixel == [0, 0, 255, 255]));
        }
        consumer.with_immediate_context(|context| context.Unmap(&copied.staging, 0));
        drop(copied);
        assert!(matches!(
            pool.acquire(&producer.device),
            Err(RenderError::PoolFull)
        ));
        drop(lease);
        drop((second, third));
        assert_eq!(
            pool.acquire(&producer.device).unwrap().texture.as_raw(),
            texture_identity
        );
    }
}
