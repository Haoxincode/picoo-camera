use std::sync::{Arc, Mutex};

use windows::core::{
    implement, AgileReference, Error, IUnknown, Interface, Ref, Result, GUID, HRESULT,
};
use windows::Win32::Foundation::{E_FAIL, E_INVALIDARG};
use windows::Win32::Media::KernelStreaming::PINNAME_VIDEO_CAPTURE;
use windows::Win32::Media::MediaFoundation::{
    IMFAsyncCallback, IMFAsyncResult, IMFMediaEvent, IMFMediaEventGenerator_Impl,
    IMFMediaEventQueue, IMFMediaSource, IMFMediaStream2, IMFMediaStream2_Impl, IMFMediaStream_Impl,
    IMFMediaType, IMFMediaTypeHandler, IMFSample, IMFSampleAllocatorControl,
    IMFSampleAllocatorControl_Impl, IMFStreamDescriptor, IMFVideoSampleAllocator, MEMediaSample,
    MEStreamFormatChanged, MEStreamStarted, MEStreamStopped, MFCreateEventQueue, MFCreateMediaType,
    MFCreateMemoryBuffer, MFCreateSample, MFCreateStreamDescriptor, MFFrameSourceTypes_Color,
    MFMediaType_Video, MFNominalRange_16_235, MFSampleAllocatorUsage,
    MFSampleAllocatorUsage_UsesProvidedAllocator, MFSampleExtension_Token, MFVideoFormat_NV12,
    MFVideoInterlace_Progressive, MFVideoPrimaries_BT709, MFVideoTransFunc_709,
    MFVideoTransferMatrix_BT709, MEDIA_EVENT_GENERATOR_GET_EVENT_FLAGS,
    MF_DEVICESTREAM_ATTRIBUTE_FRAMESOURCE_TYPES, MF_DEVICESTREAM_FRAMESERVER_SHARED,
    MF_DEVICESTREAM_STREAM_CATEGORY, MF_DEVICESTREAM_STREAM_ID, MF_E_INVALIDSTREAMNUMBER,
    MF_E_INVALID_STATE_TRANSITION, MF_E_MEDIA_SOURCE_WRONGSTATE, MF_E_SHUTDOWN,
    MF_MT_ALL_SAMPLES_INDEPENDENT, MF_MT_DEFAULT_STRIDE, MF_MT_FRAME_RATE, MF_MT_FRAME_SIZE,
    MF_MT_INTERLACE_MODE, MF_MT_MAJOR_TYPE, MF_MT_PIXEL_ASPECT_RATIO, MF_MT_SUBTYPE,
    MF_MT_TRANSFER_FUNCTION, MF_MT_VIDEO_NOMINAL_RANGE, MF_MT_VIDEO_PRIMARIES, MF_MT_YUV_MATRIX,
    MF_STREAM_STATE, MF_STREAM_STATE_RUNNING, MF_STREAM_STATE_STOPPED,
};
use windows::Win32::System::Com::StructuredStorage::PROPVARIANT;
use windows::Win32::System::Com::{IAgileObject, IAgileObject_Impl};

use crate::format::{
    is_supported_output_size, nv12_len, FRAME_RATE_DEN, FRAME_RATE_NUM, SAMPLE_DURATION_100NS,
};
use crate::frame_provider::{FrameProvider, OwnedNv12Frame};
use crate::metrics::{VcamMetrics, VcamMetricsSnapshot};

use super::{lock, ObjectTracker};

pub(super) type SharedStreamState = Arc<Mutex<StreamState>>;

pub(super) struct StreamState {
    source: Option<AgileReference<IMFMediaSource>>,
    queue: Option<AgileReference<IMFMediaEventQueue>>,
    descriptor: Option<AgileReference<IMFStreamDescriptor>>,
    current_type: Option<AgileReference<IMFMediaType>>,
    allocator: Option<AgileReference<IMFVideoSampleAllocator>>,
    frames: FrameProvider,
    metrics: VcamMetrics,
    output_width: u32,
    output_height: u32,
    state: MF_STREAM_STATE,
    stream_id: u32,
}

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

            descriptor.SetGUID(&MF_DEVICESTREAM_STREAM_CATEGORY, &PINNAME_VIDEO_CAPTURE)?;
            descriptor.SetUINT32(&MF_DEVICESTREAM_STREAM_ID, 0)?;
            descriptor.SetUINT32(&MF_DEVICESTREAM_FRAMESERVER_SHARED, 1)?;
            descriptor.SetUINT32(
                &MF_DEVICESTREAM_ATTRIBUTE_FRAMESOURCE_TYPES,
                MFFrameSourceTypes_Color.0 as u32,
            )?;

            let shared = Arc::new(Mutex::new(StreamState {
                source: None,
                queue: Some(AgileReference::new(&queue)?),
                descriptor: Some(AgileReference::new(&descriptor)?),
                current_type: Some(AgileReference::new(&type_720)?),
                allocator: None,
                frames: FrameProvider::new(),
                metrics: VcamMetrics::new(),
                output_width: 1280,
                output_height: 720,
                state: MF_STREAM_STATE_STOPPED,
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
    lock(shared)?.source = Some(AgileReference::new(&source)?);
    Ok(())
}

pub(super) fn descriptor(shared: &SharedStreamState) -> Result<IMFStreamDescriptor> {
    lock(shared)?
        .descriptor
        .as_ref()
        .ok_or_else(|| Error::from(MF_E_SHUTDOWN))
        .and_then(AgileReference::resolve)
}

pub(super) fn shutdown(shared: &SharedStreamState) -> Result<()> {
    let (queue, allocator) = {
        let mut state = lock(shared)?;
        state.state = MF_STREAM_STATE_STOPPED;
        state.source = None;
        state.descriptor = None;
        state.current_type = None;
        (state.queue.take(), state.allocator.take())
    };
    if let Some(allocator) = allocator {
        let allocator = allocator.resolve()?;
        unsafe {
            let _ = allocator.UninitializeSampleAllocator();
        }
    }
    if let Some(queue) = queue {
        let queue = queue.resolve()?;
        unsafe { queue.Shutdown()? };
    }
    Ok(())
}

pub(super) fn set_stream_state(
    shared: &SharedStreamState,
    requested: MF_STREAM_STATE,
) -> Result<()> {
    let (queue, allocator, current_type, changed) = {
        let mut state = lock(shared)?;
        if state.queue.is_none() {
            return Err(Error::from(MF_E_SHUTDOWN));
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
        state.state = requested;
        (queue, allocator, current_type, true)
    };

    let queue = queue.resolve()?;
    let allocator = allocator.map(|value| value.resolve()).transpose()?;
    let current_type = current_type.map(|value| value.resolve()).transpose()?;

    if changed && requested == MF_STREAM_STATE_RUNNING {
        if let (Some(allocator), Some(media_type)) = (allocator, current_type) {
            unsafe { allocator.InitializeSampleAllocator(10, &media_type)? };
        }
        unsafe {
            queue.QueueEventParamVar(
                MEStreamStarted.0 as u32,
                &GUID::zeroed(),
                HRESULT(0),
                std::ptr::null(),
            )?;
        }
    } else if changed {
        if let Some(allocator) = allocator {
            unsafe {
                let _ = allocator.UninitializeSampleAllocator();
            }
        }
        unsafe {
            queue.QueueEventParamVar(
                MEStreamStopped.0 as u32,
                &GUID::zeroed(),
                HRESULT(0),
                std::ptr::null(),
            )?;
        }
    }
    Ok(())
}

pub(super) fn set_default_allocator(
    shared: &SharedStreamState,
    output_stream_id: u32,
    allocator: Ref<'_, IUnknown>,
) -> Result<()> {
    let mut state = lock(shared)?;
    if output_stream_id != state.stream_id {
        return Err(Error::from(MF_E_INVALIDSTREAMNUMBER));
    }
    let allocator = allocator.ok()?;
    state.allocator = Some(AgileReference::new(&allocator.cast()?)?);
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
            .ok_or_else(|| Error::from(MF_E_SHUTDOWN))
            .and_then(AgileReference::resolve)
    }

    fn GetStreamDescriptor(&self) -> Result<IMFStreamDescriptor> {
        descriptor(&self.shared)
    }

    fn RequestSample(&self, token: Ref<'_, IUnknown>) -> Result<()> {
        let delivery_started = std::time::Instant::now();
        lock(&self.shared)?.metrics.record_request();
        let result = deliver_sample(&self.shared, token, delivery_started);
        if result.is_err() {
            let snapshot = lock(&self.shared)?
                .metrics
                .record_failure(delivery_started.elapsed());
            if let Some(snapshot) = snapshot {
                emit_metrics(snapshot);
            }
        }
        result
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
        .ok_or_else(|| Error::from(MF_E_SHUTDOWN))?
        .resolve()
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

fn ensure_output_format(state: &mut StreamState, frame: &OwnedNv12Frame) -> Result<bool> {
    if (frame.width, frame.height) == (state.output_width, state.output_height) {
        return Ok(false);
    }
    let media_type = create_nv12_media_type(frame.width, frame.height)?;
    if let Some(descriptor) = &state.descriptor {
        let descriptor = descriptor.resolve()?;
        unsafe {
            descriptor
                .GetMediaTypeHandler()?
                .SetCurrentMediaType(&media_type)?;
        }
    }
    if let Some(allocator) = &state.allocator {
        let allocator = allocator.resolve()?;
        unsafe {
            let _ = allocator.UninitializeSampleAllocator();
            if state.state == MF_STREAM_STATE_RUNNING {
                allocator.InitializeSampleAllocator(10, &media_type)?;
            }
        }
    }
    state.current_type = Some(AgileReference::new(&media_type)?);
    state.output_width = frame.width;
    state.output_height = frame.height;
    Ok(true)
}

fn deliver_sample(
    shared: &SharedStreamState,
    token: Ref<'_, IUnknown>,
    delivery_started: std::time::Instant,
) -> Result<()> {
    let (sample, queue, format_changed, current_type, frame_origin) = {
        let mut state = lock(shared)?;
        if state.state != MF_STREAM_STATE_RUNNING {
            return Err(Error::from(MF_E_MEDIA_SOURCE_WRONGSTATE));
        }
        let acquired = state.frames.acquire();
        let frame_origin = acquired.origin;
        let frame = acquired.frame;
        if nv12_len(frame.width, frame.height) != Some(frame.pixels.len()) {
            return Err(Error::from(E_FAIL));
        }
        let format_changed = ensure_output_format(&mut state, &frame)?;
        let allocator = state
            .allocator
            .as_ref()
            .map(AgileReference::resolve)
            .transpose()?;
        let sample = create_sample(allocator.as_ref(), &frame, token.as_ref())?;
        let queue = state
            .queue
            .as_ref()
            .ok_or_else(|| Error::from(MF_E_SHUTDOWN))?
            .resolve()?;
        let current_type = state
            .current_type
            .as_ref()
            .map(AgileReference::resolve)
            .transpose()?;
        (sample, queue, format_changed, current_type, frame_origin)
    };

    unsafe {
        if format_changed {
            let media_type = current_type.ok_or_else(|| Error::from(E_FAIL))?;
            queue.QueueEventParamUnk(
                MEStreamFormatChanged.0 as u32,
                &GUID::zeroed(),
                HRESULT(0),
                &media_type.cast::<IUnknown>()?,
            )?;
        }
        queue.QueueEventParamUnk(
            MEMediaSample.0 as u32,
            &GUID::zeroed(),
            HRESULT(0),
            &sample.cast::<IUnknown>()?,
        )?;
    }
    let snapshot = lock(shared)?
        .metrics
        .record_delivery(frame_origin, delivery_started.elapsed());
    if let Some(snapshot) = snapshot {
        emit_metrics(snapshot);
    }
    Ok(())
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
        let copy_result = if destination.is_null() || capacity < frame.pixels.len() as u32 {
            Err(Error::from(E_FAIL))
        } else {
            std::ptr::copy_nonoverlapping(frame.pixels.as_ptr(), destination, frame.pixels.len());
            buffer.SetCurrentLength(frame.pixels.len() as u32)
        };
        let unlock_result = buffer.Unlock();
        copy_result?;
        unlock_result?;

        sample.SetSampleTime(windows::Win32::Media::MediaFoundation::MFGetSystemTime())?;
        sample.SetSampleDuration(SAMPLE_DURATION_100NS)?;
        if let Some(token) = token {
            sample.SetUnknown(&MFSampleExtension_Token, token)?;
        }
        Ok(sample)
    }
}
