//! Windows COM server boundary for `PicooVirtualCameraSource.dll`.

mod activator;
mod class_factory;
mod media_source;
mod media_stream;

use std::ffi::c_void;
use std::panic::{catch_unwind, AssertUnwindSafe};
use std::sync::atomic::{AtomicU32, Ordering};
use std::sync::{Mutex, MutexGuard};

use windows::core::{Error, Interface, Result, GUID, HRESULT};
use windows::Win32::Foundation::{E_POINTER, S_FALSE, S_OK};
use windows::Win32::System::Diagnostics::Debug::OutputDebugStringW;

use self::class_factory::ClassFactory;

pub const PICOO_VCAM_CLSID: GUID = GUID::from_u128(0xa7c4e2f1_8b3d_4c6a_9e5f_1d2c3b4a5e6f);

static ACTIVE_OBJECTS: AtomicU32 = AtomicU32::new(0);
static SERVER_LOCKS: AtomicU32 = AtomicU32::new(0);

// Keep the product name as an explicit UTF-16 PE payload for installer/bundle validation.
#[used]
static FRIENDLY_NAME_UTF16: [u16; 13] = [
    b'P' as u16,
    b'i' as u16,
    b'c' as u16,
    b'o' as u16,
    b'o' as u16,
    b' ' as u16,
    b'C' as u16,
    b'a' as u16,
    b'm' as u16,
    b'e' as u16,
    b'r' as u16,
    b'a' as u16,
    0,
];

pub(super) struct ObjectTracker;

impl ObjectTracker {
    pub fn new() -> Self {
        ACTIVE_OBJECTS.fetch_add(1, Ordering::AcqRel);
        Self
    }
}

impl Drop for ObjectTracker {
    fn drop(&mut self) {
        ACTIVE_OBJECTS.fetch_sub(1, Ordering::AcqRel);
    }
}

pub(super) fn set_server_lock(locked: bool) {
    if locked {
        SERVER_LOCKS.fetch_add(1, Ordering::AcqRel);
    } else {
        let _ = SERVER_LOCKS.fetch_update(Ordering::AcqRel, Ordering::Acquire, |count| {
            count.checked_sub(1)
        });
    }
}

pub(super) fn lock<T>(mutex: &Mutex<T>) -> Result<MutexGuard<'_, T>> {
    mutex
        .lock()
        .map_err(|_| Error::from(windows::Win32::Foundation::E_UNEXPECTED))
}

pub(super) fn emit_debug_message(message: &str) {
    let wide = message
        .encode_utf16()
        .chain(std::iter::once(0))
        .collect::<Vec<_>>();
    unsafe {
        OutputDebugStringW(windows::core::PCWSTR(wide.as_ptr()));
    }
}

pub(super) unsafe fn query_interface<T: Interface>(
    interface: &T,
    riid: *const GUID,
    output: *mut *mut c_void,
) -> Result<()> {
    if riid.is_null() || output.is_null() {
        return Err(Error::from(E_POINTER));
    }
    output.write(std::ptr::null_mut());
    interface.query(riid, output).ok()
}

fn guarded_hresult(operation: impl FnOnce() -> Result<()>) -> HRESULT {
    match catch_unwind(AssertUnwindSafe(operation)) {
        Ok(Ok(())) => S_OK,
        Ok(Err(error)) => error.code(),
        Err(_) => windows::Win32::Foundation::E_UNEXPECTED,
    }
}

/// Standard in-process COM server entry point.
///
/// # Safety
///
/// COM must pass valid readable `clsid`/`riid` pointers and a writable `output` pointer.
/// The function validates null pointers before dereferencing them.
#[no_mangle]
pub unsafe extern "system" fn DllGetClassObject(
    clsid: *const GUID,
    riid: *const GUID,
    output: *mut *mut c_void,
) -> HRESULT {
    guarded_hresult(|| {
        if clsid.is_null() || riid.is_null() || output.is_null() {
            return Err(Error::from(E_POINTER));
        }
        output.write(std::ptr::null_mut());
        if clsid.read() != PICOO_VCAM_CLSID {
            return Err(Error::from(
                windows::Win32::Foundation::CLASS_E_CLASSNOTAVAILABLE,
            ));
        }
        let factory = ClassFactory::create();
        query_interface(&factory, riid, output)
    })
}

/// COM may unload the DLL only after all objects and explicit server locks are gone.
#[no_mangle]
pub extern "system" fn DllCanUnloadNow() -> HRESULT {
    if ACTIVE_OBJECTS.load(Ordering::Acquire) == 0 && SERVER_LOCKS.load(Ordering::Acquire) == 0 {
        S_OK
    } else {
        S_FALSE
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use windows::core::{IUnknown, Interface, BOOL};
    use windows::Win32::Media::KernelStreaming::IKsControl;
    use windows::Win32::Media::MediaFoundation::{
        IMFActivate, IMFGetService, IMFMediaSource, IMFMediaSourceEx, IMFMediaStream2, IMFSample,
        IMFSampleAllocatorControl, IMFVideoSampleAllocator, MEMediaSample, MEStreamStarted,
        MEStreamStopped, MFCreateVideoSampleAllocatorEx, MFShutdown, MFStartup, MF_EVENT_FLAG_NONE,
        MF_VERSION,
    };
    use windows::Win32::System::Com::StructuredStorage::PROPVARIANT;
    use windows::Win32::System::Com::{
        CoInitializeEx, CoUninitialize, IAgileObject, IClassFactory, COINIT_MULTITHREADED,
    };

    const MF_SAMPLEALLOCATOR_SERVICE: GUID =
        GUID::from_u128(0xbbcd045d_4d8b_49e6_9d72_6c60c22a445b);

    unsafe fn expect_stream_event(stream: &IMFMediaStream2, expected: u32) {
        let event = stream
            .GetEvent(MF_EVENT_FLAG_NONE)
            .expect("IMFMediaStream::GetEvent");
        assert_eq!(event.GetType().expect("IMFMediaEvent::GetType"), expected);
        assert_eq!(event.GetStatus().expect("IMFMediaEvent::GetStatus"), S_OK);
    }

    unsafe fn expect_sample_delivery(stream: &IMFMediaStream2) -> i64 {
        let event = stream
            .GetEvent(MF_EVENT_FLAG_NONE)
            .expect("IMFMediaStream::GetEvent(MEMediaSample)");
        assert_eq!(
            event.GetType().expect("IMFMediaEvent::GetType"),
            MEMediaSample.0 as u32
        );
        assert_eq!(event.GetStatus().expect("IMFMediaEvent::GetStatus"), S_OK);
        let value = event.GetValue().expect("IMFMediaEvent::GetValue");
        let unknown = IUnknown::try_from(&value).expect("MEMediaSample event value");
        let sample: IMFSample = unknown.cast().expect("MEMediaSample IMFSample");
        assert_eq!(
            sample
                .GetSampleDuration()
                .expect("IMFSample::GetSampleDuration"),
            crate::format::SAMPLE_DURATION_100NS
        );
        assert_eq!(
            sample.GetTotalLength().expect("IMFSample::GetTotalLength"),
            crate::format::nv12_len(1280, 720).expect("720p NV12 length") as u32
        );
        sample.GetSampleTime().expect("IMFSample::GetSampleTime")
    }

    struct ComApartment;

    impl ComApartment {
        fn start(label: &str) -> Self {
            unsafe {
                CoInitializeEx(None, COINIT_MULTITHREADED)
                    .ok()
                    .unwrap_or_else(|error| panic!("{label} CoInitializeEx(MTA): {error}"));
            }
            Self
        }
    }

    impl Drop for ComApartment {
        fn drop(&mut self) {
            unsafe { CoUninitialize() };
        }
    }

    struct MfPlatform {
        _com: ComApartment,
    }

    impl MfPlatform {
        fn start() -> Self {
            let com = ComApartment::start("test");
            unsafe {
                if let Err(error) = MFStartup(MF_VERSION, Default::default()) {
                    panic!("MFStartup failed: {error}");
                }
            }
            Self { _com: com }
        }
    }

    impl Drop for MfPlatform {
        fn drop(&mut self) {
            unsafe {
                let _ = MFShutdown();
            }
        }
    }

    #[test]
    fn in_process_activation_exposes_and_runs_the_frame_server_source_contract() {
        let _platform = MfPlatform::start();
        unsafe {
            let mut raw_factory = std::ptr::null_mut();
            DllGetClassObject(&PICOO_VCAM_CLSID, &IClassFactory::IID, &mut raw_factory)
                .ok()
                .expect("DllGetClassObject(IClassFactory)");
            let factory = IClassFactory::from_raw(raw_factory);
            let activator: IMFActivate = factory
                .CreateInstance(None::<&IUnknown>)
                .expect("IClassFactory::CreateInstance(IMFActivate)");
            activator
                .cast::<IAgileObject>()
                .expect("activator must expose IAgileObject");
            let source_ex: IMFMediaSourceEx = activator
                .ActivateObject()
                .expect("IMFActivate::ActivateObject(IMFMediaSourceEx)");
            source_ex
                .cast::<IAgileObject>()
                .expect("media source must expose IAgileObject");
            source_ex
                .cast::<IKsControl>()
                .expect("Frame Server media source must expose IKsControl");
            let services = source_ex
                .cast::<IMFGetService>()
                .expect("Frame Server media source must expose IMFGetService");
            let stream: IMFMediaStream2 = services
                .GetService(&MF_SAMPLEALLOCATOR_SERVICE)
                .expect("media source must expose its IMFMediaStream2 service");
            stream
                .cast::<IAgileObject>()
                .expect("media stream must expose IAgileObject");
            let allocator_control: IMFSampleAllocatorControl = stream
                .cast()
                .expect("media stream must expose allocator control");
            for _ in 0..2 {
                let mut raw_allocator = std::ptr::null_mut();
                MFCreateVideoSampleAllocatorEx(&IMFVideoSampleAllocator::IID, &mut raw_allocator)
                    .expect("MFCreateVideoSampleAllocatorEx");
                let allocator = IMFVideoSampleAllocator::from_raw(raw_allocator);
                allocator_control
                    .SetDefaultAllocator(0, &allocator)
                    .expect("replace stopped-stream allocator");
            }

            let cross_thread_source = source_ex.clone().into_raw() as usize;
            let cross_thread_stream = stream.clone().into_raw() as usize;
            std::thread::spawn(move || {
                let _com = ComApartment::start("worker");
                let source = IMFMediaSourceEx::from_raw(cross_thread_source as *mut _);
                let stream = IMFMediaStream2::from_raw(cross_thread_stream as *mut _);
                source
                    .GetSourceAttributes()
                    .expect("agile source must be callable from another MTA thread");
                stream
                    .GetStreamDescriptor()
                    .expect("agile stream must be callable from another MTA thread");
                drop(source);
                drop(stream);
            })
            .join()
            .expect("cross-thread agile-source check");

            let source: IMFMediaSource = source_ex.cast().expect("IMFMediaSource");
            let presentation = source
                .CreatePresentationDescriptor()
                .expect("CreatePresentationDescriptor");
            let independent_presentation = source
                .CreatePresentationDescriptor()
                .expect("CreatePresentationDescriptor clone");
            presentation.SelectStream(0).expect("SelectStream(0)");
            let mut independent_selected = BOOL(1);
            let mut independent_stream = None;
            independent_presentation
                .GetStreamDescriptorByIndex(0, &mut independent_selected, &mut independent_stream)
                .expect("inspect independent presentation descriptor");
            assert!(
                !independent_selected.as_bool(),
                "presentation descriptor selection must be caller-local"
            );
            let start_position = PROPVARIANT::default();
            source
                .Start(&presentation, &GUID::zeroed(), &start_position)
                .expect("IMFMediaSource::Start");
            expect_stream_event(&stream, MEStreamStarted.0 as u32);
            stream
                .RequestSample(None::<&IUnknown>)
                .expect("IMFMediaStream::RequestSample");
            let first_sample_time = expect_sample_delivery(&stream);
            source.Stop().expect("IMFMediaSource::Stop");
            expect_stream_event(&stream, MEStreamStopped.0 as u32);
            source
                .Start(&presentation, &GUID::zeroed(), &start_position)
                .expect("IMFMediaSource::Start after Stop");
            expect_stream_event(&stream, MEStreamStarted.0 as u32);
            stream
                .RequestSample(None::<&IUnknown>)
                .expect("IMFMediaStream::RequestSample after restart");
            let restarted_sample_time = expect_sample_delivery(&stream);
            assert!(
                restarted_sample_time >= first_sample_time,
                "sample clock must remain monotonic across Stop/Start"
            );
            source.Stop().expect("second IMFMediaSource::Stop");
            expect_stream_event(&stream, MEStreamStopped.0 as u32);
            source.Shutdown().expect("IMFMediaSource::Shutdown");
        }
    }
}
