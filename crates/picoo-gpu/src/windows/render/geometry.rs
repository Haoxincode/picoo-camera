//! Video Processor clips after rotation and mirroring, unlike source metadata.
use picoo_frame_hub::{ImageSize, VisibleRect};
use windows::Win32::Foundation::RECT;

use crate::{RenderError, RenderSpec, Rotation};

pub(super) fn rectangles(
    size: ImageSize,
    visible: VisibleRect,
    spec: RenderSpec,
) -> Result<(RECT, RECT), RenderError> {
    spec.validate()?;
    if visible.width == 0
        || visible.height == 0
        || visible
            .x
            .checked_add(visible.width)
            .is_none_or(|v| v > size.width)
        || visible
            .y
            .checked_add(visible.height)
            .is_none_or(|v| v > size.height)
        || size.width > i32::MAX as u32
        || size.height > i32::MAX as u32
    {
        return Err(RenderError::InvalidDimensions);
    }
    let (mut x, y, width, height, rotated_width) = match spec.rotation {
        Rotation::None => (
            visible.x,
            visible.y,
            visible.width,
            visible.height,
            size.width,
        ),
        Rotation::Clockwise90 => (
            size.height - visible.y - visible.height,
            visible.x,
            visible.height,
            visible.width,
            size.height,
        ),
        Rotation::Clockwise180 => (
            size.width - visible.x - visible.width,
            size.height - visible.y - visible.height,
            visible.width,
            visible.height,
            size.width,
        ),
        Rotation::Clockwise270 => (
            visible.y,
            size.width - visible.x - visible.width,
            visible.height,
            visible.width,
            size.height,
        ),
    };
    if spec.mirror {
        x = rotated_width - x - width;
    }
    let source = RECT {
        left: x as i32,
        top: y as i32,
        right: (x + width) as i32,
        bottom: (y + height) as i32,
    };
    let (dest_width, dest_height) =
        if u64::from(spec.width) * u64::from(height) <= u64::from(spec.height) * u64::from(width) {
            (
                spec.width,
                (u64::from(spec.width) * u64::from(height) / u64::from(width)) as u32,
            )
        } else {
            (
                (u64::from(spec.height) * u64::from(width) / u64::from(height)) as u32,
                spec.height,
            )
        };
    // Keep 4:2:0 luma/chroma boundaries aligned; never stretch to fill a black bar.
    let (dest_width, dest_height) = (dest_width & !1, dest_height & !1);
    if dest_width == 0 || dest_height == 0 {
        return Err(RenderError::InvalidDimensions);
    }
    let left = ((spec.width - dest_width) / 2) & !1;
    let top = ((spec.height - dest_height) / 2) & !1;
    Ok((
        source,
        RECT {
            left: left as i32,
            top: top as i32,
            right: (left + dest_width) as i32,
            bottom: (top + dest_height) as i32,
        },
    ))
}
