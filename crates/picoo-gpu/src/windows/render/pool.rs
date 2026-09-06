use crate::{RenderError, RenderSpec};
use std::os::windows::io::OwnedHandle;
use std::sync::{
    atomic::{AtomicBool, Ordering},
    Arc,
};
use windows::Win32::Graphics::Direct3D11::{
    ID3D11Device, ID3D11Texture2D, D3D11_BIND_RENDER_TARGET, D3D11_BIND_SHADER_RESOURCE,
    D3D11_RESOURCE_MISC_SHARED_KEYEDMUTEX, D3D11_RESOURCE_MISC_SHARED_NTHANDLE,
    D3D11_TEXTURE2D_DESC, D3D11_USAGE_DEFAULT,
};
use windows::Win32::Graphics::Dxgi::Common::DXGI_SAMPLE_DESC;

pub(super) struct Surface {
    pub texture: ID3D11Texture2D,
    pub shared: Option<OwnedHandle>,
    pub failed: AtomicBool,
}
impl Surface {
    #[cfg(test)]
    pub fn unshared(texture: ID3D11Texture2D) -> Self {
        Self {
            texture,
            shared: None,
            failed: AtomicBool::new(false),
        }
    }
}
// SAFETY: D3D11 surfaces are free-threaded. Only an exclusively acquired pool
// allocation is written, and every GPU reader must hold its lease to completion.
unsafe impl Send for Surface {}
unsafe impl Sync for Surface {}

pub(super) struct OutputPool {
    slots: Vec<Arc<Surface>>,
    spec: RenderSpec,
}
impl OutputPool {
    pub(super) fn new(spec: RenderSpec) -> Self {
        Self {
            slots: Vec::new(),
            spec,
        }
    }
    pub(super) unsafe fn acquire(
        &mut self,
        device: &ID3D11Device,
    ) -> Result<Arc<Surface>, RenderError> {
        for slot in &mut self.slots {
            if slot.failed.load(Ordering::Acquire) {
                return Err(RenderError::DeviceUnavailable);
            }
            if Arc::get_mut(slot).is_some() {
                return Ok(Arc::clone(slot));
            }
        }
        if self.slots.len() == 3 {
            return Err(RenderError::PoolFull);
        }
        let shared = self.spec.format == crate::OutputFormat::Bgra8;
        let mut texture = None;
        device
            .CreateTexture2D(
                &D3D11_TEXTURE2D_DESC {
                    Width: self.spec.width,
                    Height: self.spec.height,
                    MipLevels: 1,
                    ArraySize: 1,
                    Format: super::output_format(self.spec),
                    SampleDesc: DXGI_SAMPLE_DESC {
                        Count: 1,
                        Quality: 0,
                    },
                    Usage: D3D11_USAGE_DEFAULT,
                    MiscFlags: if shared {
                        (D3D11_RESOURCE_MISC_SHARED_NTHANDLE
                            | D3D11_RESOURCE_MISC_SHARED_KEYEDMUTEX)
                            .0 as u32
                    } else {
                        0
                    },
                    BindFlags: (D3D11_BIND_RENDER_TARGET | D3D11_BIND_SHADER_RESOURCE).0 as u32,
                    ..Default::default()
                },
                None,
                Some(&mut texture),
            )
            .map_err(super::platform)?;
        let texture = texture.ok_or(RenderError::DeviceUnavailable)?;
        let handle = if shared {
            Some(super::shared::create_handle(&texture)?)
        } else {
            None
        };
        let surface = Arc::new(Surface {
            texture,
            shared: handle,
            failed: AtomicBool::new(false),
        });
        self.slots.push(Arc::clone(&surface));
        Ok(surface)
    }
}
