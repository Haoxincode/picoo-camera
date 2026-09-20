//! Frame Server-side admission for producer-owned D3D11 shared surfaces.
//!
//! This is deliberately separate from `RequestSample`: it validates an
//! already duplicated target-process HANDLE and exact manager device. The
//! NV12 surfaces are the negotiated native sample contract.

use picoo_frame_hub::{WindowsSharedSurfaceDescriptor, WindowsSharedSurfaceFormat};
use std::os::windows::io::{AsRawHandle, BorrowedHandle};
use std::sync::{Arc, Mutex};
use windows::core::{implement, IUnknown, Interface, Ref, Result};
use windows::Win32::Foundation::{E_NOTIMPL, HANDLE, S_OK, WAIT_TIMEOUT};
use windows::Win32::Graphics::Direct3D11::{
    ID3D11Device1, ID3D11Texture2D, D3D11_RESOURCE_MISC_SHARED_KEYEDMUTEX,
    D3D11_RESOURCE_MISC_SHARED_NTHANDLE, D3D11_TEXTURE2D_DESC, D3D11_USAGE_DEFAULT,
};
use windows::Win32::Graphics::Dxgi::{
    Common::{DXGI_FORMAT, DXGI_FORMAT_NV12},
    IDXGIKeyedMutex,
};
use windows::Win32::Media::MediaFoundation::{
    IMFAsyncCallback, IMFAsyncCallback_Impl, IMFAsyncResult, IMFSample, IMFTrackedSample,
    MFCreateDXGISurfaceBuffer, MFCreateTrackedSample,
};
use windows::Win32::System::Com::{IAgileObject, IAgileObject_Impl};

use super::{d3d_manager::NativeDeviceBinding, ObjectTracker};

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

#[implement(IMFAsyncCallback, IAgileObject)]
struct NativeSampleRelease {
    state: Mutex<Option<NativeSampleReleaseState>>,
    _tracker: ObjectTracker,
}

struct NativeSampleReleaseState {
    mutex: IDXGIKeyedMutex,
    release_ack: Option<Arc<dyn Fn(bool) + Send + Sync>>,
}

impl NativeSampleRelease {
    fn finish(&self) {
        let state = match self.state.lock() {
            Ok(mut state) => state.take(),
            Err(poisoned) => poisoned.into_inner().take(),
        };
        let Some(state) = state else {
            return;
        };
        let released = unsafe { state.mutex.ReleaseSync(0).is_ok() };
        if let Some(release_ack) = state.release_ack {
            release_ack(released);
        }
    }
}

impl IMFAsyncCallback_Impl for NativeSampleRelease_Impl {
    fn GetParameters(&self, _flags: *mut u32, _queue: *mut u32) -> Result<()> {
        Err(windows::core::Error::from(E_NOTIMPL))
    }

    fn Invoke(&self, _result: Ref<'_, IMFAsyncResult>) -> Result<()> {
        self.finish();
        Ok(())
    }
}

impl IAgileObject_Impl for NativeSampleRelease_Impl {}

impl Drop for NativeSampleRelease {
    fn drop(&mut self) {
        self.finish();
    }
}

struct KeyedMutexGuard {
    mutex: Option<IDXGIKeyedMutex>,
}

impl Drop for KeyedMutexGuard {
    fn drop(&mut self) {
        if let Some(mutex) = self.mutex.take() {
            unsafe {
                let _ = mutex.ReleaseSync(0);
            }
        }
    }
}

pub(super) struct NativeSampleLease {
    pub(super) sample: IMFSample,
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
        })
    } else {
        Err(windows::core::Error::from(status))
    }
}

/// Build a tracked MF sample whose release callback keeps the keyed mutex
/// lease alive until the consumer releases its final sample reference. The
/// official DXGI buffer remains directly visible through the sample.
pub(super) unsafe fn make_native_sample(
    imported: ImportedNativeSurface,
    sample_time_100ns: i64,
    sample_duration_100ns: i64,
    release_ack: Option<Arc<dyn Fn(bool) + Send + Sync>>,
) -> Result<NativeSampleLease> {
    let mut guard = acquire_key_zero(&imported.mutex)?;
    let surface_buffer =
        MFCreateDXGISurfaceBuffer(&ID3D11Texture2D::IID, &imported.texture, 0, false)?;
    // Keep the official DXGI buffer object directly on the sample so its
    // IMFDXGIBuffer/IMF2DBuffer interfaces remain visible to Frame Server.
    let tracked: IMFTrackedSample = MFCreateTrackedSample()?;
    let sample: IMFSample = tracked.cast()?;
    let release: IMFAsyncCallback = NativeSampleRelease {
        state: Mutex::new(Some(NativeSampleReleaseState {
            mutex: guard.mutex.take().expect("keyed mutex guard is held"),
            release_ack,
        })),
        _tracker: ObjectTracker::new(),
    }
    .into();
    tracked.SetAllocator(&release, None)?;
    sample.AddBuffer(&surface_buffer)?;
    sample.SetSampleTime(sample_time_100ns)?;
    sample.SetSampleDuration(sample_duration_100ns)?;
    Ok(NativeSampleLease { sample })
}

/// Open and validate a duplicated target-process handle. LUID is checked as a
/// fast admission gate; COM identity remains authoritative because two devices
/// on the same adapter do not share a completion domain.
/// Import the producer's negotiated NV12 target directly. This keeps the
/// Frame Server sample in the advertised NV12 contract without a CPU copy or
/// a BGRA readback/bridge.
pub(super) unsafe fn import_nv12_surface(
    binding: &NativeDeviceBinding,
    descriptor: WindowsSharedSurfaceDescriptor,
    target_handle: BorrowedHandle<'_>,
) -> std::result::Result<ImportedNativeSurface, NativeImportError> {
    import_surface(
        binding,
        descriptor,
        target_handle,
        WindowsSharedSurfaceFormat::Nv12,
        DXGI_FORMAT_NV12,
    )
}

unsafe fn import_surface(
    binding: &NativeDeviceBinding,
    descriptor: WindowsSharedSurfaceDescriptor,
    target_handle: BorrowedHandle<'_>,
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
    let target_handle = HANDLE(target_handle.as_raw_handle());
    let texture: ID3D11Texture2D = device
        .OpenSharedResource1(target_handle)
        .map_err(NativeImportError::Open)?;
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
