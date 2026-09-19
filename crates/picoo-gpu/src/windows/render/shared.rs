//! Official NT shared textures; ownership and keyed access are both required.
use super::{platform, RenderedImage, Surface};
use crate::RenderError;
use picoo_frame_hub::{
    WindowsAdapterId, WindowsSharedSurfaceDescriptor, WindowsSharedSurfaceFormat,
    WindowsSharedSurfaceIdentity,
};
use std::os::windows::io::{AsHandle, AsRawHandle, BorrowedHandle, FromRawHandle, OwnedHandle};
use std::sync::{atomic::Ordering, Arc};
use windows::core::Interface;
use windows::Win32::Foundation::{
    CloseHandle, DuplicateHandle, DUPLICATE_CLOSE_SOURCE, DUPLICATE_HANDLE_OPTIONS, HANDLE, S_OK,
    WAIT_TIMEOUT,
};
use windows::Win32::Graphics::Direct3D11::ID3D11Texture2D;
use windows::Win32::Graphics::Dxgi::{
    IDXGIAdapter1, IDXGIDevice, IDXGIKeyedMutex, IDXGIResource1, DXGI_ADAPTER_FLAG_SOFTWARE,
    DXGI_SHARED_RESOURCE_READ, DXGI_SHARED_RESOURCE_WRITE,
};
use windows::Win32::System::Threading::GetCurrentProcess;

// The descriptor/identity types live in FrameHub so the producer and future
// Frame Server importer cannot silently drift in field meaning.

/// Owns the target-process handle until the control channel acknowledges that
/// the descriptor was received. Dropping an uncommitted transfer closes the
/// duplicated handle in the target process; `commit` transfers that ownership
/// to the consumer, which must close it after importing the resource.
pub struct WindowsSharedSurfaceTransfer<'a> {
    target_process: BorrowedHandle<'a>,
    descriptor: Option<WindowsSharedSurfaceDescriptor>,
    surface: Option<Arc<Surface>>,
}

/// Owns the producer surface for the duration of a consumer GPU read.
/// Dropping this lease is the producer-side acknowledgement that the target
/// has finished using the imported resource and released keyed mutex key 0.
pub struct WindowsSharedSurfaceLease {
    descriptor: WindowsSharedSurfaceDescriptor,
    _surface: Arc<Surface>,
}

impl WindowsSharedSurfaceLease {
    pub fn descriptor(&self) -> &WindowsSharedSurfaceDescriptor {
        &self.descriptor
    }
}

impl<'a> WindowsSharedSurfaceTransfer<'a> {
    /// Borrow the descriptor until `commit` or drop. Copying the raw value and
    /// then dropping the transfer leaves a stale target-process handle.
    pub fn descriptor(&self) -> Option<&WindowsSharedSurfaceDescriptor> {
        self.descriptor.as_ref()
    }

    pub fn commit(mut self) -> WindowsSharedSurfaceLease {
        let descriptor = self
            .descriptor
            .take()
            .expect("shared surface transfer already committed");
        let surface = self
            .surface
            .take()
            .expect("shared surface transfer missing producer lease");
        WindowsSharedSurfaceLease {
            descriptor,
            _surface: surface,
        }
    }
}

impl Drop for WindowsSharedSurfaceTransfer {
    fn drop(&mut self) {
        let Some(descriptor) = self.descriptor.take() else {
            return;
        };
        // The handle value belongs to target_process, so CloseHandle in this
        // producer process would be invalid.
        unsafe { close_remote_handle(self.target_process, descriptor.handle_value()) };
    }
}

unsafe fn close_remote_handle(target_process: BorrowedHandle<'_>, handle_value: u64) {
    // DUPLICATE_CLOSE_SOURCE closes the remote value and gives us a local
    // duplicate that can then be closed normally.
    let mut local = HANDLE::default();
    if DuplicateHandle(
        HANDLE(target_process.as_raw_handle()),
        HANDLE(handle_value as usize as *mut std::ffi::c_void),
        GetCurrentProcess(),
        &mut local,
        0,
        false,
        DUPLICATE_CLOSE_SOURCE,
    )
    .is_ok()
        && !local.0.is_null()
    {
        let _ = CloseHandle(local);
    }
}

impl RenderedImage {
    /// Borrow the completed opaque BGRA display target's NT sharing handle.
    ///
    /// # Safety
    /// The consumer must open it on the same adapter, acquire keyed mutex key 0,
    /// and release key 0 only after its GPU reads complete. Only S_OK from the
    /// raw AcquireSync HRESULT grants access: WAIT_TIMEOUT/WAIT_ABANDONED do not.
    /// Never modify the image. Retain this image (not just a duplicated handle or
    /// imported texture) until the last read and mutex release, including failure.
    pub unsafe fn shared_bgra_handle(&self) -> Result<BorrowedHandle<'_>, RenderError> {
        if self.spec.format != crate::OutputFormat::Bgra8 {
            return Err(RenderError::UnsupportedOutputFormat);
        }
        self.surface
            .shared
            .as_ref()
            .map(AsHandle::as_handle)
            .ok_or(RenderError::UnsupportedOutputFormat)
    }

    /// Duplicate the producer's NT handle into an already-authenticated target
    /// process with read-only shared-resource access. The returned value is
    /// meaningful only in that target process; it must be transferred over
    /// the protected control channel together with the identity, adapter and
    /// size metadata. This function does not establish keyed-mutex ownership;
    /// committing the transfer yields a lease retaining the producer surface.
    ///
    /// # Safety
    /// `target_process` must be a valid process handle with
    /// `PROCESS_DUP_HANDLE`, obtained after the control peer was authenticated.
    /// The caller must retain this image until the target has finished its GPU
    /// read and released key 0.
    pub unsafe fn duplicate_shared_bgra_handle_into<'target>(
        &self,
        target_process: BorrowedHandle<'target>,
        identity: WindowsSharedSurfaceIdentity,
    ) -> Result<WindowsSharedSurfaceTransfer<'target>, RenderError> {
        self.duplicate_shared_handle_into(
            target_process,
            identity,
            WindowsSharedSurfaceFormat::Bgra8,
        )
    }

    /// Duplicate an NV12 or BGRA shared target into an authenticated peer.
    /// The format is part of the descriptor and is checked against the
    /// completed RenderSpec before the handle is transferred.
    pub unsafe fn duplicate_shared_handle_into<'target>(
        &self,
        target_process: BorrowedHandle<'target>,
        identity: WindowsSharedSurfaceIdentity,
        format: WindowsSharedSurfaceFormat,
    ) -> Result<WindowsSharedSurfaceTransfer<'target>, RenderError> {
        if !identity.is_valid() {
            return Err(RenderError::Platform(
                "invalid native surface handoff identity".into(),
            ));
        }
        let device = self.surface.texture.GetDevice().map_err(platform)?;
        let dxgi: IDXGIDevice = device.cast().map_err(platform)?;
        let adapter: IDXGIAdapter1 = dxgi
            .GetAdapter()
            .map_err(platform)?
            .cast()
            .map_err(platform)?;
        let description = adapter.GetDesc1().map_err(platform)?;
        if description.Flags & DXGI_ADAPTER_FLAG_SOFTWARE.0 as u32 != 0 {
            return Err(RenderError::DeviceUnavailable);
        }
        let mut texture_desc = Default::default();
        self.surface.texture.GetDesc(&mut texture_desc);

        if (format == WindowsSharedSurfaceFormat::Bgra8
            && self.spec.format != crate::OutputFormat::Bgra8)
            || (format == WindowsSharedSurfaceFormat::Nv12
                && self.spec.format != crate::OutputFormat::Nv12)
        {
            return Err(RenderError::UnsupportedOutputFormat);
        }
        let handle = self
            .surface
            .shared
            .as_ref()
            .map(AsHandle::as_handle)
            .ok_or(RenderError::UnsupportedOutputFormat)?;
        let producer_handle = HANDLE(handle.as_raw_handle());
        let mut target_handle = HANDLE::default();
        DuplicateHandle(
            GetCurrentProcess(),
            producer_handle,
            HANDLE(target_process.as_raw_handle()),
            &mut target_handle,
            DXGI_SHARED_RESOURCE_READ.0,
            false,
            DUPLICATE_HANDLE_OPTIONS(0),
        )
        .map_err(platform)?;
        if target_handle.0.is_null() {
            return Err(RenderError::DeviceUnavailable);
        }
        let descriptor = match WindowsSharedSurfaceDescriptor::new(
            target_handle.0 as usize as u64,
            WindowsAdapterId::from_luid(
                description.AdapterLuid.LowPart,
                description.AdapterLuid.HighPart,
            ),
            texture_desc.Width,
            texture_desc.Height,
            format,
            0,
            identity,
        ) {
            Some(descriptor) => descriptor,
            None => {
                close_remote_handle(target_process, target_handle.0 as usize as u64);
                return Err(RenderError::DeviceUnavailable);
            }
        };
        Ok(WindowsSharedSurfaceTransfer {
            target_process,
            descriptor: Some(descriptor),
            surface: Some(Arc::clone(&self.surface)),
        })
    }
}

pub(super) unsafe fn create_handle(texture: &ID3D11Texture2D) -> Result<OwnedHandle, RenderError> {
    // Exactly once per pool allocation. Additional consumers duplicate this NT
    // handle; GetSharedHandle is the incompatible legacy handle API.
    let resource: IDXGIResource1 = texture.cast().map_err(platform)?;
    let handle = resource
        .CreateSharedHandle(
            None,
            (DXGI_SHARED_RESOURCE_READ | DXGI_SHARED_RESOURCE_WRITE).0,
            None,
        )
        .map_err(platform)?;
    Ok(OwnedHandle::from_raw_handle(handle.0))
}

pub(super) struct SharedAccess {
    surface: Arc<Surface>,
    mutex: IDXGIKeyedMutex,
}
impl SharedAccess {
    pub(super) unsafe fn acquire(surface: &Arc<Surface>) -> Result<Option<Self>, RenderError> {
        if surface.shared.is_none() {
            return Ok(None);
        }
        let mutex: IDXGIKeyedMutex = surface.texture.cast().map_err(platform)?;
        acquire_mutex(&mutex)?;
        Ok(Some(Self {
            surface: Arc::clone(surface),
            mutex,
        }))
    }
}
impl Drop for SharedAccess {
    fn drop(&mut self) {
        // RenderOwners retains this guard until completion, including panic and
        // cancelled waits. A failed release must never return a reusable slot.
        if unsafe { self.mutex.ReleaseSync(0) }.is_err() {
            self.surface.failed.store(true, Ordering::Release);
        }
    }
}

pub(super) unsafe fn acquire_mutex(mutex: &IDXGIKeyedMutex) -> Result<(), RenderError> {
    // windows-rs Result treats positive WAIT_* codes as success. The SDK
    // explicitly requires exact status matching, not SUCCEEDED/is_ok.
    let status = (Interface::vtable(mutex).AcquireSync)(Interface::as_raw(mutex), 0, 0);
    if status.0 == WAIT_TIMEOUT.0 as i32 {
        return Err(RenderError::SharedSurfaceBusy);
    }
    if status != S_OK {
        return Err(RenderError::Platform(format!(
            "shared target AcquireSync: {:#x}",
            status.0
        )));
    }
    Ok(())
}

#[cfg(test)]
mod tests;
