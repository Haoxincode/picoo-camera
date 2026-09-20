//! MF output samples keep their allocator lease until the device completes work.
use std::mem::ManuallyDrop;
use std::sync::Arc;

use picoo_gpu::WindowsGpuContext;
use windows::Win32::Media::MediaFoundation::{IMFSample, IMFTransform, MFT_OUTPUT_DATA_BUFFER};

use crate::DecodeError;

pub(super) struct MftOutput {
    pub sample: Option<IMFSample>,
    pub result: windows::core::Result<()>,
    _runtime: Arc<dyn Send + Sync>,
}

// SAFETY: Standard MF output samples are free-threaded. The completion only
// retains/releases the sample; it does not mutate the image. Event collections
// remain on the MFT worker, and the transform/COM apartment never enter this owner.
// The runtime lease only notifies its dedicated shutdown thread on final release.
unsafe impl Send for MftOutput {}

impl MftOutput {
    unsafe fn process(&mut self, transform: &IMFTransform, provided: Option<IMFSample>) {
        let mut buffer = MFT_OUTPUT_DATA_BUFFER {
            dwStreamID: 0,
            pSample: ManuallyDrop::new(provided),
            dwStatus: 0,
            pEvents: ManuallyDrop::new(None),
        };
        let mut status = 0;
        self.result = transform.ProcessOutput(0, std::slice::from_mut(&mut buffer), &mut status);
        // Transfer the sample before any fallible output inspection; errors may
        // still leave an allocator sample attached to the native result.
        self.sample = ManuallyDrop::take(&mut buffer.pSample);
        drop(ManuallyDrop::take(&mut buffer.pEvents));
    }
}

pub(super) unsafe fn process(
    transform: &IMFTransform,
    gpu: Option<&Arc<WindowsGpuContext>>,
    provided: Option<IMFSample>,
    runtime: Arc<dyn Send + Sync>,
) -> Result<MftOutput, DecodeError> {
    let mut output = MftOutput {
        sample: None,
        result: Ok(()),
        _runtime: runtime,
    };
    if let Some(gpu) = gpu {
        // Reserve event/wait/capacity before consuming a transform output. MFT
        // may dispatch internal work, so no external context lock surrounds it.
        gpu.submit_owned(output, |_, output| {
            output.process(transform, provided);
            Ok(())
        })
        .and_then(|completion| completion.wait_on_worker())
        .map_err(|error| DecodeError::Platform(format!("MF GPU output completion: {error}")))
    } else {
        // Only the explicitly constructed software diagnostic reaches this arm.
        output.process(transform, provided);
        Ok(output)
    }
}
