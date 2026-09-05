//! Media Foundation NV12 allocation-layout normalization.

use crate::DecodeError;

/// Media Foundation may expose a contiguous NV12 buffer whose allocation height
/// is macroblock-aligned (for example 1920x1088 for a visible 1920x1080 frame).
/// The UV plane then starts after the allocated Y rows, not after the visible
/// rows. Normalize both vertically aligned and row-pitched storage to a tight
/// visible frame so downstream consumers have one unambiguous layout.
#[cfg(test)]
pub(super) fn normalize_contiguous_nv12(
    source: Vec<u8>,
    width: u32,
    height: u32,
) -> Result<Vec<u8>, DecodeError> {
    let (width, height, tight_len) = validate_dimensions(source.len(), width, height)?;
    if source.len() == tight_len {
        return Ok(source);
    }

    let (stride, allocated_height) = contiguous_nv12_layout(source.len(), width, height)?;
    copy_visible_nv12(&source, width, height, stride, allocated_height)
}

/// Copy directly from a locked MF allocation into the one final tight buffer.
/// Unlike the owned test helper, padded layouts never materialize the full
/// padded allocation first.
pub(super) fn normalize_contiguous_nv12_slice(
    source: &[u8],
    width: u32,
    height: u32,
) -> Result<Vec<u8>, DecodeError> {
    let (width, height, tight_len) = validate_dimensions(source.len(), width, height)?;
    if source.len() == tight_len {
        return Ok(source.to_vec());
    }

    let (stride, allocated_height) = contiguous_nv12_layout(source.len(), width, height)?;
    copy_visible_nv12(source, width, height, stride, allocated_height)
}

fn validate_dimensions(
    source_len: usize,
    width: u32,
    height: u32,
) -> Result<(usize, usize, usize), DecodeError> {
    if width == 0 || height == 0 || !width.is_multiple_of(2) || !height.is_multiple_of(2) {
        return Err(DecodeError::Platform(format!(
            "invalid NV12 dimensions: {width}x{height}"
        )));
    }

    let width = width as usize;
    let height = height as usize;
    let tight_len = width
        .checked_mul(height)
        .and_then(|value| value.checked_mul(3))
        .map(|value| value / 2)
        .ok_or_else(|| DecodeError::Platform("NV12 dimensions overflow".into()))?;
    if source_len < tight_len {
        return Err(DecodeError::Platform(format!(
            "short NV12 output: {source_len} bytes, need {tight_len}"
        )));
    }
    Ok((width, height, tight_len))
}

fn contiguous_nv12_layout(
    source_len: usize,
    width: usize,
    height: usize,
) -> Result<(usize, usize), DecodeError> {
    // The byte length alone can describe both a row-pitched buffer and a
    // vertically aligned buffer. 1280x720 allocated as 1280x736 is the
    // important ambiguous case: interpreting it as 1308-byte rows shears and
    // vertically stretches the preview. Prefer the macroblock-aligned vertical
    // interpretation when the competing row pitch is not macroblock aligned.
    let visible_rows_x2 = height * 3;
    let doubled_len = source_len.saturating_mul(2);
    let row_pitch = if doubled_len.is_multiple_of(visible_rows_x2) {
        let stride = doubled_len / visible_rows_x2;
        (stride >= width).then_some(stride)
    } else {
        None
    };

    let width_x3 = width * 3;
    let allocated_height = if doubled_len.is_multiple_of(width_x3) {
        let allocated_height = doubled_len / width_x3;
        (allocated_height >= height).then_some(allocated_height)
    } else {
        None
    };

    if let Some(allocated_height) = allocated_height {
        let vertical_is_unambiguous = row_pitch.is_none();
        let vertical_matches_macroblocks = allocated_height.is_multiple_of(16)
            && row_pitch.is_some_and(|stride| !stride.is_multiple_of(16));
        if vertical_is_unambiguous || vertical_matches_macroblocks {
            return Ok((width, allocated_height));
        }
    }

    if let Some(stride) = row_pitch {
        return Ok((stride, height));
    }

    Err(DecodeError::Platform(format!(
        "unsupported NV12 allocation: {source_len} bytes for visible {width}x{height}",
    )))
}

fn copy_visible_nv12(
    source: &[u8],
    width: usize,
    height: usize,
    stride: usize,
    allocated_height: usize,
) -> Result<Vec<u8>, DecodeError> {
    let uv_offset = stride
        .checked_mul(allocated_height)
        .ok_or_else(|| DecodeError::Platform("NV12 UV offset overflow".into()))?;
    let required = uv_offset
        .checked_add(stride * (height / 2))
        .ok_or_else(|| DecodeError::Platform("NV12 allocation overflow".into()))?;
    if source.len() < required {
        return Err(DecodeError::Platform(format!(
            "short NV12 planes: {} bytes, need {required}",
            source.len()
        )));
    }

    let mut tight = vec![0_u8; width * height * 3 / 2];
    for row in 0..height {
        let src = row * stride;
        let dst = row * width;
        tight[dst..dst + width].copy_from_slice(&source[src..src + width]);
    }
    let tight_uv_offset = width * height;
    for row in 0..height / 2 {
        let src = uv_offset + row * stride;
        let dst = tight_uv_offset + row * width;
        tight[dst..dst + width].copy_from_slice(&source[src..src + width]);
    }
    Ok(tight)
}
