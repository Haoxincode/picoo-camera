//! A resource supplier owns access and GPU completion; GPUI only enqueues a draw.
use crate::{DevicePixels, Size};
use anyhow::Result;
use std::sync::Arc;
use windows::Win32::Graphics::Direct3D11::{ID3D11Device, ID3D11ShaderResourceView};

/// Supplies an immutable opaque BGRA8 SDR image to the Direct3D renderer.
///
/// # Safety
/// `with_read` must reserve resource/access/completion owners before invoking the
/// callback, invoke it synchronously on the calling thread at most once, and
/// retain those owners until the resulting GPU reads complete (also on error or
/// panic). Its view must belong to `device` and cover the declared complete image.
/// The image is BGRA8 UNORM, opaque, full-range RGB G22 with BT.709 primaries.
/// No CPU pixels or application configuration cross this interface.
pub unsafe trait Direct3DSurfaceSource: std::fmt::Debug + Send + Sync {
    /// Intrinsic image dimensions, independent of the element's layout bounds.
    fn size(&self) -> Size<DevicePixels>;

    /// Calls `read` when ready; `false` means access is busy and this draw is skipped.
    /// Implementations retain/report failures to their owner. GPUI skips failed
    /// images so unavailable media cannot prevent unrelated controls repainting.
    ///
    /// # Safety
    /// `read` must only enqueue reads of the supplied image, finish enqueueing
    /// before returning, and unbind the view before returning. It must not retain
    /// raw references for later submissions or mutate the image through aliases.
    unsafe fn with_read(
        &self,
        device: &ID3D11Device,
        read: &mut dyn FnMut(&ID3D11ShaderResourceView) -> Result<()>,
    ) -> Result<bool>;
}

/// Cloneable identity and lifetime of an externally prepared Direct3D image.
#[derive(Clone, Debug)]
pub struct Direct3DSurface(Arc<dyn Direct3DSurfaceSource>);

impl Direct3DSurface {
    /// Wrap a supplier whose access and completion contract is already established.
    pub fn new(source: impl Direct3DSurfaceSource + 'static) -> Self {
        Self(Arc::new(source))
    }
    /// Returns the supplier's immutable dimensions.
    pub fn size(&self) -> Size<DevicePixels> {
        self.0.size()
    }
    /// Enqueue a read inside the supplier's protected resource lifetime.
    ///
    /// # Safety
    /// The callback must satisfy [`Direct3DSurfaceSource::with_read`].
    pub unsafe fn with_read(
        &self,
        device: &ID3D11Device,
        read: &mut dyn FnMut(&ID3D11ShaderResourceView) -> Result<()>,
    ) -> Result<bool> {
        unsafe { self.0.with_read(device, read) }
    }
}
impl PartialEq for Direct3DSurface {
    fn eq(&self, other: &Self) -> bool {
        Arc::ptr_eq(&self.0, &other.0)
    }
}
impl Eq for Direct3DSurface {}
