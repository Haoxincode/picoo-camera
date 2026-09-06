//! GPU output preparation — ARCH-PICOO-MEDIA-002, REQ-PICOO-GPU-001.

#[cfg(target_os = "macos")]
mod apple;
#[cfg(target_os = "macos")]
pub use apple::{AppleRenderer, CpuExporter, RenderedImage};

#[cfg(any(target_os = "macos", target_os = "windows"))]
mod cpu_image;
#[cfg(any(target_os = "macos", target_os = "windows"))]
pub use cpu_image::CpuImage;

#[cfg(target_os = "windows")]
mod windows;
#[cfg(target_os = "windows")]
pub use windows::{
    CpuExporter, RenderedImage, WindowsAdapterId, WindowsCompletionError, WindowsDeviceError,
    WindowsGpuCompletion, WindowsGpuContext, WindowsRenderer,
};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum OutputColor {
    /// GPUI's current Metal surface shader contract.
    Bt601Full,
    Bt709Limited,
    /// Full-range RGB with gamma 2.2 and BT.709 primaries (DXGI display contract).
    RgbFullG22Bt709,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum OutputFormat {
    Nv12,
    Bgra8,
}

pub use picoo_frame_hub::Rotation;

/// Output geometry uses aspect-preserving contain with opaque black margins.
/// Rotation precedes the optional horizontal mirror in output coordinates.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct RenderSpec {
    pub width: u32,
    pub height: u32,
    pub rotation: Rotation,
    pub mirror: bool,
    pub color: OutputColor,
    pub format: OutputFormat,
}

#[derive(Debug, thiserror::Error)]
pub enum RenderError {
    #[error("unsupported output storage/color combination on this backend")]
    UnsupportedOutputFormat,
    #[error("invalid output size; expected positive even dimensions up to 1920")]
    InvalidDimensions,
    #[error("native output pool is full")]
    PoolFull,
    #[error("native GPU device is unavailable")]
    DeviceUnavailable,
    #[error("native source must declare BT.709 matrix, primaries and transfer")]
    UnsupportedSourceColor,
    #[error("GPU output layout differs from the CPU exporter contract")]
    OutputLayoutMismatch,
    #[error("platform GPU render failed: {0}")]
    Platform(String),
}

impl RenderSpec {
    pub fn validate(&self) -> Result<(), RenderError> {
        if !matches!(
            (self.format, self.color),
            (
                OutputFormat::Nv12,
                OutputColor::Bt601Full | OutputColor::Bt709Limited
            ) | (OutputFormat::Bgra8, OutputColor::RgbFullG22Bt709)
        ) {
            return Err(RenderError::UnsupportedOutputFormat);
        }
        if self.width == 0
            || self.height == 0
            || self.width > 1920
            || self.height > 1920
            || !self.width.is_multiple_of(2)
            || !self.height.is_multiple_of(2)
        {
            return Err(RenderError::InvalidDimensions);
        }
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn rgb_targets_cannot_be_interpreted_as_nv12_outputs() {
        let mut spec = RenderSpec {
            width: 1280,
            height: 720,
            rotation: Rotation::None,
            mirror: false,
            color: OutputColor::RgbFullG22Bt709,
            format: OutputFormat::Bgra8,
        };
        spec.validate().unwrap();
        #[cfg(any(target_os = "macos", target_os = "windows"))]
        assert!(matches!(
            crate::cpu_image::CpuImagePool::new(spec),
            Err(RenderError::UnsupportedOutputFormat)
        ));
        spec.format = OutputFormat::Nv12;
        assert!(matches!(
            spec.validate(),
            Err(RenderError::UnsupportedOutputFormat)
        ));
        spec.format = OutputFormat::Bgra8;
        spec.color = OutputColor::Bt709Limited;
        assert!(matches!(
            spec.validate(),
            Err(RenderError::UnsupportedOutputFormat)
        ));
    }

    #[test]
    fn rejects_invalid_dimensions_before_platform_allocation() {
        for (width, height) in [
            (0, 720),
            (1280, 0),
            (1281, 720),
            (1280, 721),
            (1922, 1080),
            (1080, u32::MAX),
        ] {
            let spec = RenderSpec {
                width,
                height,
                rotation: Rotation::None,
                mirror: false,
                color: OutputColor::Bt709Limited,
                format: crate::OutputFormat::Nv12,
            };
            assert!(matches!(
                spec.validate(),
                Err(RenderError::InvalidDimensions)
            ));
        }
    }
}
