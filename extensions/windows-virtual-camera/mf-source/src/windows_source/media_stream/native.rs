//! GpuNative Frame Server handoff and MF sample delivery.
//!
//! The consumer declares its negotiated fixed layout before the producer
//! creates renderer resources. All pipe, keyed-mutex and native-sample work is
//! isolated from the CpuBridge RequestSample path owned by the parent module.

use super::{lock, SharedStreamState, StreamState};
use crate::frame_provider::{FrameOrigin, LiveContentToken};
use picoo_frame_hub::{
    WindowsAdapterId, WindowsNativeChannel, WindowsNativeChannelAck, WindowsNativePipeClient,
    WindowsNativeWireMessage, WindowsSharedSurfaceFormat, WindowsSharedSurfaceIdentity,
};
use std::os::windows::io::{AsHandle, FromRawHandle, OwnedHandle};
use std::sync::{
    atomic::{AtomicBool, Ordering},
    Arc, Mutex, Weak,
};
use std::thread;
use std::time::{Duration, Instant};
use windows::core::{Error, IUnknown, Interface, Ref, Result, GUID, HRESULT};
use windows::Win32::Foundation::{GetHandleInformation, E_FAIL, E_INVALIDARG, HANDLE};
use windows::Win32::Media::MediaFoundation::{
    IMFSample, MEMediaSample, MFSampleExtension_Token, MF_E_MEDIA_SOURCE_WRONGSTATE, MF_E_SHUTDOWN,
    MF_STREAM_STATE_RUNNING,
};
use windows::Win32::System::Com::{CoInitializeEx, CoUninitialize, COINIT_MULTITHREADED};

use super::super::native_import::{import_nv12_surface, make_native_sample};
use super::super::producer_identity::is_expected_receiver_process;

const E_PENDING: HRESULT = HRESULT(0x8000000Au32 as i32);

pub(super) struct PreparedNativeSample {
    sample: IMFSample,
    release_committed: Arc<AtomicBool>,
    lifecycle_revision: u64,
    native_session_revision: u64,
    output_revision: u64,
    identity: WindowsSharedSurfaceIdentity,
    content_token: LiveContentToken,
}

// Media Foundation samples and the D3D11 manager are free-threaded objects in
// the Frame Server process. StreamState carries the corresponding explicit
// Send boundary and serializes mutation with its Mutex.
unsafe impl Send for PreparedNativeSample {}

impl PreparedNativeSample {
    pub(super) fn abort(self) {
        self.release_committed.store(true, Ordering::Release);
        drop(self.sample);
    }
}

fn prepare_native_sample(shared: &SharedStreamState) -> Result<PreparedNativeSample> {
    let (
        pipe,
        binding,
        lifecycle_revision,
        session_revision,
        output_width,
        output_height,
        output_revision,
        duration,
        frames,
    ) = {
        let mut state = lock(shared)?;
        let binding = state
            .native_device
            .as_ref()
            .cloned()
            .ok_or_else(|| Error::from(E_FAIL))?;
        let pipe = match state.native_pipe.as_ref() {
            Some(pipe) => Arc::clone(pipe),
            None => {
                let client = WindowsNativePipeClient::connect_with_server_validator(
                    is_expected_receiver_process,
                )
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
            state.native_session_revision,
            state.output_width,
            state.output_height,
            state.native_output_revision,
            state.sample_duration_100ns,
            Arc::clone(&state.frames),
        )
    };
    let content_token = frames
        .live_content_token()
        .ok_or_else(|| Error::from(E_PENDING))?;

    let pipe_guard = pipe
        .lock()
        .map_err(|_| Error::from(windows::Win32::Foundation::E_UNEXPECTED))?;
    let handshake_complete = {
        let state = lock(shared)?;
        if state.lifecycle_revision != lifecycle_revision
            || state.native_session_revision != session_revision
            || state.native_output_revision != output_revision
        {
            return Err(Error::from(MF_E_MEDIA_SOURCE_WRONGSTATE));
        }
        state.native_handshake
    };
    if !handshake_complete {
        pipe_guard
            .write_frame(
                &WindowsNativeWireMessage::Demand {
                    width: output_width,
                    height: output_height,
                    output_revision,
                }
                .encode()
                .map_err(|_| Error::from(E_INVALIDARG))?,
            )
            .map_err(|error| {
                reset_native_session_if_current(shared, session_revision);
                Error::new(E_FAIL, error.to_string())
            })?;
        let message = WindowsNativeWireMessage::decode(
            &pipe_guard
                .read_frame_timeout(Duration::from_millis(250))
                .map_err(|error| {
                    reset_native_session_if_current(shared, session_revision);
                    Error::new(E_FAIL, error.to_string())
                })?,
        )
        .map_err(|error| {
            reset_native_session_if_current(shared, session_revision);
            Error::new(E_FAIL, format!("native hello: {error:?}"))
        })?;
        let WindowsNativeWireMessage::Hello {
            source_connection_generation,
            stream_epoch,
            adapter,
            resource_generation,
            backend_generation,
            output_revision: producer_output_revision,
        } = message
        else {
            let _ = pipe_guard
                .write_frame(&WindowsNativeWireMessage::Close.encode().unwrap_or_default());
            reset_native_session_if_current(shared, session_revision);
            return Err(Error::from(E_INVALIDARG));
        };
        let expected_adapter =
            WindowsAdapterId::from_luid(binding.adapter.low, binding.adapter.high);
        if adapter != expected_adapter
            || source_connection_generation == 0
            || resource_generation == 0
            || backend_generation == 0
            || producer_output_revision != output_revision
        {
            let _ = pipe_guard
                .write_frame(&WindowsNativeWireMessage::Close.encode().unwrap_or_default());
            reset_native_session_if_current(shared, session_revision);
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
                reset_native_session_if_current(shared, session_revision);
                Error::new(E_FAIL, error.to_string())
            })?;
        let mut channel = WindowsNativeChannel::new(
            source_connection_generation,
            stream_epoch,
            adapter,
            resource_generation,
            backend_generation,
            output_revision,
        )
        .map_err(|_| {
            reset_native_session_if_current(shared, session_revision);
            Error::from(E_INVALIDARG)
        })?;
        channel.begin_handshake().map_err(|_| {
            reset_native_session_if_current(shared, session_revision);
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
                reset_native_session_if_current(shared, session_revision);
                Error::from(E_INVALIDARG)
            })?;
        let mut state = lock(shared)?;
        if state.lifecycle_revision != lifecycle_revision
            || state.native_session_revision != session_revision
            || state.native_output_revision != output_revision
        {
            drop(state);
            let _ = pipe_guard
                .write_frame(&WindowsNativeWireMessage::Close.encode().unwrap_or_default());
            return Err(Error::from(MF_E_MEDIA_SOURCE_WRONGSTATE));
        }
        state.native_handshake = true;
        state.native_handshake_identity = Some((
            source_connection_generation,
            stream_epoch,
            resource_generation,
            backend_generation,
            output_revision,
        ));
        state.native_channel = Some(Arc::new(Mutex::new(channel)));
    }
    let offer = WindowsNativeWireMessage::decode(
        &pipe_guard
            .read_frame_timeout(Duration::from_millis(250))
            .map_err(|error| {
                reset_native_session_if_current(shared, session_revision);
                Error::new(E_FAIL, error.to_string())
            })?,
    )
    .map_err(|error| {
        reset_native_session_if_current(shared, session_revision);
        Error::new(E_FAIL, format!("native offer: {error:?}"))
    })?;
    let WindowsNativeWireMessage::Offer {
        offer_id,
        descriptor,
    } = offer
    else {
        reset_native_session_if_current(shared, session_revision);
        return Err(Error::from(E_INVALIDARG));
    };
    // An authenticated producer transfers ownership of this target-process
    // HANDLE to the Consumer on every subsequent branch, including Rejected
    // paths. Validate the raw wire integer before constructing OwnedHandle.
    let offered_handle = match take_offered_handle(descriptor.handle_value()) {
        Ok(handle) => handle,
        Err(error) => {
            write_native_ack(&*pipe_guard, offer_id, WindowsNativeChannelAck::Rejected);
            reset_native_session_if_current(shared, session_revision);
            return Err(error);
        }
    };
    let native_channel = {
        let state = lock(shared)?;
        if state.lifecycle_revision != lifecycle_revision
            || state.native_session_revision != session_revision
            || state.native_output_revision != output_revision
        {
            return Err(Error::from(MF_E_MEDIA_SOURCE_WRONGSTATE));
        }
        state
            .native_channel
            .as_ref()
            .cloned()
            .ok_or_else(|| Error::from(E_INVALIDARG))?
    };
    let channel_offer_id = match native_channel
        .lock()
        .map_err(|_| Error::from(windows::Win32::Foundation::E_UNEXPECTED))?
        .offer_frame(descriptor)
    {
        Ok(offer_id) => offer_id,
        Err(_) => {
            write_native_ack(&*pipe_guard, offer_id, WindowsNativeChannelAck::Rejected);
            reset_native_session_if_current(shared, session_revision);
            return Err(Error::from(E_INVALIDARG));
        }
    };
    if channel_offer_id != offer_id {
        write_native_ack(&*pipe_guard, offer_id, WindowsNativeChannelAck::Rejected);
        reset_native_session_if_current(shared, session_revision);
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
        reset_native_session_if_current(shared, session_revision);
        return Err(Error::from(E_INVALIDARG));
    }
    drop(pipe_guard);
    let imported =
        match unsafe { import_nv12_surface(&binding, descriptor, offered_handle.as_handle()) } {
            Ok(imported) => imported,
            Err(error) => {
                send_native_ack(&pipe, offer_id, WindowsNativeChannelAck::Rejected);
                reset_native_session_if_current(shared, session_revision);
                return Err(Error::new(E_FAIL, error.to_string()));
            }
        };
    let current = lock(shared)?;
    let still_running = current.state == MF_STREAM_STATE_RUNNING
        && !current.transitioning
        && current.lifecycle_revision == lifecycle_revision
        && current.native_session_revision == session_revision
        && current.native_output_revision == output_revision;
    drop(current);
    if !still_running {
        send_native_ack(&pipe, offer_id, WindowsNativeChannelAck::Rejected);
        reset_native_session_if_current(shared, session_revision);
        return Err(Error::from(MF_E_MEDIA_SOURCE_WRONGSTATE));
    }
    let ack_pipe = Arc::clone(&pipe);
    let ack_channel = Arc::clone(&native_channel);
    // The prepared sample can live inside StreamState before delivery, so the
    // release callback must not hold a strong Arc back to that same state.
    let release_state = Arc::downgrade(shared);
    let release_ready = {
        let state = lock(shared)?;
        Arc::clone(&state.native_ready)
    };
    let release_committed = Arc::new(AtomicBool::new(false));
    let release_committed_for_ack = Arc::clone(&release_committed);
    let release_ack: Arc<dyn Fn(bool) + Send + Sync> = Arc::new(move |released| {
        if !release_committed_for_ack.load(Ordering::Acquire) {
            return;
        }
        let mut transport_failed = !released;
        if released {
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
        }
        let release_state = release_state.upgrade();
        let current_session = release_state.as_ref().is_some_and(|release_state| {
            if let Ok(mut state) = release_state.lock() {
                let current = state.native_session_revision == session_revision;
                if current {
                    state.native_inflight = false;
                }
                current
            } else {
                false
            }
        });
        if current_session {
            if let Ok(mut revision) = release_ready.0.lock() {
                *revision = revision.wrapping_add(1);
                release_ready.1.notify_all();
            }
            if transport_failed {
                reset_native_session_if_current(
                    release_state.as_ref().expect("current session has state"),
                    session_revision,
                );
            }
        }
    });
    let lease = match unsafe { make_native_sample(imported, 0, duration, Some(release_ack)) } {
        Ok(lease) => lease,
        Err(error) => {
            send_native_ack(&pipe, offer_id, WindowsNativeChannelAck::Rejected);
            reset_native_session_if_current(shared, session_revision);
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
        reset_native_session_if_current(shared, session_revision);
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
        reset_native_session_if_current(shared, session_revision);
        return Err(Error::from(E_FAIL));
    }
    Ok(PreparedNativeSample {
        sample: lease.sample,
        release_committed,
        lifecycle_revision,
        native_session_revision: session_revision,
        output_revision,
        identity: descriptor.identity(),
        content_token,
    })
}

pub(super) fn native_prepare_loop(weak: Weak<Mutex<StreamState>>) {
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
                    && !state.native_session_exhausted
                    && !state.native_placeholder_active
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
                    && !state.native_placeholder_active
                    && state.state == MF_STREAM_STATE_RUNNING
                    && !state.transitioning
                    && state.lifecycle_revision == prepared.lifecycle_revision
                    && state.native_session_revision == prepared.native_session_revision
                    && state.native_output_revision == prepared.output_revision
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

pub(super) fn deliver_native_sample(
    shared: &SharedStreamState,
    token: Ref<'_, IUnknown>,
) -> Result<FrameOrigin> {
    {
        let state = lock(shared)?;
        if state.native_session_exhausted || state.native_placeholder_active {
            return Err(Error::from(E_FAIL));
        }
    }
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
            .ok_or_else(|| Error::from(E_PENDING))?;
        state.native_inflight = true;
        prepared
    };
    let session_revision = prepared.native_session_revision;
    let (prepared, queue, sample_time, duration, frames) = {
        let mut state = lock(shared)?;
        if prepared.lifecycle_revision != state.lifecycle_revision
            || prepared.native_session_revision != state.native_session_revision
            || prepared.output_revision != state.native_output_revision
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
        (
            prepared,
            queue,
            sample_time,
            state.sample_duration_100ns,
            Arc::clone(&state.frames),
        )
    };
    // This header-only revision read is the final admission point for a live
    // sample. A Placeholder transition that linearized before it invalidates
    // the prepared surface without creating CPU pixel demand. A transition
    // after it races only with a sample already admitted as in-flight.
    if frames.live_content_token() != Some(prepared.content_token) {
        let stale = {
            let mut state = lock(shared)?;
            reset_native_session_state(&mut state)
        };
        prepared.abort();
        if let Some(stale) = stale {
            stale.abort();
        }
        return Err(Error::from(MF_E_MEDIA_SOURCE_WRONGSTATE));
    }
    let origin = {
        let mut state = lock(shared)?;
        if prepared.lifecycle_revision != state.lifecycle_revision
            || prepared.native_session_revision != state.native_session_revision
            || prepared.output_revision != state.native_output_revision
            || state.state != MF_STREAM_STATE_RUNNING
            || state.transitioning
        {
            drop(state);
            prepared.abort();
            return Err(Error::from(MF_E_MEDIA_SOURCE_WRONGSTATE));
        }
        classify_native_origin(&mut state.native_last_delivered_identity, prepared.identity)
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
        reset_native_session_if_current(shared, session_revision);
        return Err(error);
    }
    Ok(origin)
}

fn send_native_ack(
    pipe: &Arc<Mutex<WindowsNativePipeClient>>,
    offer_id: u64,
    ack: WindowsNativeChannelAck,
) {
    if let Ok(pipe) = pipe.lock() {
        write_native_ack(&*pipe, offer_id, ack);
    }
}

trait NativeAckSink {
    fn write_ack_frame(&self, bytes: &[u8]);
}

impl NativeAckSink for WindowsNativePipeClient {
    fn write_ack_frame(&self, bytes: &[u8]) {
        let _ = self.write_frame(bytes);
    }
}

/// Writes an acknowledgement through a pipe whose serialization lock is
/// already held. Rejection while decoding an Offer must use this path rather
/// than recursively acquiring the non-reentrant pipe mutex.
fn write_native_ack(pipe: &impl NativeAckSink, offer_id: u64, ack: WindowsNativeChannelAck) {
    if let Ok(bytes) = (WindowsNativeWireMessage::Ack { offer_id, ack }).encode() {
        pipe.write_ack_frame(&bytes);
    }
}

fn take_offered_handle(value: u64) -> Result<OwnedHandle> {
    let raw = usize::try_from(value).map_err(|_| Error::from(E_INVALIDARG))?;
    if raw == 0 || raw == usize::MAX {
        return Err(Error::from(E_INVALIDARG));
    }
    let handle = HANDLE(raw as *mut _);
    let mut flags = 0u32;
    unsafe { GetHandleInformation(handle, &mut flags) }.map_err(|_| Error::from(E_INVALIDARG))?;
    // SAFETY: the authenticated producer duplicated this non-pseudo handle
    // into the current process, and GetHandleInformation just confirmed that
    // it names a live handle. Ownership transfers exactly once here.
    Ok(unsafe { OwnedHandle::from_raw_handle(handle.0) })
}

fn reset_native_session_if_current(shared: &SharedStreamState, expected_revision: u64) {
    reset_native_session_inner(shared, Some(expected_revision));
}

fn reset_native_session_inner(shared: &SharedStreamState, expected_revision: Option<u64>) {
    let prepared = if let Ok(mut state) = shared.lock() {
        if expected_revision.is_some_and(|expected| expected != state.native_session_revision) {
            return;
        }
        reset_native_session_state(&mut state)
    } else {
        None
    };
    if let Some(prepared) = prepared {
        prepared.abort();
    }
}

pub(super) fn set_placeholder_active(shared: &SharedStreamState, active: bool) -> Result<()> {
    let lifecycle_operation = {
        let state = lock(shared)?;
        if state.native_placeholder_active == active {
            return Ok(());
        }
        Arc::clone(&state.lifecycle_operation)
    };
    let _operation = lock(&lifecycle_operation)?;
    let prepared = {
        let mut state = lock(shared)?;
        if state.native_placeholder_active == active {
            return Ok(());
        }
        state.native_placeholder_active = active;
        reset_native_session_state(&mut state)
    };
    if let Some(prepared) = prepared {
        prepared.abort();
    }
    Ok(())
}

pub(super) fn reset_native_session_state(state: &mut StreamState) -> Option<PreparedNativeSample> {
    state.native_pipe = None;
    state.native_handshake = false;
    state.native_handshake_identity = None;
    state.native_channel = None;
    state.native_inflight = false;
    state.native_last_delivered_identity = None;
    state.native_session_revision = match next_session_revision(state.native_session_revision) {
        Some(revision) => revision,
        None => {
            state.native_session_exhausted = true;
            u64::MAX
        }
    };
    state.native_prepared.take()
}

fn classify_native_origin(
    previous: &mut Option<WindowsSharedSurfaceIdentity>,
    identity: WindowsSharedSurfaceIdentity,
) -> FrameOrigin {
    let origin = if *previous == Some(identity) {
        FrameOrigin::Cached
    } else {
        FrameOrigin::Fresh
    };
    *previous = Some(identity);
    origin
}

fn next_session_revision(current: u64) -> Option<u64> {
    current.checked_add(1)
}

#[cfg(test)]
mod tests;
