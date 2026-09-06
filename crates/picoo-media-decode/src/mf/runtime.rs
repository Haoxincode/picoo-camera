//! REQ-PICOO-NEXT-011: COM belongs to the codec thread; MF outlives retained samples.
use std::marker::PhantomData;
use std::rc::Rc;
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::{mpsc, Arc};
use std::thread::JoinHandle;

use windows::core::HRESULT;
use windows::Win32::Foundation::RPC_E_CHANGED_MODE;
use windows::Win32::Media::MediaFoundation::{MFShutdown, MFStartup, MF_VERSION};
use windows::Win32::System::Com::{CoInitializeEx, CoUninitialize, COINIT_MULTITHREADED};

use crate::DecodeError;

const MAX_RUNTIME_COHORTS: usize = 16;
static RUNTIME_COHORTS: AtomicUsize = AtomicUsize::new(0);

struct RuntimePermit;

impl RuntimePermit {
    fn reserve() -> Result<Self, DecodeError> {
        RUNTIME_COHORTS
            .fetch_update(Ordering::AcqRel, Ordering::Acquire, |count| {
                (count < MAX_RUNTIME_COHORTS).then_some(count + 1)
            })
            .map(|_| Self)
            .map_err(|_| DecodeError::Platform("MF runtime cohort limit reached".into()))
    }
}

impl Drop for RuntimePermit {
    fn drop(&mut self) {
        RUNTIME_COHORTS.fetch_sub(1, Ordering::AcqRel);
    }
}

struct ComApartment {
    owned: bool,
    _thread_affinity: PhantomData<Rc<()>>,
}

impl ComApartment {
    fn start() -> Result<Self, DecodeError> {
        Ok(Self {
            owned: com_initialization_ownership(unsafe {
                CoInitializeEx(None, COINIT_MULTITHREADED)
            })?,
            _thread_affinity: PhantomData,
        })
    }
}

impl Drop for ComApartment {
    fn drop(&mut self) {
        if self.owned {
            unsafe { CoUninitialize() };
        }
    }
}

pub(super) struct MfRuntimeGuard {
    // Only closes a channel on final release, including on an MF callback thread.
    pub(super) _lifetime: Arc<mpsc::Sender<()>>,
    _apartment: ComApartment,
}

impl MfRuntimeGuard {
    /// One bounded diagnostic runtime; each caller still owns its COM apartment.
    #[cfg(any(test, feature = "test-codecs"))]
    pub(super) fn diagnostic() -> Result<Self, DecodeError> {
        static LIFETIME: std::sync::Mutex<Option<Arc<mpsc::Sender<()>>>> =
            std::sync::Mutex::new(None);
        let apartment = ComApartment::start()?;
        let mut shared = LIFETIME.lock().unwrap();
        if shared.is_none() {
            *shared = Some(start_lifetime()?.0);
        }
        Ok(Self {
            _lifetime: shared.as_ref().unwrap().clone(),
            _apartment: apartment,
        })
    }

    #[cfg(any(test, feature = "windows-mf"))]
    pub(super) fn start() -> Result<Self, DecodeError> {
        let apartment = ComApartment::start()?;
        let (lifetime, _worker) = start_lifetime()?;
        Ok(Self {
            _lifetime: lifetime,
            _apartment: apartment,
        })
    }
}

type RuntimeWorker = JoinHandle<Result<(), DecodeError>>;

fn start_lifetime() -> Result<(Arc<mpsc::Sender<()>>, RuntimeWorker), DecodeError> {
    let permit = RuntimePermit::reserve()?;
    let (release, released) = mpsc::channel();
    let (ready, started) = mpsc::sync_channel(1);
    let worker = std::thread::Builder::new()
        .name("picoo-mf-runtime".into())
        .spawn(move || {
            // Permit remains held even if shutdown hangs. No unbounded replacement threads.
            let _permit = permit;
            let startup = unsafe { MFStartup(MF_VERSION, Default::default()) }
                .map_err(|error| DecodeError::Platform(format!("MFStartup: {error}")));
            match startup {
                Ok(()) => {
                    let _ = ready.send(Ok(()));
                    // The sender is private to lifetime owners and never carries messages.
                    // Wait for every owner, even if an accidental message is sent.
                    while released.recv().is_ok() {}
                    unsafe { MFShutdown() }.map_err(|error| {
                        tracing::error!(%error, "MF runtime shutdown failed");
                        DecodeError::Platform(format!("MFShutdown: {error}"))
                    })
                }
                Err(error) => {
                    let _ = ready.send(Err(error));
                    Ok(())
                }
            }
        })
        .map_err(|error| DecodeError::Platform(format!("MF runtime thread: {error}")))?;
    started
        .recv()
        .map_err(|_| DecodeError::Platform("MF runtime startup disconnected".into()))??;
    Ok((Arc::new(release), worker))
}

pub(super) fn com_initialization_ownership(result: HRESULT) -> Result<bool, DecodeError> {
    if result.is_ok() {
        // S_OK and S_FALSE both require matching CoUninitialize on this thread.
        Ok(true)
    } else if result == RPC_E_CHANGED_MODE {
        Ok(false)
    } else {
        Err(DecodeError::Platform(format!(
            "CoInitializeEx: {}",
            result.message()
        )))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use windows::Win32::Media::MediaFoundation::MFCreateSample;

    #[test]
    fn retained_owner_keeps_runtime_alive_after_codec_apartment_exits() {
        let (lifetime, worker) = start_lifetime().unwrap();
        let retained = Arc::clone(&lifetime);
        std::thread::spawn(move || {
            let _codec = MfRuntimeGuard {
                _lifetime: lifetime,
                _apartment: ComApartment::start().unwrap(),
            };
            unsafe { MFCreateSample() }.unwrap();
        })
        .join()
        .unwrap();
        assert!(!worker.is_finished());
        std::thread::spawn(move || {
            let _consumer = ComApartment::start().unwrap();
            let sample = unsafe { MFCreateSample() }.unwrap();
            unsafe { sample.SetSampleTime(123).unwrap() };
            assert_eq!(unsafe { sample.GetSampleTime() }.unwrap(), 123);
            drop(sample);
            drop(retained);
        })
        .join()
        .unwrap();
        worker.join().unwrap().unwrap();
    }
}
