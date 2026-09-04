use std::sync::{Arc, Mutex};

use windows::core::{implement, Error, IUnknown, Interface, Ref, Result, GUID, HRESULT};
use windows::Win32::Foundation::{E_FAIL, E_INVALIDARG};
use windows::Win32::Media::KernelStreaming::PINNAME_VIDEO_CAPTURE;
use windows::Win32::Media::MediaFoundation::{
    IMFAsyncCallback, IMFAsyncResult, IMFMediaEvent, IMFMediaEventGenerator_Impl,
    IMFMediaEventQueue, IMFMediaSource, IMFMediaStream2, IMFMediaStream2_Impl, IMFMediaStream_Impl,
    IMFMediaType, IMFMediaTypeHandler, IMFSample, IMFSampleAllocatorControl,
    IMFSampleAllocatorControl_Impl, IMFStreamDescriptor, IMFVideoSampleAllocator, MEMediaSample,
    MEStreamStarted, MEStreamStopped, MFCreateEventQueue, MFCreateMediaType, MFCreateMemoryBuffer,
    MFCreateSample, MFCreateStreamDescriptor, MFFrameSourceTypes_Color, MFMediaType_Video,
    MFNominalRange_16_235, MFSampleAllocatorUsage, MFSampleAllocatorUsage_UsesProvidedAllocator,
    MFSampleExtension_Token, MFVideoFormat_NV12, MFVideoInterlace_Progressive,
    MFVideoPrimaries_BT709, MFVideoTransFunc_709, MFVideoTransferMatrix_BT709,
    MEDIA_EVENT_GENERATOR_GET_EVENT_FLAGS, MF_DEVICESTREAM_ATTRIBUTE_FRAMESOURCE_TYPES,
    MF_DEVICESTREAM_FRAMESERVER_SHARED, MF_DEVICESTREAM_STREAM_CATEGORY, MF_DEVICESTREAM_STREAM_ID,
    MF_E_INVALIDREQUEST, MF_E_INVALIDSTREAMNUMBER, MF_E_INVALID_STATE_TRANSITION,
    MF_E_MEDIA_SOURCE_WRONGSTATE, MF_E_SHUTDOWN, MF_MT_ALL_SAMPLES_INDEPENDENT,
    MF_MT_DEFAULT_STRIDE, MF_MT_FRAME_RATE, MF_MT_FRAME_SIZE, MF_MT_INTERLACE_MODE,
    MF_MT_MAJOR_TYPE, MF_MT_PIXEL_ASPECT_RATIO, MF_MT_SUBTYPE, MF_MT_TRANSFER_FUNCTION,
    MF_MT_VIDEO_NOMINAL_RANGE, MF_MT_VIDEO_PRIMARIES, MF_MT_YUV_MATRIX, MF_STREAM_STATE,
    MF_STREAM_STATE_RUNNING, MF_STREAM_STATE_STOPPED,
};
use windows::Win32::System::Com::StructuredStorage::PROPVARIANT;
use windows::Win32::System::Com::{IAgileObject, IAgileObject_Impl};

use crate::format::{
    is_supported_output_size, nv12_len, FRAME_RATE_DEN, FRAME_RATE_NUM, SAMPLE_DURATION_100NS,
};
use crate::frame_provider::{FrameProvider, OwnedNv12Frame};
use crate::metrics::{VcamMetrics, VcamMetricsSnapshot};
use crate::sample_clock::SampleClock;

use super::{lock, ObjectTracker};

pub(super) type SharedStreamState = Arc<Mutex<StreamState>>;

pub(super) struct StreamState {
    source: Option<IMFMediaSource>,
    queue: Option<IMFMediaEventQueue>,
    descriptor: Option<IMFStreamDescriptor>,
    current_type: Option<IMFMediaType>,
    allocator: Option<IMFVideoSampleAllocator>,
    frames: Arc<FrameProvider>,
    metrics: VcamMetrics,
    sample_clock: SampleClock,
    output_width: u32,
    output_height: u32,
    state: MF_STREAM_STATE,
    transitioning: bool,
    lifecycle_revision: u64,
    lifecycle_operation: Arc<Mutex<()>>,
    stream_id: u32,
}

// SAFETY: the Media Foundation objects stored here are the platform's
// free-threaded event queue/descriptors/media types/allocator, plus Picoo's own
// IAgileObject media source. Every mutation is serialized by the containing
// Mutex. We intentionally assert that contract directly instead of calling
// RoGetAgileReference, which requires proxy registration these MF interfaces do
// not provide in the Frame Server process.
unsafe impl Send for StreamState {}

#[implement(IMFMediaStream2, IMFSampleAllocatorControl, IAgileObject)]
pub(super) struct MediaStream {
    shared: SharedStreamState,
    _tracker: ObjectTracker,
}

impl MediaStream {
    pub fn create() -> Result<(IMFMediaStream2, SharedStreamState)> {
        unsafe {
            let queue = MFCreateEventQueue()?;
            let type_480 = create_nv12_media_type(854, 480)?;
            let type_720 = create_nv12_media_type(1280, 720)?;
            let type_1080 = create_nv12_media_type(1920, 1080)?;
            let descriptor = MFCreateStreamDescriptor(
                0,
                &[Some(type_480), Some(type_720.clone()), Some(type_1080)],
            )?;
            let handler: IMFMediaTypeHandler = descriptor.GetMediaTypeHandler()?;
            handler.SetCurrentMediaType(&type_720)?;
            let frames = Arc::new(FrameProvider::new().map_err(|_| Error::from(E_FAIL))?);

            descriptor.SetGUID(&MF_DEVICESTREAM_STREAM_CATEGORY, &PINNAME_VIDEO_CAPTURE)?;
            descriptor.SetUINT32(&MF_DEVICESTREAM_STREAM_ID, 0)?;
            descriptor.SetUINT32(&MF_DEVICESTREAM_FRAMESERVER_SHARED, 1)?;
            descriptor.SetUINT32(
                &MF_DEVICESTREAM_ATTRIBUTE_FRAMESOURCE_TYPES,
                MFFrameSourceTypes_Color.0 as u32,
            )?;

            let shared = Arc::new(Mutex::new(StreamState {
                source: None,
                queue: Some(queue),
                descriptor: Some(descriptor),
                current_type: Some(type_720),
                allocator: None,
                frames,
                metrics: VcamMetrics::new(),
                sample_clock: SampleClock::new(SAMPLE_DURATION_100NS),
                output_width: 1280,
                output_height: 720,
                state: MF_STREAM_STATE_STOPPED,
                transitioning: false,
                lifecycle_revision: 0,
                lifecycle_operation: Arc::new(Mutex::new(())),
                stream_id: 0,
            }));
            let interface = Self {
                shared: Arc::clone(&shared),
                _tracker: ObjectTracker::new(),
            }
            .into();
            Ok((interface, shared))
        }
    }
}

pub(super) fn attach_source(shared: &SharedStreamState, source: IMFMediaSource) -> Result<()> {
    lock(shared)?.source = Some(source);
    Ok(())
}

pub(super) fn descriptor(shared: &SharedStreamState) -> Result<IMFStreamDescriptor> {
    lock(shared)?
        .descriptor
        .as_ref()
        .cloned()
        .ok_or_else(|| Error::from(MF_E_SHUTDOWN))
}

pub(super) fn shutdown(shared: &SharedStreamState) -> Result<()> {
    let lifecycle_operation = Arc::clone(&lock(shared)?.lifecycle_operation);
    let _operation = lock(&lifecycle_operation)?;
    let (queue, allocator, frames) = {
        let mut state = lock(shared)?;
        state.state = MF_STREAM_STATE_STOPPED;
        state.transitioning = false;
        state.lifecycle_revision = state.lifecycle_revision.wrapping_add(1);
        state.source = None;
        state.descriptor = None;
        state.current_type = None;
        (
            state.queue.take(),
            state.allocator.take(),
            Arc::clone(&state.frames),
        )
    };
    frames.shutdown();
    if let Some(allocator) = allocator {
        unsafe {
            let _ = allocator.UninitializeSampleAllocator();
        }
    }
    if let Some(queue) = queue {
        unsafe { queue.Shutdown()? };
    }
    Ok(())
}

pub(super) fn set_stream_state(
    shared: &SharedStreamState,
    requested: MF_STREAM_STATE,
) -> Result<()> {
    let lifecycle_operation = Arc::clone(&lock(shared)?.lifecycle_operation);
    let _operation = lock(&lifecycle_operation)?;
    let (previous, queue, allocator, current_type) = {
        let mut state = lock(shared)?;
        if state.queue.is_none() {
            return Err(Error::from(MF_E_SHUTDOWN));
        }
        if state.transitioning {
            return Err(Error::from(MF_E_INVALID_STATE_TRANSITION));
        }
        if state.state == requested {
            return Ok(());
        }
        match requested {
            MF_STREAM_STATE_RUNNING | MF_STREAM_STATE_STOPPED => {}
            _ => return Err(Error::from(MF_E_INVALID_STATE_TRANSITION)),
        }
        let queue = state
            .queue
            .clone()
            .ok_or_else(|| Error::from(MF_E_SHUTDOWN))?;
        let allocator = state.allocator.clone();
        let current_type = state.current_type.clone();
        state.transitioning = true;
        (state.state, queue, allocator, current_type)
    };

    let result = (|| {
        if requested == MF_STREAM_STATE_RUNNING {
            if let (Some(allocator), Some(media_type)) = (&allocator, &current_type) {
                unsafe { allocator.InitializeSampleAllocator(10, media_type)? };
            }
            let commit_result = {
                let mut state = lock(shared)?;
                if state.queue.is_none() {
                    Err(Error::from(MF_E_SHUTDOWN))
                } else if state.state != previous {
                    Err(Error::from(MF_E_INVALID_STATE_TRANSITION))
                } else {
                    state.state = requested;
                    state.sample_clock.reset();
                    state.lifecycle_revision = state.lifecycle_revision.wrapping_add(1);
                    Ok(())
                }
            };
            if let Err(error) = commit_result {
                if let Some(allocator) = allocator {
                    unsafe {
                        let _ = allocator.UninitializeSampleAllocator();
                    }
                }
                return Err(error);
            }
            let event_result = unsafe {
                queue.QueueEventParamVar(
                    MEStreamStarted.0 as u32,
                    &GUID::zeroed(),
                    HRESULT(0),
                    std::ptr::null(),
                )
            };
            if let Err(error) = event_result {
                let mut state = lock(shared)?;
                if state.state == requested {
                    state.state = previous;
                    state.lifecycle_revision = state.lifecycle_revision.wrapping_add(1);
                }
                drop(state);
                if let Some(allocator) = allocator {
                    unsafe {
                        let _ = allocator.UninitializeSampleAllocator();
                    }
                }
                return Err(error);
            }
        } else {
            if let Some(allocator) = &allocator {
                unsafe {
                    allocator.UninitializeSampleAllocator()?;
                }
            }
            let commit_result = {
                let mut state = lock(shared)?;
                if state.queue.is_none() {
                    Err(Error::from(MF_E_SHUTDOWN))
                } else if state.state != previous {
                    Err(Error::from(MF_E_INVALID_STATE_TRANSITION))
                } else {
                    state.state = requested;
                    state.lifecycle_revision = state.lifecycle_revision.wrapping_add(1);
                    Ok(())
                }
            };
            if let Err(error) = commit_result {
                if let (Some(allocator), Some(media_type)) = (&allocator, &current_type) {
                    let _ = unsafe { allocator.InitializeSampleAllocator(10, media_type) };
                }
                return Err(error);
            }
            let event_result = unsafe {
                queue.QueueEventParamVar(
                    MEStreamStopped.0 as u32,
                    &GUID::zeroed(),
                    HRESULT(0),
                    std::ptr::null(),
                )
            };
            if let Err(error) = event_result {
                let allocator_restored =
                    if let (Some(allocator), Some(media_type)) = (&allocator, &current_type) {
                        unsafe { allocator.InitializeSampleAllocator(10, media_type) }.is_ok()
                    } else {
                        true
                    };
                if allocator_restored {
                    let mut state = lock(shared)?;
                    if state.state == requested {
                        state.state = previous;
                        state.lifecycle_revision = state.lifecycle_revision.wrapping_add(1);
                    }
                }
                return Err(error);
            }
        }
        Ok(())
    })();

    lock(shared)?.transitioning = false;
    result
}

pub(super) fn set_default_allocator(
    shared: &SharedStreamState,
    output_stream_id: u32,
    allocator: Ref<'_, IUnknown>,
) -> Result<()> {
    let allocator = allocator.ok()?;
    let replacement = allocator.cast()?;
    let lifecycle_operation = Arc::clone(&lock(shared)?.lifecycle_operation);
    let _operation = lock(&lifecycle_operation)?;
    let previous = {
        let mut state = lock(shared)?;
        if state.queue.is_none() {
            return Err(Error::from(MF_E_SHUTDOWN));
        }
        if output_stream_id != state.stream_id {
            return Err(Error::from(MF_E_INVALIDSTREAMNUMBER));
        }
        if state.state != MF_STREAM_STATE_STOPPED {
            return Err(Error::from(MF_E_INVALIDREQUEST));
        }
        if state.transitioning {
            return Err(Error::from(MF_E_INVALIDREQUEST));
        }
        state.allocator.replace(replacement)
    };
    if let Some(previous) = previous {
        unsafe {
            let _ = previous.UninitializeSampleAllocator();
        }
    }
    Ok(())
}

pub(super) fn set_output_media_type(
    shared: &SharedStreamState,
    media_type: &IMFMediaType,
) -> Result<()> {
    let packed_size = unsafe { media_type.GetUINT64(&MF_MT_FRAME_SIZE)? };
    let width = (packed_size >> 32) as u32;
    let height = packed_size as u32;
    if !is_supported_output_size(width, height) {
        return Err(Error::from(E_INVALIDARG));
    }

    let mut state = lock(shared)?;
    if state.transitioning {
        return Err(Error::from(MF_E_INVALIDREQUEST));
    }
    if state.state != MF_STREAM_STATE_STOPPED {
        return if (state.output_width, state.output_height) == (width, height) {
            Ok(())
        } else {
            Err(Error::from(MF_E_INVALIDREQUEST))
        };
    }
    let descriptor = state
        .descriptor
        .as_ref()
        .cloned()
        .ok_or_else(|| Error::from(MF_E_SHUTDOWN))?;
    unsafe {
        descriptor
            .GetMediaTypeHandler()?
            .SetCurrentMediaType(media_type)?;
    }
    state.current_type = Some(media_type.clone());
    state.output_width = width;
    state.output_height = height;
    Ok(())
}

pub(super) fn allocator_usage(
    shared: &SharedStreamState,
    output_stream_id: u32,
    input_stream_id: *mut u32,
    usage: *mut MFSampleAllocatorUsage,
) -> Result<()> {
    let state = lock(shared)?;
    if output_stream_id != state.stream_id {
        return Err(Error::from(MF_E_INVALIDSTREAMNUMBER));
    }
    if usage.is_null() {
        return Err(Error::from(windows::Win32::Foundation::E_POINTER));
    }
    unsafe {
        if !input_stream_id.is_null() {
            input_stream_id.write(state.stream_id);
        }
        usage.write(MFSampleAllocatorUsage_UsesProvidedAllocator);
    }
    Ok(())
}

impl IMFMediaEventGenerator_Impl for MediaStream_Impl {
    fn GetEvent(&self, flags: MEDIA_EVENT_GENERATOR_GET_EVENT_FLAGS) -> Result<IMFMediaEvent> {
        let queue = stream_queue(&self.shared)?;
        unsafe { queue.GetEvent(flags.0) }
    }

    fn BeginGetEvent(
        &self,
        callback: Ref<'_, IMFAsyncCallback>,
        state: Ref<'_, IUnknown>,
    ) -> Result<()> {
        let queue = stream_queue(&self.shared)?;
        unsafe { queue.BeginGetEvent(callback.as_ref(), state.as_ref()) }
    }

    fn EndGetEvent(&self, result: Ref<'_, IMFAsyncResult>) -> Result<IMFMediaEvent> {
        let queue = stream_queue(&self.shared)?;
        unsafe { queue.EndGetEvent(result.as_ref()) }
    }

    fn QueueEvent(
        &self,
        event_type: u32,
        extended_type: *const GUID,
        status: HRESULT,
        value: *const PROPVARIANT,
    ) -> Result<()> {
        let queue = stream_queue(&self.shared)?;
        unsafe { queue.QueueEventParamVar(event_type, extended_type, status, value) }
    }
}

impl IMFMediaStream_Impl for MediaStream_Impl {
    fn GetMediaSource(&self) -> Result<IMFMediaSource> {
        lock(&self.shared)?
            .source
            .as_ref()
            .cloned()
            .ok_or_else(|| Error::from(MF_E_SHUTDOWN))
    }

    fn GetStreamDescriptor(&self) -> Result<IMFStreamDescriptor> {
        descriptor(&self.shared)
    }

    fn RequestSample(&self, token: Ref<'_, IUnknown>) -> Result<()> {
        let delivery_started = std::time::Instant::now();
        let result = deliver_sample(&self.shared, token);
        let origin = result.as_ref().ok().copied();
        let snapshot = lock(&self.shared)?
            .metrics
            .record_result(origin, delivery_started.elapsed());
        if let Some(snapshot) = snapshot {
            emit_metrics(snapshot);
        }
        result.map(|_| ())
    }
}

impl IMFMediaStream2_Impl for MediaStream_Impl {
    fn SetStreamState(&self, state: MF_STREAM_STATE) -> Result<()> {
        set_stream_state(&self.shared, state)
    }

    fn GetStreamState(&self) -> Result<MF_STREAM_STATE> {
        Ok(lock(&self.shared)?.state)
    }
}

impl IMFSampleAllocatorControl_Impl for MediaStream_Impl {
    fn SetDefaultAllocator(&self, stream_id: u32, allocator: Ref<'_, IUnknown>) -> Result<()> {
        set_default_allocator(&self.shared, stream_id, allocator)
    }

    fn GetAllocatorUsage(
        &self,
        stream_id: u32,
        input_stream_id: *mut u32,
        usage: *mut MFSampleAllocatorUsage,
    ) -> Result<()> {
        allocator_usage(&self.shared, stream_id, input_stream_id, usage)
    }
}

impl IAgileObject_Impl for MediaStream_Impl {}

fn stream_queue(shared: &SharedStreamState) -> Result<IMFMediaEventQueue> {
    lock(shared)?
        .queue
        .as_ref()
        .cloned()
        .ok_or_else(|| Error::from(MF_E_SHUTDOWN))
}

fn create_nv12_media_type(width: u32, height: u32) -> Result<IMFMediaType> {
    if !is_supported_output_size(width, height) {
        return Err(Error::from(E_INVALIDARG));
    }
    unsafe {
        let media_type = MFCreateMediaType()?;
        media_type.SetGUID(&MF_MT_MAJOR_TYPE, &MFMediaType_Video)?;
        media_type.SetGUID(&MF_MT_SUBTYPE, &MFVideoFormat_NV12)?;
        media_type.SetUINT32(&MF_MT_INTERLACE_MODE, MFVideoInterlace_Progressive.0 as u32)?;
        media_type.SetUINT32(&MF_MT_ALL_SAMPLES_INDEPENDENT, 1)?;
        media_type.SetUINT64(&MF_MT_FRAME_SIZE, pack_u32_pair(width, height))?;
        media_type.SetUINT32(&MF_MT_DEFAULT_STRIDE, width)?;
        media_type.SetUINT64(
            &MF_MT_FRAME_RATE,
            pack_u32_pair(FRAME_RATE_NUM, FRAME_RATE_DEN),
        )?;
        media_type.SetUINT64(&MF_MT_PIXEL_ASPECT_RATIO, pack_u32_pair(1, 1))?;
        media_type.SetUINT32(&MF_MT_YUV_MATRIX, MFVideoTransferMatrix_BT709.0 as u32)?;
        media_type.SetUINT32(&MF_MT_VIDEO_NOMINAL_RANGE, MFNominalRange_16_235.0 as u32)?;
        media_type.SetUINT32(&MF_MT_VIDEO_PRIMARIES, MFVideoPrimaries_BT709.0 as u32)?;
        media_type.SetUINT32(&MF_MT_TRANSFER_FUNCTION, MFVideoTransFunc_709.0 as u32)?;
        Ok(media_type)
    }
}

fn pack_u32_pair(high: u32, low: u32) -> u64 {
    ((high as u64) << 32) | low as u64
}

fn deliver_sample(
    shared: &SharedStreamState,
    token: Ref<'_, IUnknown>,
) -> Result<crate::frame_provider::FrameOrigin> {
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
    let frame = acquired.frame;
    if nv12_len(frame.width, frame.height) != Some(frame.pixels.len()) {
        return Err(Error::from(E_FAIL));
    }
    // Pixel conversion intentionally happens outside the lifecycle operation.
    // Revalidate immediately before touching MF objects so a Stop/Shutdown can
    // never uninitialize the allocator or overtake this sample event.
    let _operation = lock(&lifecycle_operation)?;
    let (allocator, queue, sample_time_100ns) = {
        let mut state = lock(shared)?;
        if state.state != MF_STREAM_STATE_RUNNING
            || state.transitioning
            || state.lifecycle_revision != lifecycle_revision
            || (state.output_width, state.output_height) != (output_width, output_height)
        {
            return Err(Error::from(MF_E_MEDIA_SOURCE_WRONGSTATE));
        }
        let allocator = state.allocator.as_ref().cloned();
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
        (allocator, queue, sample_time_100ns)
    };
    let sample = create_sample(
        allocator.as_ref(),
        &frame,
        token.as_ref(),
        sample_time_100ns,
    )?;

    unsafe {
        queue.QueueEventParamUnk(
            MEMediaSample.0 as u32,
            &GUID::zeroed(),
            HRESULT(0),
            &sample.cast::<IUnknown>()?,
        )?;
    }
    Ok(frame_origin)
}

fn emit_metrics(snapshot: VcamMetricsSnapshot) {
    let requests_per_second = if snapshot.elapsed_ms == 0 {
        0.0
    } else {
        snapshot.requests as f64 * 1_000.0 / snapshot.elapsed_ms as f64
    };
    let message = format!(
        "Picoo VCam metrics: requests_per_sec={requests_per_second:.1} requests={} fresh={} cached={} placeholder={} failed={} delivery_avg_us={} delivery_max_us={}\n",
        snapshot.requests,
        snapshot.fresh,
        snapshot.cached,
        snapshot.placeholder,
        snapshot.failed,
        snapshot.delivery_average_us,
        snapshot.delivery_max_us,
    );
    super::emit_debug_message(&message);
}

fn create_sample(
    allocator: Option<&IMFVideoSampleAllocator>,
    frame: &OwnedNv12Frame,
    token: Option<&IUnknown>,
    sample_time_100ns: i64,
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
        sample.SetSampleDuration(SAMPLE_DURATION_100NS)?;
        if let Some(token) = token {
            sample.SetUnknown(&MFSampleExtension_Token, token)?;
        }
        Ok(sample)
    }
}
