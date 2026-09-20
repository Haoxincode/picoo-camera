//! CpuBridge sample delivery.
//!
//! Prepared ring pixels are copied exactly once into the final MF buffer. GPU
//! native surfaces and their synchronization remain in the sibling module.

use super::{lock, SharedStreamState};
use crate::format::nv12_len;
use crate::frame_provider::{FrameOrigin, OwnedNv12Frame};
use std::sync::Arc;
use windows::core::{Error, IUnknown, Interface, Ref, Result, GUID, HRESULT};
use windows::Win32::Foundation::E_FAIL;
use windows::Win32::Media::MediaFoundation::{
    IMFMediaEventQueue, IMFSample, IMFVideoSampleAllocator, MEMediaSample, MFCreateMemoryBuffer,
    MFCreateSample, MFSampleExtension_Token, MF_E_MEDIA_SOURCE_WRONGSTATE, MF_E_SHUTDOWN,
    MF_STREAM_STATE_RUNNING,
};

pub(super) fn deliver_cpu_sample(
    shared: &SharedStreamState,
    token: Ref<'_, IUnknown>,
) -> Result<FrameOrigin> {
    deliver_cpu_sample_inner(shared, token, false)
}

pub(super) fn deliver_placeholder_sample(
    shared: &SharedStreamState,
    token: Ref<'_, IUnknown>,
) -> Result<FrameOrigin> {
    deliver_cpu_sample_inner(shared, token, true)
}

fn deliver_cpu_sample_inner(
    shared: &SharedStreamState,
    token: Ref<'_, IUnknown>,
    require_placeholder: bool,
) -> Result<FrameOrigin> {
    let (frames, lifecycle_operation, lifecycle_revision, output_width, output_height) = {
        let state = lock(shared)?;
        if state.state != MF_STREAM_STATE_RUNNING || state.transitioning {
            return Err(Error::from(MF_E_MEDIA_SOURCE_WRONGSTATE));
        }
        (
            Arc::clone(&state.frames),
            Arc::clone(&state.lifecycle_operation),
            state.lifecycle_revision,
            state.output_width,
            state.output_height,
        )
    };
    let acquired = frames
        .acquire_for_output(output_width, output_height)
        .ok_or_else(|| Error::from(E_FAIL))?;
    let frame_origin = acquired.origin;
    if require_placeholder && frame_origin != FrameOrigin::Placeholder {
        return Err(Error::from(MF_E_MEDIA_SOURCE_WRONGSTATE));
    }
    let frame = acquired.frame;
    if nv12_len(frame.width, frame.height) != Some(frame.pixels.len()) {
        return Err(Error::from(E_FAIL));
    }
    // Pixel conversion intentionally happens outside the lifecycle operation.
    // Revalidate immediately before touching MF objects so Stop/Shutdown
    // cannot uninitialize the allocator or overtake the sample event.
    let _operation = lock(&lifecycle_operation)?;
    let (allocator, queue, sample_time_100ns, sample_duration_100ns) = {
        let mut state = lock(shared)?;
        if state.state != MF_STREAM_STATE_RUNNING
            || state.transitioning
            || state.lifecycle_revision != lifecycle_revision
            || (state.output_width, state.output_height) != (output_width, output_height)
        {
            return Err(Error::from(MF_E_MEDIA_SOURCE_WRONGSTATE));
        }
        // Explicit placeholders in GpuNative mode are source-owned memory
        // samples. A retained CpuBridge allocator is uninitialized while the
        // D3D manager binding is active and must not be reused here.
        let allocator = if require_placeholder {
            None
        } else {
            state.allocator.as_ref().cloned()
        };
        let queue = state
            .queue
            .as_ref()
            .cloned()
            .ok_or_else(|| Error::from(MF_E_SHUTDOWN))?;
        let now_100ns = unsafe { windows::Win32::Media::MediaFoundation::MFGetSystemTime() };
        let sample_time_100ns = state
            .sample_clock
            .next_timestamp(now_100ns)
            .ok_or_else(|| Error::from(E_FAIL))?;
        (
            allocator,
            queue,
            sample_time_100ns,
            state.sample_duration_100ns,
        )
    };
    let sample = create_sample(
        allocator.as_ref(),
        &frame,
        token.as_ref(),
        sample_time_100ns,
        sample_duration_100ns,
    )?;

    queue_sample(&queue, &sample)?;
    Ok(frame_origin)
}

fn queue_sample(queue: &IMFMediaEventQueue, sample: &IMFSample) -> Result<()> {
    unsafe {
        queue.QueueEventParamUnk(
            MEMediaSample.0 as u32,
            &GUID::zeroed(),
            HRESULT(0),
            &sample.cast::<IUnknown>()?,
        )
    }
}

fn create_sample(
    allocator: Option<&IMFVideoSampleAllocator>,
    frame: &OwnedNv12Frame,
    token: Option<&IUnknown>,
    sample_time_100ns: i64,
    sample_duration_100ns: i64,
) -> Result<IMFSample> {
    unsafe {
        let sample = if let Some(allocator) = allocator {
            allocator.AllocateSample()?
        } else {
            let sample = MFCreateSample()?;
            let buffer = MFCreateMemoryBuffer(frame.pixels.len() as u32)?;
            sample.AddBuffer(&buffer)?;
            sample
        };

        if sample.GetBufferCount()? == 0 {
            return Err(Error::from(E_FAIL));
        }
        let buffer = sample.GetBufferByIndex(0)?;
        let mut destination = std::ptr::null_mut();
        let mut capacity = 0u32;
        buffer.Lock(&mut destination, Some(&mut capacity), None)?;
        let copy_result = if destination.is_null() {
            Err(Error::from(E_FAIL))
        } else {
            let destination = std::slice::from_raw_parts_mut(destination, capacity as usize);
            crate::copy_prepared_frame(&frame.pixels, destination)
                .map_err(|_| Error::from(E_FAIL))
                .and_then(|copied| buffer.SetCurrentLength(copied as u32))
        };
        let unlock_result = buffer.Unlock();
        copy_result?;
        unlock_result?;

        sample.SetSampleTime(sample_time_100ns)?;
        sample.SetSampleDuration(sample_duration_100ns)?;
        if let Some(token) = token {
            sample.SetUnknown(&MFSampleExtension_Token, token)?;
        }
        Ok(sample)
    }
}
