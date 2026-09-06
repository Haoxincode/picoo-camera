//! Bounded GPU completion ownership using the platform's event query.
use std::ffi::c_void;
use std::panic::{catch_unwind, AssertUnwindSafe};
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::{mpsc, Arc};

use windows::core::Interface;
use windows::Win32::Foundation::{CloseHandle, HANDLE};
use windows::Win32::Graphics::Direct3D11::{
    ID3D11Device4, ID3D11DeviceContext3, D3D11_CONTEXT_TYPE_ALL,
};
use windows::Win32::System::Threading::{
    CloseThreadpoolWait, CreateEventW, CreateThreadpoolWait, SetThreadpoolWait,
    PTP_CALLBACK_INSTANCE, PTP_WAIT,
};

use super::WindowsGpuContext;

const CONTEXT_LIMIT: usize = 3;
const PROCESS_LIMIT: usize = 12;
static PROCESS_PENDING: AtomicUsize = AtomicUsize::new(0);

#[derive(Debug, thiserror::Error)]
pub enum WindowsCompletionError {
    #[error("GPU completion capacity is exhausted")]
    Capacity,
    #[error("GPU command submission panicked")]
    SubmissionPanicked,
    #[error("GPU completion notification closed")]
    NotificationClosed,
    #[error("Windows GPU work failed: {0}")]
    Platform(#[from] windows::core::Error),
}

struct Permit(Arc<AtomicUsize>);
impl Permit {
    fn acquire(local: &Arc<AtomicUsize>) -> Result<Self, WindowsCompletionError> {
        let reserve = |count: &AtomicUsize, limit| {
            count.fetch_update(Ordering::AcqRel, Ordering::Acquire, |value| {
                (value < limit).then_some(value + 1)
            })
        };
        reserve(&PROCESS_PENDING, PROCESS_LIMIT).map_err(|_| WindowsCompletionError::Capacity)?;
        if reserve(local, CONTEXT_LIMIT).is_err() {
            PROCESS_PENDING.fetch_sub(1, Ordering::AcqRel);
            return Err(WindowsCompletionError::Capacity);
        }
        Ok(Self(Arc::clone(local)))
    }
}
impl Drop for Permit {
    fn drop(&mut self) {
        self.0.fetch_sub(1, Ordering::AcqRel);
        PROCESS_PENDING.fetch_sub(1, Ordering::AcqRel);
    }
}

struct Event(HANDLE);
impl Drop for Event {
    fn drop(&mut self) {
        unsafe {
            let _ = CloseHandle(self.0);
        }
    }
}

struct RemovalEvent {
    device: ID3D11Device4,
    cookie: u32,
}
impl Drop for RemovalEvent {
    fn drop(&mut self) {
        unsafe {
            self.device.UnregisterDeviceRemoved(self.cookie);
        }
    }
}

struct NativeWait(PTP_WAIT);
impl Drop for NativeWait {
    fn drop(&mut self) {
        // Never rearmed: either setup failed before arming or this is its sole
        // callback. Native cleanup waits asynchronously for this callback to exit.
        unsafe {
            CloseThreadpoolWait(self.0);
        }
    }
}

struct Delivery<T> {
    result: Result<T, WindowsCompletionError>,
    _permit: Permit,
}

/// Dropping the receiver never cancels the native resource retention.
pub struct WindowsGpuCompletion<T> {
    received: mpsc::Receiver<Delivery<T>>,
}
impl<T> WindowsGpuCompletion<T> {
    /// Only a dedicated codec/output worker may block waiting for GPU work.
    pub fn wait_on_worker(self) -> Result<T, WindowsCompletionError> {
        self.received
            .recv()
            .map_err(|_| WindowsCompletionError::NotificationClosed)?
            .result
    }

    /// UI and owner executors inspect readiness without waiting.
    pub fn try_take(&self) -> Result<Option<T>, WindowsCompletionError> {
        match self.received.try_recv() {
            Ok(delivery) => delivery.result.map(Some),
            Err(mpsc::TryRecvError::Empty) => Ok(None),
            Err(mpsc::TryRecvError::Disconnected) => {
                Err(WindowsCompletionError::NotificationClosed)
            }
        }
    }
}

struct Pending<T> {
    // Native registrations must be released before the event and context.
    wait: Option<NativeWait>,
    _removal: RemovalEvent,
    event: Event,
    gpu: Arc<WindowsGpuContext>,
    owners: Option<T>,
    submitted: Result<(), WindowsCompletionError>,
    send: mpsc::SyncSender<Delivery<T>>,
    permit: Option<Permit>,
}

impl WindowsGpuContext {
    /// REQ-PICOO-NEXT-016/025: reserve notifications before submitting any commands.
    /// The completion retains `owners` through success, failure, panic and cancellation.
    ///
    /// # Safety
    /// All resources used by new GPU work must remain owned by `owners`; the
    /// closure must not release or overwrite them, even on error. Existing work
    /// must already have its own completion owner. The closure must enqueue its
    /// commands before returning, not delegate pending submission to another thread.
    /// Do not publish images until this completion succeeds. Platform MFT calls
    /// run outside an external immediate-context lock; direct command groups use
    /// `with_immediate_context` and retain the same owners until completion.
    pub unsafe fn submit_owned<T: Send + 'static>(
        self: &Arc<Self>,
        owners: T,
        submit: impl FnOnce(&Self, &mut T) -> Result<(), WindowsCompletionError>,
    ) -> Result<WindowsGpuCompletion<T>, WindowsCompletionError> {
        let context: ID3D11DeviceContext3 = self.immediate.cast()?;
        let (mut pending, completion) = Pending::prepare(Arc::clone(self), owners)?;
        pending.submitted = catch_unwind(AssertUnwindSafe(|| {
            submit(self, pending.owners.as_mut().expect("prepared owner"))
        }))
        .unwrap_or(Err(WindowsCompletionError::SubmissionPanicked));
        // Even a failed or panicking submission may have queued commands. Flush
        // their completion query before allowing the one-shot callback to run.
        self.with_immediate_context(|_| {
            context.Flush1(D3D11_CONTEXT_TYPE_ALL, Some(pending.event.0));
        });
        Pending::arm(pending);
        Ok(completion)
    }
}

impl<T: Send + 'static> Pending<T> {
    unsafe fn prepare(
        gpu: Arc<WindowsGpuContext>,
        owners: T,
    ) -> Result<(Box<Self>, WindowsGpuCompletion<T>), WindowsCompletionError> {
        let permit = Permit::acquire(&gpu.completion_slots)?;
        let device: ID3D11Device4 = gpu.device.cast()?;
        let event = Event(CreateEventW(None, true, false, None)?);
        let cookie = device.RegisterDeviceRemovedEvent(event.0)?;
        let (send, received) = mpsc::sync_channel(1);
        let mut pending = Box::new(Self {
            wait: None,
            _removal: RemovalEvent { device, cookie },
            event,
            gpu,
            owners: Some(owners),
            submitted: Ok(()),
            send,
            permit: Some(permit),
        });
        let wait = CreateThreadpoolWait(
            Some(completed::<T>),
            Some((&mut *pending as *mut Self).cast()),
            None,
        )?;
        pending.wait = Some(NativeWait(wait));
        Ok((pending, WindowsGpuCompletion { received }))
    }

    unsafe fn arm(pending: Box<Self>) {
        let wait = pending.wait.as_ref().expect("prepared wait").0;
        let event = pending.event.0;
        // The sole callback now owns the box, including the original sample
        // leases. Never dereference it after arming: it may complete immediately.
        let _ = Box::into_raw(pending);
        SetThreadpoolWait(wait, Some(event), None);
    }
}

unsafe extern "system" fn completed<T: Send + 'static>(
    _: PTP_CALLBACK_INSTANCE,
    context: *mut c_void,
    _: PTP_WAIT,
    _: u32,
) {
    // No Rust panic (including a user owner's destructor) crosses the system ABI.
    let _ = catch_unwind(AssertUnwindSafe(|| {
        let mut pending = Box::from_raw(context.cast::<Pending<T>>());
        let result = pending
            .gpu
            .device
            .GetDeviceRemovedReason()
            .map_err(WindowsCompletionError::from)
            .and(std::mem::replace(&mut pending.submitted, Ok(())))
            .map(|()| pending.owners.take().expect("completion owner"));
        let send = pending.send.clone();
        let delivery = Delivery {
            result,
            _permit: pending.permit.take().expect("completion permit"),
        };
        drop(pending);
        let _ = send.send(delivery);
    }));
}

#[cfg(test)]
mod tests;
