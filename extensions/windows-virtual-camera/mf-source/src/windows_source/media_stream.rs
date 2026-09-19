use picoo_frame_hub::{
    WindowsAdapterId, WindowsNativeChannel, WindowsNativePipeClient, WindowsSharedSurfaceIdentity,
};
use std::sync::{Arc, Condvar, Mutex};
use std::thread::{self, JoinHandle};

use windows::core::{implement, Error, IUnknown, Interface, Ref, Result, GUID, HRESULT};
use windows::Win32::Foundation::{E_FAIL, E_INVALIDARG};
use windows::Win32::Media::KernelStreaming::PINNAME_VIDEO_CAPTURE;
use windows::Win32::Media::MediaFoundation::{
    IMFAsyncCallback, IMFAsyncResult, IMFMediaEvent, IMFMediaEventGenerator_Impl,
    IMFMediaEventQueue, IMFMediaSource, IMFMediaStream2, IMFMediaStream2_Impl, IMFMediaStream_Impl,
    IMFMediaType, IMFMediaTypeHandler, IMFSampleAllocatorControl, IMFSampleAllocatorControl_Impl,
    IMFStreamDescriptor, IMFVideoSampleAllocator, MEStreamStarted, MEStreamStopped,
    MFCreateEventQueue, MFCreateMediaType, MFCreateStreamDescriptor, MFFrameSourceTypes_Color,
    MFMediaType_Video, MFNominalRange_16_235, MFSampleAllocatorUsage,
    MFSampleAllocatorUsage_UsesCustomAllocator, MFSampleAllocatorUsage_UsesProvidedAllocator,
    MFVideoFormat_NV12, MFVideoInterlace_Progressive, MFVideoPrimaries_BT709, MFVideoTransFunc_709,
    MFVideoTransferMatrix_BT709, MEDIA_EVENT_GENERATOR_GET_EVENT_FLAGS,
    MF_DEVICESTREAM_ATTRIBUTE_FRAMESOURCE_TYPES, MF_DEVICESTREAM_FRAMESERVER_SHARED,
    MF_DEVICESTREAM_STREAM_CATEGORY, MF_DEVICESTREAM_STREAM_ID, MF_E_INVALIDREQUEST,
    MF_E_INVALIDSTREAMNUMBER, MF_E_INVALID_STATE_TRANSITION, MF_E_SHUTDOWN,
    MF_MT_ALL_SAMPLES_INDEPENDENT, MF_MT_AVG_BITRATE, MF_MT_COMPRESSED, MF_MT_DEFAULT_STRIDE,
    MF_MT_FIXED_SIZE_SAMPLES, MF_MT_FRAME_RATE, MF_MT_FRAME_SIZE, MF_MT_INTERLACE_MODE,
    MF_MT_MAJOR_TYPE, MF_MT_PIXEL_ASPECT_RATIO, MF_MT_SAMPLE_SIZE, MF_MT_SUBTYPE,
    MF_MT_TRANSFER_FUNCTION, MF_MT_VIDEO_NOMINAL_RANGE, MF_MT_VIDEO_PRIMARIES, MF_MT_YUV_MATRIX,
    MF_STREAM_STATE, MF_STREAM_STATE_RUNNING, MF_STREAM_STATE_STOPPED,
};
use windows::Win32::System::Com::StructuredStorage::PROPVARIANT;
use windows::Win32::System::Com::{IAgileObject, IAgileObject_Impl};

use crate::format::{
    is_supported_frame_rate, is_supported_output_size, nv12_len, sample_duration_100ns,
    DEFAULT_FRAME_RATE_DEN, DEFAULT_FRAME_RATE_NUM, FRAME_RATES,
};
use crate::frame_provider::FrameProvider;
use crate::metrics::{VcamMetrics, VcamMetricsSnapshot};
use crate::sample_clock::SampleClock;

use super::d3d_manager::NativeDeviceBinding;
use super::{lock, ObjectTracker};

mod cpu;
mod delivery;
mod native;

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
    native_output_revision: u64,
    frame_rate_num: u32,
    frame_rate_den: u32,
    sample_duration_100ns: i64,
    state: MF_STREAM_STATE,
    transitioning: bool,
    lifecycle_revision: u64,
    lifecycle_operation: Arc<Mutex<()>>,
    stream_id: u32,
    /// `Some(generation)` means SetD3DManager admitted native output. The
    /// native sample path must consume this generation; CPU delivery is not a
    /// permitted fallback while it is set.
    native_generation: Option<u64>,
    native_adapter: Option<WindowsAdapterId>,
    native_device: Option<NativeDeviceBinding>,
    native_pipe: Option<Arc<Mutex<WindowsNativePipeClient>>>,
    native_session_revision: u64,
    native_session_exhausted: bool,
    native_placeholder_active: bool,
    native_handshake: bool,
    native_handshake_identity: Option<(u64, u64, u64, u64, u64)>,
    native_channel: Option<Arc<Mutex<WindowsNativeChannel>>>,
    native_prepared: Option<native::PreparedNativeSample>,
    native_inflight: bool,
    native_last_delivered_identity: Option<WindowsSharedSurfaceIdentity>,
    native_ready: Arc<(Mutex<u64>, Condvar)>,
    native_worker: Option<JoinHandle<()>>,
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
            let mut media_types = Vec::with_capacity(2 * FRAME_RATES.len());
            for (width, height) in [(1280, 720), (1920, 1080)] {
                for (frame_rate_num, frame_rate_den) in FRAME_RATES {
                    media_types.push(Some(create_nv12_media_type(
                        width,
                        height,
                        frame_rate_num,
                        frame_rate_den,
                    )?));
                }
            }
            let descriptor = MFCreateStreamDescriptor(0, &media_types)?;
            let handler: IMFMediaTypeHandler = descriptor.GetMediaTypeHandler()?;
            // The type at index 0 is the registered 720p30 default. Reuse the
            // descriptor-owned object instead of setting an equivalent type
            // that is not one of its advertised entries.
            let default_type = media_types
                .first()
                .and_then(|media_type| media_type.as_ref())
                .ok_or_else(|| Error::from(E_INVALIDARG))?;
            handler.SetCurrentMediaType(default_type)?;
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
                current_type: Some(default_type.clone()),
                allocator: None,
                frames,
                metrics: VcamMetrics::new(),
                sample_clock: SampleClock::for_frame_rate(
                    DEFAULT_FRAME_RATE_NUM,
                    DEFAULT_FRAME_RATE_DEN,
                )
                .ok_or_else(|| Error::from(E_INVALIDARG))?,
                output_width: 1280,
                output_height: 720,
                native_output_revision: 1,
                frame_rate_num: DEFAULT_FRAME_RATE_NUM,
                frame_rate_den: DEFAULT_FRAME_RATE_DEN,
                sample_duration_100ns: sample_duration_100ns(
                    DEFAULT_FRAME_RATE_NUM,
                    DEFAULT_FRAME_RATE_DEN,
                )
                .ok_or_else(|| Error::from(E_INVALIDARG))?,
                state: MF_STREAM_STATE_STOPPED,
                transitioning: false,
                lifecycle_revision: 0,
                lifecycle_operation: Arc::new(Mutex::new(())),
                stream_id: 0,
                native_generation: None,
                native_adapter: None,
                native_device: None,
                native_pipe: None,
                native_session_revision: 1,
                native_session_exhausted: false,
                native_placeholder_active: false,
                native_handshake: false,
                native_handshake_identity: None,
                native_channel: None,
                native_prepared: None,
                native_inflight: false,
                native_last_delivered_identity: None,
                native_ready: Arc::new((Mutex::new(0), Condvar::new())),
                native_worker: None,
            }));
            let worker_state = Arc::downgrade(&shared);
            let worker_tracker = ObjectTracker::new();
            let worker = thread::Builder::new()
                .name("picoo-vcam-native-preparer".into())
                .spawn(move || {
                    let _tracker = worker_tracker;
                    native::native_prepare_loop(worker_state);
                })
                .map_err(|_| Error::from(E_FAIL))?;
            lock(&shared)?.native_worker = Some(worker);
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

pub(super) fn set_native_generation(
    shared: &SharedStreamState,
    generation: Option<u64>,
    binding: Option<NativeDeviceBinding>,
) -> Result<()> {
    let prepared = {
        let mut state = lock(shared)?;
        if state.queue.is_none() {
            return Err(Error::from(MF_E_SHUTDOWN));
        }
        if state.state == MF_STREAM_STATE_RUNNING || state.transitioning {
            return Err(Error::from(MF_E_INVALIDREQUEST));
        }
        state.native_generation = generation;
        state.native_placeholder_active = false;
        state.lifecycle_revision = state.lifecycle_revision.wrapping_add(1);
        state.native_adapter = binding
            .as_ref()
            .map(|binding| WindowsAdapterId::from_luid(binding.adapter.low, binding.adapter.high));
        state.native_device = binding;
        native::reset_native_session_state(&mut state)
    };
    if let Some(prepared) = prepared {
        prepared.abort();
    }
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
    let (queue, allocator, frames, prepared, worker) = {
        let mut state = lock(shared)?;
        state.state = MF_STREAM_STATE_STOPPED;
        state.transitioning = false;
        state.lifecycle_revision = state.lifecycle_revision.wrapping_add(1);
        state.source = None;
        state.descriptor = None;
        state.current_type = None;
        let prepared = native::reset_native_session_state(&mut state);
        (
            state.queue.take(),
            state.allocator.take(),
            Arc::clone(&state.frames),
            prepared,
            state.native_worker.take(),
        )
    };
    if let Some(prepared) = prepared {
        prepared.abort();
    }
    frames.shutdown();
    if let Some(allocator) = allocator {
        unsafe {
            let _ = allocator.UninitializeSampleAllocator();
        }
    }
    let queue_result = match queue {
        Some(queue) => unsafe { queue.Shutdown() },
        None => Ok(()),
    };
    if let Some(worker) = worker {
        let _ = worker.join();
    }
    queue_result?;
    Ok(())
}

pub(super) fn set_stream_state(
    shared: &SharedStreamState,
    requested: MF_STREAM_STATE,
) -> Result<()> {
    let lifecycle_operation = Arc::clone(&lock(shared)?.lifecycle_operation);
    let _operation = lock(&lifecycle_operation)?;
    let (previous, queue, allocator, current_type, frames, output_width, output_height, cpu_bridge) = {
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
        // GpuNative owns producer surfaces and therefore reports/uses a
        // custom allocator. The Frame Server allocator is only initialized
        // for the CpuBridge path.
        let allocator = if state.native_generation.is_none() {
            state.allocator.clone()
        } else {
            None
        };
        let current_type = state.current_type.clone();
        state.transitioning = true;
        (
            state.state,
            queue,
            allocator,
            current_type,
            Arc::clone(&state.frames),
            state.output_width,
            state.output_height,
            state.native_generation.is_none(),
        )
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
            frames.set_output_active(output_width, output_height, cpu_bridge);
            let event_result = unsafe {
                queue.QueueEventParamVar(
                    MEStreamStarted.0 as u32,
                    &GUID::zeroed(),
                    HRESULT(0),
                    std::ptr::null(),
                )
            };
            if let Err(error) = event_result {
                let prepared = {
                    let mut state = lock(shared)?;
                    if state.state == requested {
                        state.state = previous;
                        state.lifecycle_revision = state.lifecycle_revision.wrapping_add(1);
                    }
                    native::reset_native_session_state(&mut state)
                };
                if let Some(prepared) = prepared {
                    prepared.abort();
                }
                frames.set_output_active(output_width, output_height, false);
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
            let prepared = {
                let mut state = lock(shared)?;
                native::reset_native_session_state(&mut state)
            };
            if let Some(prepared) = prepared {
                prepared.abort();
            }
            frames.set_output_active(output_width, output_height, false);
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
                    drop(state);
                    frames.set_output_active(output_width, output_height, cpu_bridge);
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
    let packed_rate = unsafe { media_type.GetUINT64(&MF_MT_FRAME_RATE)? };
    let frame_rate_num = (packed_rate >> 32) as u32;
    let frame_rate_den = packed_rate as u32;
    if !is_supported_frame_rate(frame_rate_num, frame_rate_den) {
        return Err(Error::from(E_INVALIDARG));
    }
    let sample_duration = sample_duration_100ns(frame_rate_num, frame_rate_den)
        .ok_or_else(|| Error::from(E_INVALIDARG))?;
    unsafe {
        validate_output_media_type(media_type, width, height, frame_rate_num, frame_rate_den)?;
    }

    let prepared = {
        let mut state = lock(shared)?;
        if state.transitioning {
            return Err(Error::from(MF_E_INVALIDREQUEST));
        }
        if state.state != MF_STREAM_STATE_STOPPED {
            return if (state.output_width, state.output_height) == (width, height)
                && state.frame_rate_num == frame_rate_num
                && state.frame_rate_den == frame_rate_den
            {
                Ok(())
            } else {
                Err(Error::from(MF_E_INVALIDREQUEST))
            };
        }
        let changed = (state.output_width, state.output_height) != (width, height)
            || state.frame_rate_num != frame_rate_num
            || state.frame_rate_den != frame_rate_den;
        let next_output_revision = next_output_revision(state.native_output_revision, changed)
            .ok_or_else(|| Error::from(E_FAIL))?;
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
        state.native_output_revision = next_output_revision;
        state.frame_rate_num = frame_rate_num;
        state.frame_rate_den = frame_rate_den;
        state.sample_duration_100ns = sample_duration;
        state.sample_clock = SampleClock::for_frame_rate(frame_rate_num, frame_rate_den)
            .ok_or_else(|| Error::from(E_INVALIDARG))?;
        if changed {
            native::reset_native_session_state(&mut state)
        } else {
            None
        }
    };
    if let Some(prepared) = prepared {
        prepared.abort();
    }
    Ok(())
}

fn next_output_revision(current: u64, changed: bool) -> Option<u64> {
    if changed {
        current.checked_add(1)
    } else {
        Some(current)
    }
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
        usage.write(if state.native_generation.is_some() {
            MFSampleAllocatorUsage_UsesCustomAllocator
        } else {
            MFSampleAllocatorUsage_UsesProvidedAllocator
        });
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
        let result = delivery::deliver_sample(&self.shared, token);
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

fn create_nv12_media_type(
    width: u32,
    height: u32,
    frame_rate_num: u32,
    frame_rate_den: u32,
) -> Result<IMFMediaType> {
    if !is_supported_output_size(width, height) {
        return Err(Error::from(E_INVALIDARG));
    }
    if !is_supported_frame_rate(frame_rate_num, frame_rate_den) {
        return Err(Error::from(E_INVALIDARG));
    }
    let sample_size =
        u32::try_from(nv12_len(width, height).ok_or_else(|| Error::from(E_INVALIDARG))?)
            .map_err(|_| Error::from(E_INVALIDARG))?;
    let average_bitrate = sample_size
        .checked_mul(8)
        .and_then(|bits_per_frame| bits_per_frame.checked_mul(frame_rate_num))
        .and_then(|bits_per_second| bits_per_second.checked_div(frame_rate_den))
        .ok_or_else(|| Error::from(E_INVALIDARG))?;
    unsafe {
        let media_type = MFCreateMediaType()?;
        media_type.SetGUID(&MF_MT_MAJOR_TYPE, &MFMediaType_Video)?;
        media_type.SetGUID(&MF_MT_SUBTYPE, &MFVideoFormat_NV12)?;
        media_type.SetUINT32(&MF_MT_INTERLACE_MODE, MFVideoInterlace_Progressive.0 as u32)?;
        media_type.SetUINT32(&MF_MT_COMPRESSED, 0)?;
        media_type.SetUINT32(&MF_MT_FIXED_SIZE_SAMPLES, 1)?;
        media_type.SetUINT32(&MF_MT_ALL_SAMPLES_INDEPENDENT, 1)?;
        media_type.SetUINT32(&MF_MT_SAMPLE_SIZE, sample_size)?;
        media_type.SetUINT32(&MF_MT_AVG_BITRATE, average_bitrate)?;
        media_type.SetUINT64(&MF_MT_FRAME_SIZE, pack_u32_pair(width, height))?;
        media_type.SetUINT32(&MF_MT_DEFAULT_STRIDE, width)?;
        media_type.SetUINT64(
            &MF_MT_FRAME_RATE,
            pack_u32_pair(frame_rate_num, frame_rate_den),
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

unsafe fn validate_output_media_type(
    media_type: &IMFMediaType,
    width: u32,
    height: u32,
    frame_rate_num: u32,
    frame_rate_den: u32,
) -> Result<()> {
    if media_type.GetGUID(&MF_MT_MAJOR_TYPE)? != MFMediaType_Video
        || media_type.GetGUID(&MF_MT_SUBTYPE)? != MFVideoFormat_NV12
        || media_type.GetUINT32(&MF_MT_COMPRESSED)? != 0
        || media_type.GetUINT32(&MF_MT_INTERLACE_MODE)? != MFVideoInterlace_Progressive.0 as u32
        || media_type.GetUINT32(&MF_MT_FIXED_SIZE_SAMPLES)? != 1
        || media_type.GetUINT32(&MF_MT_ALL_SAMPLES_INDEPENDENT)? != 1
        || media_type.GetUINT32(&MF_MT_DEFAULT_STRIDE)? != width
        || media_type.GetUINT32(&MF_MT_YUV_MATRIX)? != MFVideoTransferMatrix_BT709.0 as u32
        || media_type.GetUINT32(&MF_MT_VIDEO_NOMINAL_RANGE)? != MFNominalRange_16_235.0 as u32
        || media_type.GetUINT32(&MF_MT_VIDEO_PRIMARIES)? != MFVideoPrimaries_BT709.0 as u32
        || media_type.GetUINT32(&MF_MT_TRANSFER_FUNCTION)? != MFVideoTransFunc_709.0 as u32
    {
        return Err(Error::from(E_INVALIDARG));
    }
    if media_type.GetUINT64(&MF_MT_FRAME_SIZE)? != pack_u32_pair(width, height)
        || media_type.GetUINT64(&MF_MT_FRAME_RATE)? != pack_u32_pair(frame_rate_num, frame_rate_den)
        || media_type.GetUINT64(&MF_MT_PIXEL_ASPECT_RATIO)? != pack_u32_pair(1, 1)
    {
        return Err(Error::from(E_INVALIDARG));
    }
    let sample_size =
        u32::try_from(nv12_len(width, height).ok_or_else(|| Error::from(E_INVALIDARG))?)
            .map_err(|_| Error::from(E_INVALIDARG))?;
    if media_type.GetUINT32(&MF_MT_SAMPLE_SIZE)? != sample_size {
        return Err(Error::from(E_INVALIDARG));
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

#[cfg(test)]
mod tests;
