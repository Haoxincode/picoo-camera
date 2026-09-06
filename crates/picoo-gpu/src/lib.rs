//! GPU output preparation — ARCH-PICOO-MEDIA-002, REQ-PICOO-GPU-001.

#[cfg(target_os = "macos")]
mod apple;
#[cfg(target_os = "macos")]
pub use apple::{AppleRenderer, CpuExporter, CpuImage, RenderedImage};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum OutputColor {
    /// GPUI's current Metal surface shader contract.
    Bt601Full,
    Bt709Limited,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Rotation {
    None,
    Clockwise90,
    Clockwise180,
    Clockwise270,
}

/// Output geometry uses aspect-preserving contain with opaque black margins.
/// Rotation precedes the optional horizontal mirror in output coordinates.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct RenderSpec {
    pub width: u32,
    pub height: u32,
    pub rotation: Rotation,
    pub mirror: bool,
    pub color: OutputColor,
}

#[derive(Debug, thiserror::Error)]
pub enum RenderError {
    #[error("invalid output size; expected positive even dimensions up to 1920")]
    InvalidDimensions,
    #[error("native output pool is full")]
    PoolFull,
    #[error("Metal device is unavailable")]
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
            };
            assert!(matches!(
                spec.validate(),
                Err(RenderError::InvalidDimensions)
            ));
        }
    }
}
