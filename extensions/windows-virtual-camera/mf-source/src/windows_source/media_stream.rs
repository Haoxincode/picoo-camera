use picoo_frame_hub::{
    WindowsAdapterId, WindowsNativeChannel, WindowsNativeChannelAck, WindowsNativePipeClient,
    WindowsNativeWireMessage, WindowsSharedSurfaceFormat,
};
use std::sync::{
    atomic::{AtomicBool, Ordering},
    Arc, Condvar, Mutex, Weak,
};
use std::thread;
use std::time::{Duration, Instant};

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
    MF_E_MEDIA_SOURCE_WRONGSTATE, MF_E_SHUTDOWN, MF_E_UNSUPPORTED_BYTESTREAM_TYPE,
    MF_MT_ALL_SAMPLES_INDEPENDENT, MF_MT_AVG_BITRATE, MF_MT_COMPRESSED, MF_MT_DEFAULT_STRIDE,
    MF_MT_FIXED_SIZE_SAMPLES, MF_MT_FRAME_RATE, MF_MT_FRAME_SIZE, MF_MT_INTERLACE_MODE,
    MF_MT_MAJOR_TYPE, MF_MT_PIXEL_ASPECT_RATIO, MF_MT_SAMPLE_SIZE, MF_MT_SUBTYPE,
    MF_MT_TRANSFER_FUNCTION, MF_MT_VIDEO_NOMINAL_RANGE, MF_MT_VIDEO_PRIMARIES, MF_MT_YUV_MATRIX,
    MF_STREAM_STATE, MF_STREAM_STATE_RUNNING, MF_STREAM_STATE_STOPPED,
};
use windows::Win32::System::Com::StructuredStorage::PROPVARIANT;
use windows::Win32::System::Com::{
    CoInitializeEx, CoUninitialize, IAgileObject, IAgileObject_Impl, COINIT_MULTITHREADED,
};

use crate::format::{
    is_supported_frame_rate, is_supported_output_size, nv12_len, sample_duration_100ns,
    DEFAULT_FRAME_RATE_DEN, DEFAULT_FRAME_RATE_NUM, FRAME_RATES,
};
use crate::frame_provider::{FrameProvider, OwnedNv12Frame};
use crate::metrics::{VcamMetrics, VcamMetricsSnapshot};
use crate::sample_clock::SampleClock;

use super::d3d_manager::NativeDeviceBinding;
use super::native_import::{import_nv12_surface, make_native_sample};
use super::{lock, ObjectTracker};

pub(super) type SharedStreamState = Arc<Mutex<StreamState>>;

struct PreparedNativeSample {
    sample: IMFSample,
    release_committed: Arc<AtomicBool>,
    lifecycle_revision: u64,
}

// Media Foundation samples and the D3D11 manager are free-threaded objects in
// the Frame Server process. StreamState already carries the corresponding
// explicit Send boundary; keep the prepared sample on the same boundary.
unsafe impl Send for PreparedNativeSample {}

impl PreparedNativeSample {
    fn abort(self) {
        self.release_committed.store(true, Ordering::Release);
        drop(self.sample);
    }
}

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
    native_handshake: bool,
    native_handshake_identity: Option<(u64, u64, u64, u64, u64)>,
    native_channel: Option<Arc<Mutex<WindowsNativeChannel>>>,
    native_prepared: Option<PreparedNativeSample>,
    native_inflight: bool,
    native_ready: Arc<(Mutex<u64>, Condvar)>,
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
                native_handshake: false,
                native_handshake_identity: None,
                native_channel: None,
                native_prepared: None,
                native_inflight: false,
                native_ready: Arc::new((Mutex::new(0), Condvar::new())),
            }));
            let worker_state = Arc::downgrade(&shared);
            thread::Builder::new()
                .name("picoo-vcam-native-preparer".into())
                .spawn(move || native_prepare_loop(worker_state))
                .map_err(|_| Error::from(E_FAIL))?;
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
        state.lifecycle_revision = state.lifecycle_revision.wrapping_add(1);
        state.native_adapter = binding
            .as_ref()
            .map(|binding| WindowsAdapterId::from_luid(binding.adapter.low, binding.adapter.high));
        state.native_device = binding;
        state.native_pipe = None;
        state.native_handshake = false;
        state.native_handshake_identity = None;
        state.native_channel = None;
        state.native_inflight = false;
        state.native_prepared.take()
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
    let (queue, allocator, frames, prepared) = {
        let mut state = lock(shared)?;
        state.state = MF_STREAM_STATE_STOPPED;
        state.transitioning = false;
        state.lifecycle_revision = state.lifecycle_revision.wrapping_add(1);
        state.source = None;
        state.descriptor = None;
        state.current_type = None;
        state.native_inflight = false;
        (
            state.queue.take(),
            state.allocator.take(),
            Arc::clone(&state.frames),
            state.native_prepared.take(),
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
    let (previous, queue, allocator, current_type, frames, output_width, output_height) = {
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
        (
            state.state,
            queue,
            allocator,
            current_type,
            Arc::clone(&state.frames),
            state.output_width,
            state.output_height,
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
            frames.set_output_active(output_width, output_height, true);
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
            let prepared = lock(shared)?.native_prepared.take();
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
                    frames.set_output_active(output_width, output_height, true);
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
    state.frame_rate_num = frame_rate_num;
    state.frame_rate_den = frame_rate_den;
    state.sample_duration_100ns = sample_duration;
    state.sample_clock = SampleClock::for_frame_rate(frame_rate_num, frame_rate_den)
        .ok_or_else(|| Error::from(E_INVALIDARG))?;
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

fn deliver_sample(
    shared: &SharedStreamState,
    token: Ref<'_, IUnknown>,
) -> Result<crate::frame_provider::FrameOrigin> {
    let native = {
        let state = lock(shared)?;
        if state.state != MF_STREAM_STATE_RUNNING || state.transitioning {
            return Err(Error::from(MF_E_MEDIA_SOURCE_WRONGSTATE));
        }
        state.native_generation.is_some()
    };
    if native {
        return deliver_native_sample(shared, token);
    }
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
    let (allocator, queue, sample_time_100ns, sample_duration_100ns) = {
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

fn prepare_native_sample(shared: &SharedStreamState) -> Result<PreparedNativeSample> {
    let (pipe, binding, lifecycle_revision, output_width, output_height, allocator, duration) = {
        let mut state = lock(shared)?;
        let binding = state
            .native_device
            .as_ref()
            .cloned()
            .ok_or_else(|| Error::from(E_FAIL))?;
        let pipe = match state.native_pipe.as_ref() {
            Some(pipe) => Arc::clone(pipe),
            None => {
                let client = WindowsNativePipeClient::connect()
                    .map_err(|error| Error::new(E_FAIL, error.to_string()))?;
                let pipe = Arc::new(Mutex::new(client));
                state.native_pipe = Some(Arc::clone(&pipe));
                pipe
            }
        };
        (
            pipe,
            binding,
            state.lifecycle_revision,
            state.output_width,
            state.output_height,
            state.allocator.clone(),
            state.sample_duration_100ns,
        )
    };

    let mut pipe_guard = pipe
        .lock()
        .map_err(|_| Error::from(windows::Win32::Foundation::E_UNEXPECTED))?;
    let handshake_needed = lock(shared)?.native_handshake;
    if !handshake_needed {
        let message = WindowsNativeWireMessage::decode(
            &pipe_guard
                .read_frame_timeout(Duration::from_millis(250))
                .map_err(|error| {
                    reset_native_session(shared);
                    Error::new(E_FAIL, error.to_string())
                })?,
        )
        .map_err(|error| {
            reset_native_session(shared);
            Error::new(E_FAIL, format!("native hello: {error:?}"))
        })?;
        let WindowsNativeWireMessage::Hello {
            source_connection_generation,
            stream_epoch,
            adapter,
            resource_generation,
            backend_generation,
            output_revision,
        } = message
        else {
            let _ = pipe_guard
                .write_frame(&WindowsNativeWireMessage::Close.encode().unwrap_or_default());
            reset_native_session(shared);
            return Err(Error::from(E_INVALIDARG));
        };
        let expected_adapter =
            WindowsAdapterId::from_luid(binding.adapter.low, binding.adapter.high);
        if adapter != expected_adapter
            || source_connection_generation == 0
            || resource_generation == 0
            || backend_generation == 0
        {
            let _ = pipe_guard
                .write_frame(&WindowsNativeWireMessage::Close.encode().unwrap_or_default());
            reset_native_session(shared);
            return Err(Error::from(E_INVALIDARG));
        }
        let ready = WindowsNativeWireMessage::Ready {
            source_connection_generation,
            stream_epoch,
            resource_generation,
            backend_generation,
            output_revision,
        };
        pipe_guard
            .write_frame(&ready.encode().map_err(|_| Error::from(E_INVALIDARG))?)
            .map_err(|error| {
                reset_native_session(shared);
                Error::new(E_FAIL, error.to_string())
            })?;
        lock(shared)?.native_handshake = true;
        lock(shared)?.native_handshake_identity = Some((
            source_connection_generation,
            stream_epoch,
            resource_generation,
            backend_generation,
            output_revision,
        ));
        let mut channel = WindowsNativeChannel::new(
            source_connection_generation,
            stream_epoch,
            adapter,
            resource_generation,
            backend_generation,
            output_revision,
        )
        .map_err(|_| {
            reset_native_session(shared);
            Error::from(E_INVALIDARG)
        })?;
        channel.begin_handshake().map_err(|_| {
            reset_native_session(shared);
            Error::from(E_INVALIDARG)
        })?;
        channel
            .accept_ready(
                source_connection_generation,
                stream_epoch,
                resource_generation,
                backend_generation,
                output_revision,
            )
            .map_err(|_| {
                reset_native_session(shared);
                Error::from(E_INVALIDARG)
            })?;
        lock(shared)?.native_channel = Some(Arc::new(Mutex::new(channel)));
    }
    let offer = WindowsNativeWireMessage::decode(
        &pipe_guard
            .read_frame_timeout(Duration::from_millis(250))
            .map_err(|error| {
                reset_native_session(shared);
                Error::new(E_FAIL, error.to_string())
            })?,
    )
    .map_err(|error| {
        reset_native_session(shared);
        Error::new(E_FAIL, format!("native offer: {error:?}"))
    })?;
    let WindowsNativeWireMessage::Offer {
        offer_id,
        descriptor,
    } = offer
    else {
        reset_native_session(shared);
        return Err(Error::from(E_INVALIDARG));
    };
    let native_channel = lock(shared)?
        .native_channel
        .as_ref()
        .cloned()
        .ok_or_else(|| Error::from(E_INVALIDARG))?;
    let channel_offer_id = match native_channel
        .lock()
        .map_err(|_| Error::from(windows::Win32::Foundation::E_UNEXPECTED))?
        .offer_frame(descriptor)
    {
        Ok(offer_id) => offer_id,
        Err(_) => {
            send_native_ack(&pipe, offer_id, WindowsNativeChannelAck::Rejected);
            reset_native_session(shared);
            return Err(Error::from(E_INVALIDARG));
        }
    };
    if channel_offer_id != offer_id {
        send_native_ack(&pipe, offer_id, WindowsNativeChannelAck::Rejected);
        reset_native_session(shared);
        return Err(Error::from(E_INVALIDARG));
    }
    if descriptor.format() != WindowsSharedSurfaceFormat::Nv12
        || descriptor.size() != (output_width, output_height)
        || !lock(shared)?
            .native_handshake_identity
            .is_some_and(|identity| {
                let descriptor_identity = descriptor.identity();
                (
                    descriptor_identity.source_connection_generation,
                    descriptor_identity.stream_epoch,
                    descriptor_identity.resource_generation,
                    descriptor_identity.backend_generation,
                    descriptor_identity.output_revision,
                ) == identity
            })
    {
        let rejected = WindowsNativeWireMessage::Ack {
            offer_id,
            ack: WindowsNativeChannelAck::Rejected,
        };
        let _ = pipe_guard.write_frame(&rejected.encode().unwrap_or_default());
        let _ = native_channel
            .lock()
            .map_err(|_| ())
            .and_then(|mut channel| {
                channel
                    .acknowledge(offer_id, WindowsNativeChannelAck::Rejected)
                    .map_err(|_| ())
            });
        reset_native_session(shared);
        return Err(Error::from(E_INVALIDARG));
    }
    drop(pipe_guard);
    let imported = match unsafe { import_nv12_surface(&binding, descriptor) } {
        Ok(imported) => imported,
        Err(error) => {
            send_native_ack(&pipe, offer_id, WindowsNativeChannelAck::Rejected);
            reset_native_session(shared);
            return Err(Error::new(E_FAIL, error.to_string()));
        }
    };
    let current = lock(shared)?;
    let still_running = current.state == MF_STREAM_STATE_RUNNING
        && !current.transitioning
        && current.lifecycle_revision == lifecycle_revision;
    drop(current);
    if !still_running {
        send_native_ack(&pipe, offer_id, WindowsNativeChannelAck::Rejected);
        reset_native_session(shared);
        return Err(Error::from(MF_E_MEDIA_SOURCE_WRONGSTATE));
    }
    let ack_pipe = Arc::clone(&pipe);
    let ack_channel = Arc::clone(&native_channel);
    let release_state = Arc::clone(shared);
    let release_ready = {
        let state = lock(shared)?;
        Arc::clone(&state.native_ready)
    };
    let release_committed = Arc::new(AtomicBool::new(false));
    let release_committed_for_ack = Arc::clone(&release_committed);
    let release_ack: Arc<dyn Fn() + Send + Sync> = Arc::new(move || {
        if !release_committed_for_ack.load(Ordering::Acquire) {
            return;
        }
        let mut transport_failed = false;
        if let Ok(pipe) = ack_pipe.lock() {
            let message = WindowsNativeWireMessage::Ack {
                offer_id,
                ack: WindowsNativeChannelAck::Released,
            };
            if let Ok(bytes) = message.encode() {
                transport_failed = pipe.write_frame(&bytes).is_err();
            } else {
                transport_failed = true;
            }
        } else {
            transport_failed = true;
        }
        if let Ok(mut channel) = ack_channel.lock() {
            if channel
                .acknowledge(offer_id, WindowsNativeChannelAck::Released)
                .is_err()
            {
                transport_failed = true;
            }
        } else {
            transport_failed = true;
        }
        if let Ok(mut state) = release_state.lock() {
            state.native_inflight = false;
        }
        if let Ok(mut revision) = release_ready.0.lock() {
            *revision = revision.wrapping_add(1);
            release_ready.1.notify_all();
        }
        if transport_failed {
            reset_native_session(&release_state);
        }
    });
    let lease = match unsafe {
        make_native_sample(imported, 0, duration, Some(release_ack), allocator.as_ref())
    } {
        Ok(lease) => lease,
        Err(error) => {
            send_native_ack(&pipe, offer_id, WindowsNativeChannelAck::Rejected);
            reset_native_session(shared);
            return Err(error);
        }
    };
    let imported_state_ok = match native_channel.lock() {
        Ok(mut channel) => channel
            .acknowledge(offer_id, WindowsNativeChannelAck::Imported)
            .is_ok(),
        Err(_) => false,
    };
    if !imported_state_ok {
        reset_native_session(shared);
        return Err(Error::from(E_INVALIDARG));
    }
    let imported_sent = if let Ok(pipe) = pipe.lock() {
        WindowsNativeWireMessage::Ack {
            offer_id,
            ack: WindowsNativeChannelAck::Imported,
        }
        .encode()
        .ok()
        .is_some_and(|bytes| pipe.write_frame(&bytes).is_ok())
    } else {
        false
    };
    if !imported_sent {
        reset_native_session(shared);
        return Err(Error::from(E_FAIL));
    }
    Ok(PreparedNativeSample {
        sample: lease.sample,
        release_committed,
        lifecycle_revision,
    })
}

fn native_prepare_loop(weak: Weak<Mutex<StreamState>>) {
    let com_initialized = unsafe { CoInitializeEx(None, COINIT_MULTITHREADED).is_ok() };
    loop {
        let Some(shared) = weak.upgrade() else {
            if com_initialized {
                unsafe { CoUninitialize() };
            }
            return;
        };
        let should_prepare = match shared.lock() {
            Ok(state) => {
                if state.queue.is_none() {
                    if com_initialized {
                        unsafe { CoUninitialize() };
                    }
                    return;
                }
                state.native_generation.is_some()
                    && state.native_device.is_some()
                    && state.state == MF_STREAM_STATE_RUNNING
                    && !state.transitioning
                    && state.native_prepared.is_none()
                    && !state.native_inflight
            }
            Err(_) => {
                if com_initialized {
                    unsafe { CoUninitialize() };
                }
                return;
            }
        };
        if !should_prepare {
            thread::sleep(Duration::from_millis(10));
            continue;
        }
        match prepare_native_sample(&shared) {
            Ok(prepared) => {
                let mut state = match shared.lock() {
                    Ok(state) => state,
                    Err(_) => {
                        if com_initialized {
                            unsafe { CoUninitialize() };
                        }
                        return;
                    }
                };
                if state.native_generation.is_some()
                    && state.state == MF_STREAM_STATE_RUNNING
                    && !state.transitioning
                    && state.lifecycle_revision == prepared.lifecycle_revision
                    && state.native_prepared.is_none()
                    && !state.native_inflight
                {
                    state.native_prepared = Some(prepared);
                    let (ready_lock, ready_cv) = &*state.native_ready;
                    if let Ok(mut revision) = ready_lock.lock() {
                        *revision = revision.wrapping_add(1);
                        ready_cv.notify_all();
                    }
                } else {
                    drop(state);
                    prepared.abort();
                }
            }
            Err(_) => thread::sleep(Duration::from_millis(25)),
        }
    }
}

fn deliver_native_sample(
    shared: &SharedStreamState,
    token: Ref<'_, IUnknown>,
) -> Result<crate::frame_provider::FrameOrigin> {
    let lifecycle_operation = Arc::clone(&lock(shared)?.lifecycle_operation);
    let ready = Arc::clone(&lock(shared)?.native_ready);
    let deadline = Instant::now() + Duration::from_millis(100);
    loop {
        let prepared = lock(shared)?.native_prepared.is_some();
        if prepared || Instant::now() >= deadline {
            break;
        }
        let wait = deadline.saturating_duration_since(Instant::now());
        let guard = ready
            .0
            .lock()
            .map_err(|_| Error::from(windows::Win32::Foundation::E_UNEXPECTED))?;
        let _ = ready
            .1
            .wait_timeout(guard, wait)
            .map_err(|_| Error::from(windows::Win32::Foundation::E_UNEXPECTED))?;
    }
    let _operation = lock(&lifecycle_operation)?;
    let prepared = {
        let mut state = lock(shared)?;
        if state.state != MF_STREAM_STATE_RUNNING || state.transitioning {
            return Err(Error::from(MF_E_MEDIA_SOURCE_WRONGSTATE));
        }
        let prepared = state
            .native_prepared
            .take()
            .ok_or_else(|| Error::from(windows::Win32::Foundation::E_PENDING))?;
        state.native_inflight = true;
        prepared
    };
    let (queue, sample_time, duration) = {
        let mut state = lock(shared)?;
        if prepared.lifecycle_revision != state.lifecycle_revision
            || state.state != MF_STREAM_STATE_RUNNING
            || state.transitioning
        {
            drop(state);
            prepared.abort();
            return Err(Error::from(MF_E_MEDIA_SOURCE_WRONGSTATE));
        }
        let now = unsafe { windows::Win32::Media::MediaFoundation::MFGetSystemTime() };
        let sample_time = match state.sample_clock.next_timestamp(now) {
            Some(sample_time) => sample_time,
            None => {
                drop(state);
                prepared.abort();
                return Err(Error::from(E_FAIL));
            }
        };
        let queue = match state.queue.as_ref().cloned() {
            Some(queue) => queue,
            None => {
                drop(state);
                prepared.abort();
                return Err(Error::from(MF_E_SHUTDOWN));
            }
        };
        (prepared, queue, sample_time, state.sample_duration_100ns)
    };
    prepared.release_committed.store(true, Ordering::Release);
    if let Err(error) = unsafe {
        prepared.sample.SetSampleTime(sample_time)?;
        prepared.sample.SetSampleDuration(duration)?;
        if let Some(token) = token.as_ref() {
            prepared
                .sample
                .SetUnknown(&MFSampleExtension_Token, token)?;
        }
        queue.QueueEventParamUnk(
            MEMediaSample.0 as u32,
            &GUID::zeroed(),
            HRESULT(0),
            &prepared.sample.cast::<IUnknown>()?,
        )
    } {
        prepared.abort();
        reset_native_session(shared);
        return Err(error);
    }
    Ok(crate::frame_provider::FrameOrigin::Fresh)
}

fn send_native_ack(
    pipe: &Arc<Mutex<WindowsNativePipeClient>>,
    offer_id: u64,
    ack: WindowsNativeChannelAck,
) {
    if let Ok(pipe) = pipe.lock() {
        if let Ok(bytes) = (WindowsNativeWireMessage::Ack { offer_id, ack }).encode() {
            let _ = pipe.write_frame(&bytes);
        }
    }
}

fn reset_native_session(shared: &SharedStreamState) {
    let prepared = if let Ok(mut state) = shared.lock() {
        state.native_pipe = None;
        state.native_handshake = false;
        state.native_handshake_identity = None;
        state.native_channel = None;
        state.native_inflight = false;
        state.native_prepared.take()
    } else {
        None
    };
    if let Some(prepared) = prepared {
        prepared.abort();
    }
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
