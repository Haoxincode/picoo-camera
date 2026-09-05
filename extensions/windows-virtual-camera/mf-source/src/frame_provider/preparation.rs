use std::sync::Arc;

#[cfg(test)]
use std::sync::atomic::{AtomicU64, Ordering};

use fast_image_resize::images::{Image, ImageRef};
use fast_image_resize::{FilterType, PixelType, ResizeAlg, ResizeOptions, Resizer};
use picoo_frame_hub::waiting_placeholder_for_size;

use super::{OutputSize, OwnedNv12Frame, SourceKey};

pub(super) struct PreparedFrameSet {
    pub(super) key: SourceKey,
    pub(super) output: OutputSize,
    frame: OwnedNv12Frame,
}

impl PreparedFrameSet {
    fn placeholder(output: OutputSize) -> Self {
        Self {
            key: SourceKey::Placeholder,
            output,
            frame: placeholder_for_size(output.width, output.height),
        }
    }

    pub(super) fn from_live(
        key: SourceKey,
        source: &OwnedNv12Frame,
        output: OutputSize,
        resources: &mut PreparationResources,
    ) -> Self {
        debug_assert!(matches!(key, SourceKey::Live(_)));
        Self {
            key,
            output,
            frame: prepare_for_size(source, output.width, output.height, resources),
        }
    }

    pub(super) fn output(&self, width: u32, height: u32) -> Option<OwnedNv12Frame> {
        (self.output.width == width && self.output.height == height).then(|| self.frame.clone())
    }
}

pub(super) struct PlaceholderFrames {
    output_480: Arc<PreparedFrameSet>,
    output_720: Arc<PreparedFrameSet>,
    output_1080: Arc<PreparedFrameSet>,
}

pub(super) struct PreparedFrames {
    output_480: Arc<PreparedFrameSet>,
    output_720: Arc<PreparedFrameSet>,
    output_1080: Arc<PreparedFrameSet>,
}

impl PlaceholderFrames {
    pub(super) fn new() -> Self {
        Self {
            output_480: Arc::new(PreparedFrameSet::placeholder(OutputSize {
                width: 854,
                height: 480,
            })),
            output_720: Arc::new(PreparedFrameSet::placeholder(OutputSize {
                width: 1280,
                height: 720,
            })),
            output_1080: Arc::new(PreparedFrameSet::placeholder(OutputSize {
                width: 1920,
                height: 1080,
            })),
        }
    }

    pub(super) fn get(&self, output: OutputSize) -> Arc<PreparedFrameSet> {
        match (output.width, output.height) {
            (854, 480) => Arc::clone(&self.output_480),
            (1280, 720) => Arc::clone(&self.output_720),
            (1920, 1080) => Arc::clone(&self.output_1080),
            _ => unreachable!("OutputSize only represents negotiated formats"),
        }
    }
}

impl PreparedFrames {
    pub(super) fn new(placeholders: &PlaceholderFrames) -> Self {
        Self {
            output_480: placeholders.get(super::OUTPUT_SIZES[0]),
            output_720: placeholders.get(super::OUTPUT_SIZES[1]),
            output_1080: placeholders.get(super::OUTPUT_SIZES[2]),
        }
    }

    pub(super) fn get(&self, output: OutputSize) -> Arc<PreparedFrameSet> {
        Arc::clone(match output.slot() {
            0 => &self.output_480,
            1 => &self.output_720,
            2 => &self.output_1080,
            _ => unreachable!("OutputSize slot is bounded"),
        })
    }

    pub(super) fn set(&mut self, output: OutputSize, frame: Arc<PreparedFrameSet>) {
        match output.slot() {
            0 => self.output_480 = frame,
            1 => self.output_720 = frame,
            2 => self.output_1080 = frame,
            _ => unreachable!("OutputSize slot is bounded"),
        }
    }
}

fn placeholder_for_size(width: u32, height: u32) -> OwnedNv12Frame {
    OwnedNv12Frame {
        width,
        height,
        stride: width,
        pixels: waiting_placeholder_for_size(width, height).into(),
    }
}

fn prepare_for_size(
    source: &OwnedNv12Frame,
    width: u32,
    height: u32,
    resources: &mut PreparationResources,
) -> OwnedNv12Frame {
    fit_nv12(source, width, height, resources).unwrap_or_else(|| OwnedNv12Frame {
        width,
        height,
        stride: width,
        pixels: picoo_frame_hub::nv12_black(width, height).into(),
    })
}

#[derive(Default)]
pub(super) struct PreparationResources {
    resizer: Resizer,
    y_scratch: Vec<u8>,
    uv_scratch: Vec<u8>,
}

#[derive(Default)]
pub(super) struct PreparationCounters {
    #[cfg(test)]
    pub(super) output_480: AtomicU64,
    #[cfg(test)]
    pub(super) output_720: AtomicU64,
    #[cfg(test)]
    pub(super) output_1080: AtomicU64,
}

impl PreparationCounters {
    pub(super) fn record(&self, output: OutputSize) {
        #[cfg(not(test))]
        let _ = output;
        #[cfg(test)]
        let counter = match (output.width, output.height) {
            (854, 480) => &self.output_480,
            (1280, 720) => &self.output_720,
            (1920, 1080) => &self.output_1080,
            _ => return,
        };
        #[cfg(test)]
        counter.fetch_add(1, Ordering::Relaxed);
    }
}

pub(super) fn fit_nv12(
    frame: &OwnedNv12Frame,
    output_width: u32,
    output_height: u32,
    resources: &mut PreparationResources,
) -> Option<OwnedNv12Frame> {
    if frame.width == output_width && frame.height == output_height && frame.stride == frame.width {
        return Some(frame.clone());
    }
    if frame.width < 2
        || frame.height < 2
        || output_width < 2
        || output_height < 2
        || !frame.width.is_multiple_of(2)
        || !frame.height.is_multiple_of(2)
        || !output_width.is_multiple_of(2)
        || !output_height.is_multiple_of(2)
        || frame.stride < frame.width
        || frame.pixels.len() < (frame.stride as usize * frame.height as usize * 3 / 2)
    {
        return None;
    }

    let source_wider = u64::from(frame.width) * u64::from(output_height)
        > u64::from(output_width) * u64::from(frame.height);
    let (fit_width, fit_height) = if source_wider {
        let height =
            (u64::from(output_width) * u64::from(frame.height) / u64::from(frame.width)) as u32;
        (output_width, height.max(2) & !1)
    } else {
        let width =
            (u64::from(output_height) * u64::from(frame.width) / u64::from(frame.height)) as u32;
        (width.max(2) & !1, output_height)
    };
    let offset_x = ((output_width - fit_width) / 2) & !1;
    let offset_y = ((output_height - fit_height) / 2) & !1;
    let src_stride = frame.stride as usize;
    let dst_stride = output_width as usize;
    let src_y_len = src_stride * frame.height as usize;
    let dst_y_len = dst_stride * output_height as usize;
    let tight_source;
    let source = if src_stride == frame.width as usize {
        frame.pixels.as_ref()
    } else {
        let mut pixels = Vec::with_capacity(frame.width as usize * frame.height as usize * 3 / 2);
        for row in frame.pixels[..src_y_len].chunks_exact(src_stride) {
            pixels.extend_from_slice(&row[..frame.width as usize]);
        }
        for row in frame.pixels[src_y_len..].chunks_exact(src_stride) {
            pixels.extend_from_slice(&row[..frame.width as usize]);
        }
        tight_source = pixels;
        tight_source.as_slice()
    };
    let tight_y_len = frame.width as usize * frame.height as usize;
    let source_y = ImageRef::new(
        frame.width,
        frame.height,
        &source[..tight_y_len],
        PixelType::U8,
    )
    .ok()?;
    let source_uv = ImageRef::new(
        frame.width / 2,
        frame.height / 2,
        &source[tight_y_len..],
        PixelType::U8x2,
    )
    .ok()?;

    let y_scratch_len = fit_width as usize * fit_height as usize;
    let uv_scratch_len = y_scratch_len / 2;
    resources.y_scratch.resize(y_scratch_len, 0);
    resources.uv_scratch.resize(uv_scratch_len, 0);
    let options = ResizeOptions::new().resize_alg(ResizeAlg::Convolution(FilterType::CatmullRom));
    {
        let mut output_y = Image::from_slice_u8(
            fit_width,
            fit_height,
            &mut resources.y_scratch,
            PixelType::U8,
        )
        .ok()?;
        resources
            .resizer
            .resize(&source_y, &mut output_y, Some(&options))
            .ok()?;
    }
    {
        let mut output_uv = Image::from_slice_u8(
            fit_width / 2,
            fit_height / 2,
            &mut resources.uv_scratch,
            PixelType::U8x2,
        )
        .ok()?;
        resources
            .resizer
            .resize(&source_uv, &mut output_uv, Some(&options))
            .ok()?;
    }

    let mut pixels = picoo_frame_hub::nv12_black(output_width, output_height);
    for row in 0..fit_height as usize {
        let source = row * fit_width as usize;
        let destination = (offset_y as usize + row) * dst_stride + offset_x as usize;
        pixels[destination..destination + fit_width as usize]
            .copy_from_slice(&resources.y_scratch[source..source + fit_width as usize]);
    }
    for row in 0..fit_height as usize / 2 {
        let source = row * fit_width as usize;
        let destination =
            dst_y_len + (offset_y as usize / 2 + row) * dst_stride + offset_x as usize;
        pixels[destination..destination + fit_width as usize]
            .copy_from_slice(&resources.uv_scratch[source..source + fit_width as usize]);
    }

    Some(OwnedNv12Frame {
        width: output_width,
        height: output_height,
        stride: output_width,
        pixels: pixels.into(),
    })
}
