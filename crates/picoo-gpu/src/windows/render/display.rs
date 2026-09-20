//! A display worker adopts the UI device and retains every imported GPU read.
use super::{platform, shared, RenderedImage};
use crate::{OutputFormat, RenderError, WindowsGpuContext};
use std::os::windows::io::AsRawHandle;
use std::sync::{atomic::Ordering, Arc, Mutex, Weak};
use windows::core::{IUnknown, Interface};
use windows::Win32::Foundation::HANDLE;
use windows::Win32::Graphics::Direct3D11::{
    ID3D11Device, ID3D11Device1, ID3D11ShaderResourceView, ID3D11Texture2D,
};
use windows::Win32::Graphics::Dxgi::IDXGIKeyedMutex;

#[cfg(test)]
mod tests;

/// One preview owner shares this UI-device binding across its target images.
#[derive(Default)]
pub struct WindowsDisplayReader {
    device: Mutex<Option<Arc<WindowsGpuContext>>>,
}
impl WindowsDisplayReader {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn image(
        self: &Arc<Self>,
        image: RenderedImage,
    ) -> Result<WindowsDisplayImage, RenderError> {
        if image.spec().format != OutputFormat::Bgra8 || image.surface.shared.is_none() {
            return Err(RenderError::UnsupportedOutputFormat);
        }
        Ok(WindowsDisplayImage {
            image,
            reader: Arc::clone(self),
            imported: Mutex::new(None),
        })
    }

    unsafe fn device(&self, device: &ID3D11Device) -> Result<Arc<WindowsGpuContext>, RenderError> {
        let mut cached = self.device.lock().map_err(|_| poisoned())?;
        if let Some(current) = cached.as_ref() {
            if current.device.cast::<IUnknown>().map_err(platform)?
                == device.cast::<IUnknown>().map_err(platform)?
            {
                return Ok(Arc::clone(current));
            }
        }
        let adopted = Arc::new(
            WindowsGpuContext::from_existing_device(device.clone())
                .map_err(|error| RenderError::Platform(error.to_string()))?,
        );
        *cached = Some(Arc::clone(&adopted));
        Ok(adopted)
    }
}

/// One immutable BGRA image, reusable across UI redraws without recursive mutex acquisition.
pub struct WindowsDisplayImage {
    image: RenderedImage,
    reader: Arc<WindowsDisplayReader>,
    imported: Mutex<Option<Arc<Imported>>>,
}
impl std::fmt::Debug for WindowsDisplayImage {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("WindowsDisplayImage")
            .field("spec", &self.image.spec())
            .finish_non_exhaustive()
    }
}
impl WindowsDisplayImage {
    pub fn spec(&self) -> crate::RenderSpec {
        self.image.spec()
    }

    /// Enqueue a draw without waiting for the GPU. `false` means shared access is busy.
    ///
    /// # Safety
    /// Called on the UI device's owner thread. `read` must enqueue only immutable
    /// reads of the provided view on this device, before returning, and unbind it
    /// before returning. Never retain raw references for a later submission. The
    /// native context is protected for this whole direct-command group.
    pub unsafe fn with_read(
        &self,
        device: &ID3D11Device,
        read: impl FnOnce(&ID3D11ShaderResourceView) -> Result<(), RenderError>,
    ) -> Result<bool, RenderError> {
        if self.image.surface.failed.load(Ordering::Acquire) {
            return Err(RenderError::DeviceUnavailable);
        }
        let gpu = self.reader.device(device)?;
        gpu.device.GetDeviceRemovedReason().map_err(platform)?;
        let imported = {
            let mut cached = self.imported.lock().map_err(|_| poisoned())?;
            if cached
                .as_ref()
                .is_none_or(|current| !Arc::ptr_eq(&current.gpu, &gpu))
            {
                *cached = Some(Arc::new(Imported::new(gpu, &self.image)?));
            }
            Arc::clone(cached.as_ref().expect("initialized import"))
        };
        let access = {
            let mut active = imported.active.lock().map_err(|_| poisoned())?;
            match active.upgrade() {
                Some(access) => access,
                None => {
                    match shared::acquire_mutex(&imported.mutex) {
                        Ok(()) => {}
                        Err(RenderError::SharedSurfaceBusy) => return Ok(false),
                        Err(error) => return Err(error),
                    }
                    let access = Arc::new(ReadAccess {
                        image: self.image.clone(),
                        imported: Arc::clone(&imported),
                    });
                    *active = Arc::downgrade(&access);
                    access
                }
            }
        };
        let mut result = None;
        // Preparation happens before the draw callback. Dropping the completion
        // receiver is intentional: the native callback still owns the read lease.
        let completion = imported
            .gpu
            .submit_owned(access, |gpu, access| {
                result = Some(gpu.with_immediate_context(|_| read(&access.imported.view)));
                Ok(())
            })
            .map_err(|error| RenderError::Platform(error.to_string()))?;
        drop(completion);
        result
            .unwrap_or_else(|| Err(RenderError::Platform("display draw panicked".into())))
            .map(|()| true)
    }
}

fn poisoned() -> RenderError {
    RenderError::Platform("display resource lock poisoned".into())
}

struct Imported {
    gpu: Arc<WindowsGpuContext>,
    view: ID3D11ShaderResourceView,
    mutex: IDXGIKeyedMutex,
    active: Mutex<Weak<ReadAccess>>,
}
// SAFETY: Free-threaded D3D11 interfaces; draw commands are confined to the
// protected UI context. The active access guard retains the original pool lease.
unsafe impl Send for Imported {}
unsafe impl Sync for Imported {}
impl Imported {
    unsafe fn new(gpu: Arc<WindowsGpuContext>, image: &RenderedImage) -> Result<Self, RenderError> {
        let device: ID3D11Device1 = gpu.device.cast().map_err(platform)?;
        let texture: ID3D11Texture2D = device
            .OpenSharedResource1(HANDLE(image.shared_bgra_handle()?.as_raw_handle()))
            .map_err(platform)?;
        let mut view = None;
        gpu.device
            .CreateShaderResourceView(&texture, None, Some(&mut view))
            .map_err(platform)?;
        let mutex = texture.cast().map_err(platform)?;
        Ok(Self {
            gpu,
            view: view.ok_or(RenderError::DeviceUnavailable)?,
            mutex,
            active: Mutex::new(Weak::new()),
        })
    }
}
struct ReadAccess {
    image: RenderedImage,
    imported: Arc<Imported>,
}
impl Drop for ReadAccess {
    fn drop(&mut self) {
        if unsafe { self.imported.mutex.ReleaseSync(0) }.is_err() {
            self.image.surface.failed.store(true, Ordering::Release);
        }
    }
}
