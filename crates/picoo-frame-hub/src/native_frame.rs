//! Immutable source meaning, identity and ownership — REQ-PICOO-FRAME-012.

use std::time::Instant;

use crate::NativeImage;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct FrameIdentity {
    pub connection_generation: u64,
    pub stream_epoch: u64,
    pub decoder_generation: u64,
    pub frame_id: u64,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ImageSize {
    pub width: u32,
    pub height: u32,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct VisibleRect {
    pub x: u32,
    pub y: u32,
    pub width: u32,
    pub height: u32,
}

impl VisibleRect {
    pub fn fits_in(self, width: u32, height: u32) -> bool {
        self.width > 0
            && self.height > 0
            && self.width.is_multiple_of(2)
            && self.height.is_multiple_of(2)
            && self.x.is_multiple_of(2)
            && self.y.is_multiple_of(2)
            && self
                .x
                .checked_add(self.width)
                .is_some_and(|right| right <= width)
            && self
                .y
                .checked_add(self.height)
                .is_some_and(|bottom| bottom <= height)
    }
}

/// Remaining visible crop after subtracting crop the platform decoder already applied.
/// Coded size comes from SPS; allocation comes from the native output. If the
/// decoder published the full coded allocation, remaining crop is the admitted
/// SPS picture. If its remaining aperture already equals that picture, do not
/// crop the same edge again.
pub fn remaining_visible_after_decoder(
    admitted: VisibleRect,
    coded: ImageSize,
    allocation: ImageSize,
    platform: VisibleRect,
) -> Option<VisibleRect> {
    if !admitted.fits_in(coded.width, coded.height)
        || allocation.width == 0
        || allocation.height == 0
    {
        return None;
    }
    if (platform.width, platform.height) == (admitted.width, admitted.height)
        && platform.fits_in(allocation.width, allocation.height)
    {
        return Some(platform);
    }
    let full_allocation = VisibleRect {
        x: 0,
        y: 0,
        width: allocation.width,
        height: allocation.height,
    };
    if platform == full_allocation
        && allocation == coded
        && admitted.fits_in(allocation.width, allocation.height)
    {
        return Some(admitted);
    }
    None
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct PixelAspectRatio {
    pub numerator: u32,
    pub denominator: u32,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Rotation {
    None,
    Clockwise90,
    Clockwise180,
    Clockwise270,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct PresentationTransform {
    pub rotation: Rotation,
    pub mirror: bool,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ChromaSiting {
    Left,
    Center,
}

/// The admitted source color contract; unknown color is not a valid source.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SourceColor {
    Nv12Bt709Limited { chroma_siting: ChromaSiting },
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct FrameDescription {
    /// Encoded geometry, separate from the native allocation dimensions.
    pub coded_size: ImageSize,
    /// Remaining visible crop in native image coordinates. The producer must
    /// remove any crop already applied by its platform decoder.
    pub visible_rect: VisibleRect,
    pub pixel_aspect_ratio: PixelAspectRatio,
    pub color: SourceColor,
    /// Only the presentation operations not yet applied to the source image.
    pub transform: PresentationTransform,
    pub config_revision: u64,
}

#[derive(Debug, Clone, Copy)]
pub struct FrameTimeline {
    pub encoded_at_us: u64,
    pub received_at_us: u64,
    pub decode_submitted_at_us: u64,
    pub decoded_at: Instant,
}

#[derive(Debug, thiserror::Error, PartialEq, Eq)]
#[error("invalid native frame geometry or pixel aspect ratio")]
pub struct InvalidFrameDescription;

/// A completed immutable native source. It has no CPU stride or pixel storage.
/// The description is the submitting AU's snapshot, not current owner state.
#[derive(Debug)]
pub struct NativeVideoFrame {
    identity: FrameIdentity,
    source_pts_us: u64,
    description: FrameDescription,
    image: NativeImage,
    timeline: FrameTimeline,
}

impl NativeVideoFrame {
    pub fn new(
        identity: FrameIdentity,
        source_pts_us: u64,
        description: FrameDescription,
        image: NativeImage,
        timeline: FrameTimeline,
    ) -> Result<Self, InvalidFrameDescription> {
        let rect = description.visible_rect;
        if description.coded_size.width == 0
            || description.coded_size.height == 0
            || rect.width == 0
            || rect.height == 0
            || !rect.width.is_multiple_of(2)
            || !rect.height.is_multiple_of(2)
            || !rect.x.is_multiple_of(2)
            || !rect.y.is_multiple_of(2)
            || rect
                .x
                .checked_add(rect.width)
                .is_none_or(|right| right > image.width())
            || rect
                .y
                .checked_add(rect.height)
                .is_none_or(|bottom| bottom > image.height())
            || description.pixel_aspect_ratio.numerator == 0
            || description.pixel_aspect_ratio.denominator == 0
        {
            return Err(InvalidFrameDescription);
        }
        Ok(Self {
            identity,
            source_pts_us,
            description,
            image,
            timeline,
        })
    }

    pub fn identity(&self) -> FrameIdentity {
        self.identity
    }
    pub fn source_pts_us(&self) -> u64 {
        self.source_pts_us
    }
    pub fn description(&self) -> FrameDescription {
        self.description
    }
    pub fn image(&self) -> &NativeImage {
        &self.image
    }
    pub fn timeline(&self) -> FrameTimeline {
        self.timeline
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn rect(x: u32, y: u32, width: u32, height: u32) -> VisibleRect {
        VisibleRect {
            x,
            y,
            width,
            height,
        }
    }

    fn size(width: u32, height: u32) -> ImageSize {
        ImageSize { width, height }
    }

    #[test]
    fn remaining_crop_uses_admitted_picture_when_decoder_published_coded_allocation() {
        let admitted = rect(0, 0, 1920, 1080);
        let coded = size(1920, 1088);
        let allocation = coded;
        assert_eq!(
            remaining_visible_after_decoder(admitted, coded, allocation, rect(0, 0, 1920, 1088)),
            Some(admitted)
        );
    }

    #[test]
    fn remaining_crop_keeps_platform_aperture_when_decoder_already_applied_it() {
        let admitted = rect(0, 0, 1920, 1080);
        let coded = size(1920, 1088);
        assert_eq!(
            remaining_visible_after_decoder(
                admitted,
                coded,
                size(1920, 1088),
                rect(0, 0, 1920, 1080)
            ),
            Some(rect(0, 0, 1920, 1080))
        );
        assert_eq!(
            remaining_visible_after_decoder(
                admitted,
                coded,
                size(1920, 1080),
                rect(0, 0, 1920, 1080)
            ),
            Some(rect(0, 0, 1920, 1080))
        );
    }

    #[test]
    fn remaining_crop_rejects_conflicting_platform_aperture() {
        assert_eq!(
            remaining_visible_after_decoder(
                rect(0, 0, 1920, 1080),
                size(1920, 1088),
                size(1920, 1088),
                rect(0, 0, 1280, 720)
            ),
            None
        );
        assert_eq!(
            remaining_visible_after_decoder(
                rect(0, 0, 1920, 1080),
                size(1920, 1088),
                size(1280, 720),
                rect(0, 0, 1280, 720)
            ),
            None
        );
    }
}
