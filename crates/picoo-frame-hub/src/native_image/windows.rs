//! MF sample retention owns a decoder allocation, not just its texture COM reference.
use std::sync::Arc;

use thiserror::Error;
use windows::core::Interface;
use windows::Win32::Graphics::Direct3D11::{
    ID3D11Device, ID3D11Texture2D, D3D11_TEXTURE2D_DESC, D3D11_USAGE_DEFAULT,
};
use windows::Win32::Graphics::Dxgi::Common::DXGI_FORMAT_NV12;
use windows::Win32::Media::MediaFoundation::{IMFDXGIBuffer, IMFSample};

#[derive(Debug, Error)]
pub enum NativeImageError {
    #[error("native source belongs to a different D3D11 device")]
    WrongDevice,
    #[error("native source requires a single default-usage NV12 D3D11 surface")]
    UnsupportedStorage,
    #[error("native source has invalid dimensions or subresource")]
    InvalidGeometry,
    #[error("native source platform error: {0}")]
    Platform(#[from] windows::core::Error),
}

struct RetainedSurface {
    _sample: IMFSample,
    texture: ID3D11Texture2D,
    subresource: u32,
    width: u32,
    height: u32,
}

// SAFETY: The unsafe constructor requires a free-threaded MF sample and immutable,
// completed image storage. No safe method exposes COM mutation or a device context.
unsafe impl Send for RetainedSurface {}
unsafe impl Sync for RetainedSurface {}

#[derive(Clone)]
pub struct D3D11ImageLease(Arc<RetainedSurface>);

impl std::fmt::Debug for D3D11ImageLease {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("D3D11ImageLease")
            .field("width", &self.width())
            .field("height", &self.height())
            .field("subresource", &self.0.subresource)
            .finish_non_exhaustive()
    }
}

impl D3D11ImageLease {
    /// REQ-PICOO-FRAME-016: retain the allocator's sample lease before returning output.
    /// The expected device must be the Decoder generation's fixed device.
    ///
    /// # Safety
    /// The sample must be free-threaded. All image writes must have completed and
    /// all aliases must remain immutable until the final clone is released. The
    /// producer must honor the retained sample's allocator lease rather than
    /// manually overwrite the texture or recycle its array slice.
    pub unsafe fn retain_completed(
        sample: &IMFSample,
        expected_device: &ID3D11Device,
    ) -> Result<Self, NativeImageError> {
        if sample.GetBufferCount()? != 1 {
            return Err(NativeImageError::UnsupportedStorage);
        }
        let surface: IMFDXGIBuffer = sample.GetBufferByIndex(0)?.cast()?;
        let mut raw = std::ptr::null_mut();
        surface.GetResource(&ID3D11Texture2D::IID, &mut raw)?;
        if raw.is_null() {
            return Err(NativeImageError::UnsupportedStorage);
        }
        // GetResource returns an owned interface reference on success.
        let texture = ID3D11Texture2D::from_raw(raw);
        // COM identity, not adapter identity: two devices on one adapter have
        // distinct command queues and completion domains.
        let actual_device = texture.GetDevice()?;
        if actual_device.cast::<windows::core::IUnknown>()?
            != expected_device.cast::<windows::core::IUnknown>()?
        {
            return Err(NativeImageError::WrongDevice);
        }
        let subresource = surface.GetSubresourceIndex()?;
        let mut description = D3D11_TEXTURE2D_DESC::default();
        texture.GetDesc(&mut description);
        if description.Format != DXGI_FORMAT_NV12
            || description.Usage != D3D11_USAGE_DEFAULT
            || description.CPUAccessFlags != 0
            || description.MipLevels != 1
            || description.SampleDesc.Count != 1
            || description.SampleDesc.Quality != 0
        {
            return Err(NativeImageError::UnsupportedStorage);
        }
        if description.Width == 0
            || description.Height == 0
            || !description.Width.is_multiple_of(2)
            || !description.Height.is_multiple_of(2)
            || subresource >= description.ArraySize
        {
            return Err(NativeImageError::InvalidGeometry);
        }
        Ok(Self(Arc::new(RetainedSurface {
            _sample: sample.clone(),
            texture,
            subresource,
            width: description.Width,
            height: description.Height,
        })))
    }

    pub fn width(&self) -> u32 {
        self.0.width
    }

    pub fn height(&self) -> u32 {
        self.0.height
    }

    /// Read-only platform access for GPU consumers.
    ///
    /// # Safety
    /// Do not mutate or map the texture, or expose mutable aliases. Retain an
    /// image clone until all GPU reads finish, including cancellation and errors.
    pub unsafe fn texture(&self) -> (&ID3D11Texture2D, u32) {
        (&self.0.texture, self.0.subresource)
    }
}

#[cfg(test)]
mod tests;
