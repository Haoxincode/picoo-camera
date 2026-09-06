//! Official NT shared textures; ownership and keyed access are both required.
use super::{platform, RenderedImage, Surface};
use crate::RenderError;
use std::os::windows::io::{AsHandle, BorrowedHandle, FromRawHandle, OwnedHandle};
use std::sync::{atomic::Ordering, Arc};
use windows::core::Interface;
use windows::Win32::Foundation::S_OK;
use windows::Win32::Graphics::Direct3D11::ID3D11Texture2D;
use windows::Win32::Graphics::Dxgi::{
    IDXGIKeyedMutex, IDXGIResource1, DXGI_SHARED_RESOURCE_READ, DXGI_SHARED_RESOURCE_WRITE,
};

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
        self.surface
            .shared
            .as_ref()
            .map(AsHandle::as_handle)
            .ok_or(RenderError::UnsupportedOutputFormat)
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
