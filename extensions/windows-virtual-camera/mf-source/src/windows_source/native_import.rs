//! Frame Server-side admission for producer-owned D3D11 shared surfaces.
//!
//! This is deliberately separate from `RequestSample`: it validates an
//! already duplicated target-process HANDLE and exact manager device. The
//! NV12 surfaces are the negotiated native sample contract; BGRA admission is
//! retained for isolated interop probes and is never queued as an MF sample.

use picoo_frame_hub::{WindowsSharedSurfaceDescriptor, WindowsSharedSurfaceFormat};
use std::sync::{
    atomic::{AtomicBool, Ordering},
    Arc,
};
use windows::core::{implement, IUnknown, Interface, Result};
use windows::Win32::Foundation::{CloseHandle, HANDLE, S_OK};
use windows::Win32::Graphics::Direct3D11::{
    ID3D11Device1, ID3D11Texture2D, D3D11_RESOURCE_MISC_SHARED_KEYEDMUTEX,
    D3D11_RESOURCE_MISC_SHARED_NTHANDLE, D3D11_TEXTURE2D_DESC, D3D11_USAGE_DEFAULT,
};
use windows::Win32::Graphics::Dxgi::{
    Common::{DXGI_FORMAT, DXGI_FORMAT_B8G8R8A8_UNORM, DXGI_FORMAT_NV12},
    IDXGIKeyedMutex,
};
use windows::Win32::Media::MediaFoundation::{
    IMFMediaBuffer, IMFMediaBuffer_Impl, IMFSample, IMFVideoSampleAllocator,
    MFCreateDXGISurfaceBuffer, MFCreateSample,
};
use windows::Win32::System::Com::{IAgileObject, IAgileObject_Impl};
use windows::Win32::System::Threading::WAIT_TIMEOUT;

use super::d3d_manager::NativeDeviceBinding;

#[derive(Debug, thiserror::Error)]
pub(super) enum NativeImportError {
    #[error("native surface descriptor does not match the Frame Server device")]
    Contract,
    #[error("native surface handle could not be opened")]
    Open(#[source] windows::core::Error),
    #[error("native surface belongs to a different D3D11 device")]
    WrongDevice,
}

pub(super) struct ImportedNativeSurface {
    pub(super) texture: ID3D11Texture2D,
    pub(super) mutex: IDXGIKeyedMutex,
}

#[implement(IMFMediaBuffer, IAgileObject)]
struct KeyedMutexBuffer {
    inner: IMFMediaBuffer,
    mutex: IDXGIKeyedMutex,
    release_failed: Arc<AtomicBool>,
    release_ack: Option<Arc<dyn Fn() + Send + Sync>>,
}

impl IMFMediaBuffer_Impl for KeyedMutexBuffer_Impl {
    fn Lock(
        &self,
        ppbbuffer: *mut *mut u8,
        pcbmaxlength: *mut u32,
        pcbcurrentlength: *mut u32,
    ) -> Result<()> {
        unsafe {
            self.inner
                .Lock(ppbbuffer, Some(pcbmaxlength), Some(pcbcurrentlength))
        }
    }

    fn Unlock(&self) -> Result<()> {
        unsafe { self.inner.Unlock() }
    }

    fn GetCurrentLength(&self) -> Result<u32> {
        unsafe { self.inner.GetCurrentLength() }
    }

    fn SetCurrentLength(&self, cbcurrentlength: u32) -> Result<()> {
        unsafe { self.inner.SetCurrentLength(cbcurrentlength) }
    }

    fn GetMaxLength(&self) -> Result<u32> {
        unsafe { self.inner.GetMaxLength() }
    }
}

impl IAgileObject_Impl for KeyedMutexBuffer_Impl {}

impl Drop for KeyedMutexBuffer {
    fn drop(&mut self) {
        unsafe {
            let released = self.mutex.ReleaseSync(0).is_ok();
            if !released {
                self.release_failed.store(true, Ordering::Release);
            }
            if released {
                if let Some(release_ack) = &self.release_ack {
                    release_ack();
                }
            }
        }
    }
}

struct KeyedMutexGuard {
    mutex: Option<IDXGIKeyedMutex>,
    release_failed: Arc<AtomicBool>,
}

impl Drop for KeyedMutexGuard {
    fn drop(&mut self) {
        if let Some(mutex) = self.mutex.take() {
            unsafe {
                if mutex.ReleaseSync(0).is_err() {
                    self.release_failed.store(true, Ordering::Release);
                }
            }
        }
    }
}

pub(super) struct NativeSampleLease {
    pub(super) sample: IMFSample,
    release_failed: Arc<AtomicBool>,
}

impl NativeSampleLease {
    pub(super) fn sample(&self) -> &IMFSample {
        &self.sample
    }

    pub(super) fn release_status(&self) -> Arc<AtomicBool> {
        Arc::clone(&self.release_failed)
    }

    /// Drop this owner and return a status handle. Other COM clones may keep
    /// the buffer alive, so the status must be observed after the final clone
    /// is released rather than read synchronously here.
    pub(super) fn finish(self) -> Arc<AtomicBool> {
        let release_failed = Arc::clone(&self.release_failed);
        drop(self.sample);
        release_failed
    }
}

unsafe fn acquire_key_zero(mutex: &IDXGIKeyedMutex) -> Result<KeyedMutexGuard> {
    const KEYED_MUTEX_WAIT_MS: u32 = 250;
    let status =
        (Interface::vtable(mutex).AcquireSync)(Interface::as_raw(mutex), 0, KEYED_MUTEX_WAIT_MS);
    if status.0 == WAIT_TIMEOUT.0 as i32 {
        return Err(windows::core::Error::from(status));
    }
    if status == S_OK {
        Ok(KeyedMutexGuard {
            mutex: Some(mutex.clone()),
            release_failed: Arc::new(AtomicBool::new(false)),
        })
    } else {
        Err(windows::core::Error::from(status))
    }
}

/// Build an MF sample whose buffer keeps the keyed mutex lease alive until the
/// consumer releases its final COM reference. This is an isolated primitive;
/// the caller still owns lifecycle/generation admission and QueueEvent wiring.
pub(super) unsafe fn make_native_sample(
    imported: ImportedNativeSurface,
    sample_time_100ns: i64,
    sample_duration_100ns: i64,
    release_ack: Option<Arc<dyn Fn() + Send + Sync>>,
    allocator: Option<&IMFVideoSampleAllocator>,
) -> Result<NativeSampleLease> {
    let mut guard = acquire_key_zero(&imported.mutex)?;
    let surface_buffer =
        MFCreateDXGISurfaceBuffer(&ID3D11Texture2D::IID, &imported.texture, 0, false)?;
    let release_failed = Arc::clone(&guard.release_failed);
    let buffer: IMFMediaBuffer = KeyedMutexBuffer {
        inner: surface_buffer,
        mutex: guard.mutex.take().expect("keyed mutex guard is held"),
        release_failed: Arc::clone(&release_failed),
        release_ack,
    }
    .into();
    let sample = if let Some(allocator) = allocator {
        let sample = allocator.AllocateSample()?;
        sample.RemoveAllBuffers()?;
        sample
    } else {
        MFCreateSample()?
    };
    sample.AddBuffer(&buffer)?;
    sample.SetSampleTime(sample_time_100ns)?;
    sample.SetSampleDuration(sample_duration_100ns)?;
    Ok(NativeSampleLease {
        sample,
        release_failed,
    })
}

/// Open and validate a duplicated target-process handle. LUID is checked as a
/// fast admission gate; COM identity remains authoritative because two devices
/// on the same adapter do not share a completion domain.
pub(super) unsafe fn import_bgra_surface(
    binding: &NativeDeviceBinding,
    descriptor: WindowsSharedSurfaceDescriptor,
) -> std::result::Result<ImportedNativeSurface, NativeImportError> {
    import_surface(
        binding,
        descriptor,
        WindowsSharedSurfaceFormat::Bgra8,
        DXGI_FORMAT_B8G8R8A8_UNORM,
    )
}

/// Import the producer's negotiated NV12 target directly. This keeps the
/// Frame Server sample in the advertised NV12 contract without a CPU copy or
/// a BGRA readback/bridge.
pub(super) unsafe fn import_nv12_surface(
    binding: &NativeDeviceBinding,
    descriptor: WindowsSharedSurfaceDescriptor,
) -> std::result::Result<ImportedNativeSurface, NativeImportError> {
    import_surface(
        binding,
        descriptor,
        WindowsSharedSurfaceFormat::Nv12,
        DXGI_FORMAT_NV12,
    )
}

unsafe fn import_surface(
    binding: &NativeDeviceBinding,
    descriptor: WindowsSharedSurfaceDescriptor,
    expected_format: WindowsSharedSurfaceFormat,
    expected_dxgi_format: DXGI_FORMAT,
) -> std::result::Result<ImportedNativeSurface, NativeImportError> {
    if descriptor.format() != expected_format
        || descriptor.keyed_mutex_key() != 0
        || descriptor.adapter().low() != binding.adapter.low
        || descriptor.adapter().high() != binding.adapter.high
        || !descriptor.identity().is_valid()
    {
        return Err(NativeImportError::Contract);
    }
    let device: ID3D11Device1 = binding.device.cast().map_err(NativeImportError::Open)?;
    let target_handle = HANDLE(descriptor.handle_value() as usize as *mut _);
    let texture_result = device.OpenSharedResource1(target_handle);
    // The numeric handle belongs to this Frame Server process after the
    // producer's DuplicateHandle. The COM texture retains the resource; the
    // process handle itself must be closed on both success and failure.
    let close_result = CloseHandle(target_handle);
    let texture: ID3D11Texture2D = texture_result.map_err(NativeImportError::Open)?;
    close_result.map_err(NativeImportError::Open)?;
    let actual_device = texture.GetDevice().map_err(NativeImportError::Open)?;
    if actual_device
        .cast::<IUnknown>()
        .map_err(NativeImportError::Open)?
        != binding
            .device
            .cast::<IUnknown>()
            .map_err(NativeImportError::Open)?
    {
        return Err(NativeImportError::WrongDevice);
    }
    let mut description = D3D11_TEXTURE2D_DESC::default();
    texture.GetDesc(&mut description);
    let (width, height) = descriptor.size();
    let required_misc =
        (D3D11_RESOURCE_MISC_SHARED_NTHANDLE.0 | D3D11_RESOURCE_MISC_SHARED_KEYEDMUTEX.0) as u32;
    if description.Format != expected_dxgi_format
        || description.Width != width
        || description.Height != height
        || description.Usage != D3D11_USAGE_DEFAULT
        || description.CPUAccessFlags != 0
        || description.MipLevels != 1
        || description.ArraySize != 1
        || description.SampleDesc.Count != 1
        || description.SampleDesc.Quality != 0
        || description.MiscFlags & required_misc != required_misc
    {
        return Err(NativeImportError::Contract);
    }
    let mutex = texture.cast().map_err(NativeImportError::Open)?;
    Ok(ImportedNativeSurface { texture, mutex })
}
