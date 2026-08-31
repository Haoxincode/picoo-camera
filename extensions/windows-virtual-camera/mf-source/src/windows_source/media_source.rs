use std::ffi::c_void;
use std::sync::Mutex;

use windows::core::{
    implement, AgileReference, Error, IUnknown, Interface, Ref, Result, BOOL, GUID, HRESULT,
};
use windows::Win32::Media::MediaFoundation::{
    IMFAsyncCallback, IMFAsyncResult, IMFAttributes, IMFGetService, IMFGetService_Impl,
    IMFMediaEvent, IMFMediaEventGenerator_Impl, IMFMediaEventQueue, IMFMediaSource,
    IMFMediaSourceEx, IMFMediaSourceEx_Impl, IMFMediaSource_Impl, IMFMediaStream2, IMFMediaType,
    IMFSampleAllocatorControl, IMFSampleAllocatorControl_Impl, IMFStreamDescriptor, MENewStream,
    MESourceStarted, MESourceStopped, MEUpdatedStream, MFCreateEventQueue,
    MFCreatePresentationDescriptor, MFSampleAllocatorUsage, MEDIA_EVENT_GENERATOR_GET_EVENT_FLAGS,
    MFMEDIASOURCE_IS_LIVE, MF_E_INVALIDSTREAMNUMBER, MF_E_INVALID_STATE_TRANSITION, MF_E_SHUTDOWN,
    MF_E_UNSUPPORTED_SERVICE,
};
use windows::Win32::System::Com::StructuredStorage::PROPVARIANT;
use windows::Win32::System::Com::{IAgileObject, IAgileObject_Impl};

use super::media_stream::{
    allocator_usage, attach_source, descriptor, set_default_allocator, set_output_media_type,
    set_stream_state, shutdown, MediaStream, SharedStreamState,
};
use super::{lock, query_interface, ObjectTracker};

const MF_SAMPLEALLOCATOR_SERVICE: GUID = GUID::from_u128(0xbbcd045d_4d8b_49e6_9d72_6c60c22a445b);

struct SourceState {
    queue: Option<AgileReference<IMFMediaEventQueue>>,
    presentation:
        Option<AgileReference<windows::Win32::Media::MediaFoundation::IMFPresentationDescriptor>>,
    stream: Option<AgileReference<IMFMediaStream2>>,
    stream_state: SharedStreamState,
    shutdown: bool,
    stream_presented: bool,
}

#[implement(
    IMFMediaSourceEx,
    IMFGetService,
    IMFSampleAllocatorControl,
    IAgileObject
)]
pub(super) struct MediaSource {
    attributes: AgileReference<IMFAttributes>,
    state: Mutex<SourceState>,
    _tracker: ObjectTracker,
}

impl MediaSource {
    pub fn create(attributes: IMFAttributes) -> Result<IMFMediaSourceEx> {
        unsafe {
            let queue = MFCreateEventQueue()?;
            let (stream, stream_state) = MediaStream::create()?;
            let stream_descriptor = descriptor(&stream_state)?;
            let presentation = MFCreatePresentationDescriptor(Some(&[Some(stream_descriptor)]))?;

            let source: IMFMediaSourceEx = Self {
                attributes: AgileReference::new(&attributes)?,
                state: Mutex::new(SourceState {
                    queue: Some(AgileReference::new(&queue)?),
                    presentation: Some(AgileReference::new(&presentation)?),
                    stream: Some(AgileReference::new(&stream)?),
                    stream_state: stream_state.clone(),
                    shutdown: false,
                    stream_presented: false,
                }),
                _tracker: ObjectTracker::new(),
            }
            .into();
            attach_source(&stream_state, source.cast::<IMFMediaSource>()?)?;
            Ok(source)
        }
    }
}

impl IMFMediaEventGenerator_Impl for MediaSource_Impl {
    fn GetEvent(&self, flags: MEDIA_EVENT_GENERATOR_GET_EVENT_FLAGS) -> Result<IMFMediaEvent> {
        let queue = source_queue(&self.state)?;
        unsafe { queue.GetEvent(flags.0) }
    }

    fn BeginGetEvent(
        &self,
        callback: Ref<'_, IMFAsyncCallback>,
        state: Ref<'_, IUnknown>,
    ) -> Result<()> {
        let queue = source_queue(&self.state)?;
        unsafe { queue.BeginGetEvent(callback.as_ref(), state.as_ref()) }
    }

    fn EndGetEvent(&self, result: Ref<'_, IMFAsyncResult>) -> Result<IMFMediaEvent> {
        let queue = source_queue(&self.state)?;
        unsafe { queue.EndGetEvent(result.as_ref()) }
    }

    fn QueueEvent(
        &self,
        event_type: u32,
        extended_type: *const GUID,
        status: HRESULT,
        value: *const PROPVARIANT,
    ) -> Result<()> {
        let queue = source_queue(&self.state)?;
        unsafe { queue.QueueEventParamVar(event_type, extended_type, status, value) }
    }
}

impl IMFMediaSource_Impl for MediaSource_Impl {
    fn GetCharacteristics(&self) -> Result<u32> {
        ensure_source_alive(&self.state)?;
        Ok(MFMEDIASOURCE_IS_LIVE.0 as u32)
    }

    fn CreatePresentationDescriptor(
        &self,
    ) -> Result<windows::Win32::Media::MediaFoundation::IMFPresentationDescriptor> {
        let state = lock(&self.state)?;
        if state.shutdown {
            return Err(Error::from(MF_E_SHUTDOWN));
        }
        state
            .presentation
            .as_ref()
            .ok_or_else(|| Error::from(MF_E_SHUTDOWN))?
            .resolve()
    }

    fn Start(
        &self,
        presentation: Ref<'_, windows::Win32::Media::MediaFoundation::IMFPresentationDescriptor>,
        _time_format: *const GUID,
        _start_position: *const PROPVARIANT,
    ) -> Result<()> {
        let mut selected_media_type: Option<IMFMediaType> = None;
        if let Some(presentation) = presentation.as_ref() {
            unsafe {
                let mut selected = BOOL(0);
                let mut descriptor: Option<IMFStreamDescriptor> = None;
                presentation.GetStreamDescriptorByIndex(0, &mut selected, &mut descriptor)?;
                if !selected.as_bool() {
                    presentation.SelectStream(0)?;
                }
                if let Some(descriptor) = descriptor {
                    selected_media_type =
                        Some(descriptor.GetMediaTypeHandler()?.GetCurrentMediaType()?);
                }
            }
        }

        let (queue, stream, stream_state, first_presentation) = {
            let state = lock(&self.state)?;
            if state.shutdown {
                return Err(Error::from(MF_E_SHUTDOWN));
            }
            let queue = state
                .queue
                .as_ref()
                .ok_or_else(|| Error::from(MF_E_SHUTDOWN))?
                .resolve()?;
            let stream = state
                .stream
                .as_ref()
                .ok_or_else(|| Error::from(MF_E_SHUTDOWN))?
                .resolve()?;
            let first = !state.stream_presented;
            (queue, stream, state.stream_state.clone(), first)
        };

        if let Some(media_type) = selected_media_type.as_ref() {
            set_output_media_type(&stream_state, media_type)?;
        }

        let stream_unknown = stream.cast::<IUnknown>()?;
        unsafe {
            queue.QueueEventParamUnk(
                if first_presentation {
                    MENewStream.0 as u32
                } else {
                    MEUpdatedStream.0 as u32
                },
                &GUID::zeroed(),
                HRESULT(0),
                &stream_unknown,
            )?;
        }
        if first_presentation {
            let mut state = lock(&self.state)?;
            if state.shutdown {
                return Err(Error::from(MF_E_SHUTDOWN));
            }
            state.stream_presented = true;
        }
        set_stream_state(
            &stream_state,
            windows::Win32::Media::MediaFoundation::MF_STREAM_STATE_RUNNING,
        )?;
        let started = unsafe {
            queue.QueueEventParamVar(
                MESourceStarted.0 as u32,
                &GUID::zeroed(),
                HRESULT(0),
                std::ptr::null(),
            )
        };
        if let Err(error) = started {
            let _ = set_stream_state(
                &stream_state,
                windows::Win32::Media::MediaFoundation::MF_STREAM_STATE_STOPPED,
            );
            return Err(error);
        }
        Ok(())
    }

    fn Stop(&self) -> Result<()> {
        let (queue, stream_state) = {
            let state = lock(&self.state)?;
            if state.shutdown {
                return Err(Error::from(MF_E_SHUTDOWN));
            }
            (
                state
                    .queue
                    .as_ref()
                    .ok_or_else(|| Error::from(MF_E_SHUTDOWN))?
                    .resolve()?,
                state.stream_state.clone(),
            )
        };
        set_stream_state(
            &stream_state,
            windows::Win32::Media::MediaFoundation::MF_STREAM_STATE_STOPPED,
        )?;
        unsafe {
            queue.QueueEventParamVar(
                MESourceStopped.0 as u32,
                &GUID::zeroed(),
                HRESULT(0),
                std::ptr::null(),
            )?;
        }
        Ok(())
    }

    fn Pause(&self) -> Result<()> {
        Err(Error::from(MF_E_INVALID_STATE_TRANSITION))
    }

    fn Shutdown(&self) -> Result<()> {
        let (queue, stream_state, already_shutdown) = {
            let mut state = lock(&self.state)?;
            if state.shutdown {
                (None, state.stream_state.clone(), true)
            } else {
                state.shutdown = true;
                state.stream_presented = false;
                state.presentation = None;
                state.stream = None;
                (state.queue.take(), state.stream_state.clone(), false)
            }
        };
        if already_shutdown {
            return Ok(());
        }
        shutdown(&stream_state)?;
        if let Some(queue) = queue {
            let queue = queue.resolve()?;
            unsafe { queue.Shutdown()? };
        }
        Ok(())
    }
}

impl IMFMediaSourceEx_Impl for MediaSource_Impl {
    fn GetSourceAttributes(&self) -> Result<IMFAttributes> {
        ensure_source_alive(&self.state)?;
        self.attributes.resolve()
    }

    fn GetStreamAttributes(&self, stream_id: u32) -> Result<IMFAttributes> {
        if stream_id != 0 {
            return Err(Error::from(MF_E_INVALIDSTREAMNUMBER));
        }
        let state = lock(&self.state)?;
        if state.shutdown {
            return Err(Error::from(MF_E_SHUTDOWN));
        }
        descriptor(&state.stream_state)?.cast()
    }

    fn SetD3DManager(&self, _manager: Ref<'_, IUnknown>) -> Result<()> {
        ensure_source_alive(&self.state)
    }
}

impl IMFGetService_Impl for MediaSource_Impl {
    fn GetService(
        &self,
        service: *const GUID,
        riid: *const GUID,
        output: *mut *mut c_void,
    ) -> Result<()> {
        if service.is_null() || unsafe { service.read() } != MF_SAMPLEALLOCATOR_SERVICE {
            return Err(Error::from(MF_E_UNSUPPORTED_SERVICE));
        }
        let stream = lock(&self.state)?
            .stream
            .as_ref()
            .ok_or_else(|| Error::from(MF_E_SHUTDOWN))?
            .resolve()?;
        unsafe { query_interface(&stream, riid, output) }
    }
}

impl IAgileObject_Impl for MediaSource_Impl {}

impl IMFSampleAllocatorControl_Impl for MediaSource_Impl {
    fn SetDefaultAllocator(&self, stream_id: u32, allocator: Ref<'_, IUnknown>) -> Result<()> {
        let state = lock(&self.state)?;
        if state.shutdown {
            return Err(Error::from(MF_E_SHUTDOWN));
        }
        set_default_allocator(&state.stream_state, stream_id, allocator)
    }

    fn GetAllocatorUsage(
        &self,
        stream_id: u32,
        input_stream_id: *mut u32,
        usage: *mut MFSampleAllocatorUsage,
    ) -> Result<()> {
        let state = lock(&self.state)?;
        if state.shutdown {
            return Err(Error::from(MF_E_SHUTDOWN));
        }
        allocator_usage(&state.stream_state, stream_id, input_stream_id, usage)
    }
}

fn source_queue(state: &Mutex<SourceState>) -> Result<IMFMediaEventQueue> {
    let state = lock(state)?;
    if state.shutdown {
        return Err(Error::from(MF_E_SHUTDOWN));
    }
    state
        .queue
        .as_ref()
        .ok_or_else(|| Error::from(MF_E_SHUTDOWN))?
        .resolve()
}

fn ensure_source_alive(state: &Mutex<SourceState>) -> Result<()> {
    if lock(state)?.shutdown {
        Err(Error::from(MF_E_SHUTDOWN))
    } else {
        Ok(())
    }
}
