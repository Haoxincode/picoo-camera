//! Shared output bytes and fixed allocation budget; platform exporters own mapping.
use crate::{RenderError, RenderSpec};
use std::sync::Arc;

/// Tightly packed NV12 output for an explicit CPU consumer, never a source frame.
pub struct CpuImage {
    spec: RenderSpec,
    pixels: Vec<u8>,
}
impl CpuImage {
    pub fn spec(&self) -> RenderSpec {
        self.spec
    }
    pub fn stride(&self) -> u32 {
        self.spec.width
    }
    pub fn pixels(&self) -> &[u8] {
        &self.pixels
    }
}

pub(crate) struct CpuImagePool {
    spec: RenderSpec,
    slots: Vec<Arc<CpuImage>>,
}
impl CpuImagePool {
    pub(crate) fn new(spec: RenderSpec) -> Result<Self, RenderError> {
        spec.validate()?;
        if spec.format != crate::OutputFormat::Nv12 {
            return Err(RenderError::UnsupportedOutputFormat);
        }
        Ok(Self {
            spec,
            slots: Vec::with_capacity(3),
        })
    }

    /// Reserve writable storage before platform readback. Failure never publishes
    /// a partial image. Strong and weak readers both exclude writable access.
    pub(crate) fn materialize(
        &mut self,
        copy: impl FnOnce(&mut [u8]) -> Result<(), RenderError>,
    ) -> Result<Arc<CpuImage>, RenderError> {
        let mut available = None;
        for (index, slot) in self.slots.iter_mut().enumerate() {
            if Arc::get_mut(slot).is_some() {
                available = Some(index);
                break;
            }
        }
        let index = match available {
            Some(index) => index,
            None if self.slots.len() < 3 => {
                let length = self.spec.width as usize * self.spec.height as usize * 3 / 2;
                self.slots.push(Arc::new(CpuImage {
                    spec: self.spec,
                    pixels: vec![0; length],
                }));
                self.slots.len() - 1
            }
            None => return Err(RenderError::PoolFull),
        };
        let output = Arc::get_mut(&mut self.slots[index]).ok_or(RenderError::PoolFull)?;
        copy(&mut output.pixels)?;
        Ok(Arc::clone(&self.slots[index]))
    }
}
