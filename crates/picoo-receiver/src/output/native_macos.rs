//! Latest-only macOS VCam worker using the legal CMIO sink queue.

use std::sync::{Arc, Condvar, Mutex};
use std::thread::{self, JoinHandle};
use std::time::Duration;

use picoo_frame_hub::{NativeVideoFrame, PlaceholderMode, SharedRingSubmitOutcome};

use super::apple::{placeholder_frame, prepare, PrepareError, PreparedImage, Resources};
use super::backend::{BackendCapabilities, BackendFailureReason, BackendState, OutputBackend};
use super::macos_cmio::{CmioSinkError, MacCmioSink, SinkLayout};
use super::OutputEvent;

#[derive(Clone)]
enum Request {
    Frame(Arc<NativeVideoFrame>),
    Placeholder(PlaceholderMode, bool),
}

impl Request {
    fn kind(&self) -> ContentKind {
        match self {
            Self::Frame(_) => ContentKind::Live,
            Self::Placeholder(..) => ContentKind::Placeholder,
        }
    }
}

#[derive(Clone, Copy, PartialEq, Eq)]
enum ContentKind {
    Live,
    Placeholder,
}

#[derive(Clone)]
struct PendingRequest {
    request: Request,
    discontinuity: bool,
}

struct State {
    pending: Option<PendingRequest>,
    revision: u64,
    kind: ContentKind,
    stopped: bool,
}

impl Default for State {
    fn default() -> Self {
        Self {
            pending: None,
            revision: 0,
            kind: ContentKind::Placeholder,
            stopped: false,
        }
    }
}

impl State {
    fn replace(&mut self, request: Request) -> Result<bool, ()> {
        let revision = self.revision.checked_add(1).ok_or(())?;
        let kind = request.kind();
        // Preserve a privacy boundary while replacing an unsubmitted request:
        // a newer placeholder must not erase the live→placeholder reset that
        // the worker still has to perform before its first enqueue.
        let discontinuity = kind != self.kind
            || self
                .pending
                .as_ref()
                .is_some_and(|pending| pending.discontinuity);
        self.kind = kind;
        self.revision = revision;
        Ok(self
            .pending
            .replace(PendingRequest {
                request,
                discontinuity,
            })
            .is_some())
    }
}

#[derive(Clone, Copy, PartialEq, Eq)]
struct PreparedKey {
    revision: u64,
    layout: SinkLayout,
    backend: OutputBackend,
}

pub(crate) struct NativeOutput {
    shared: Arc<(Mutex<State>, Condvar)>,
    events: Arc<Mutex<Option<(u64, OutputEvent)>>>,
    worker: Option<JoinHandle<()>>,
}

impl NativeOutput {
    pub(crate) fn start() -> Result<Self, String> {
        let backend = select_backend()?;
        let shared = Arc::new((Mutex::new(State::default()), Condvar::new()));
        let events = Arc::new(Mutex::new(None));
        let worker_shared = Arc::clone(&shared);
        let worker_events = Arc::clone(&events);
        let selected = backend.selection().backend;
        let worker = thread::Builder::new()
            .name("picoo-macos-vcam".into())
            .spawn(move || run_worker(worker_shared, worker_events, selected))
            .map_err(|error| error.to_string())?;
        Ok(Self {
            shared,
            events,
            worker: Some(worker),
        })
    }

    pub(crate) fn submit(&self, frame: Arc<NativeVideoFrame>) -> SharedRingSubmitOutcome {
        self.enqueue(Request::Frame(frame))
    }

    pub(crate) fn placeholder(&self, mode: PlaceholderMode, reconnecting: bool) {
        let _ = self.enqueue(Request::Placeholder(mode, reconnecting));
    }

    fn enqueue(&self, request: Request) -> SharedRingSubmitOutcome {
        let mut state = self.shared.0.lock().unwrap();
        if state.stopped {
            return SharedRingSubmitOutcome::Stopped;
        }
        let replaced = match state.replace(request) {
            Ok(replaced) => replaced,
            Err(()) => {
                state.stopped = true;
                state.pending = None;
                self.shared.1.notify_one();
                return SharedRingSubmitOutcome::Stopped;
            }
        };
        self.shared.1.notify_one();
        if replaced {
            SharedRingSubmitOutcome::ReplacedPending
        } else {
            SharedRingSubmitOutcome::Queued
        }
    }

    pub(crate) fn poll_event(&self) -> Option<OutputEvent> {
        let (revision, event) = self.events.lock().unwrap().take()?;
        (revision == self.shared.0.lock().unwrap().revision).then_some(event)
    }
}

impl Drop for NativeOutput {
    fn drop(&mut self) {
        {
            let mut state = self.shared.0.lock().unwrap();
            state.stopped = true;
            state.pending = None;
            self.shared.1.notify_one();
        }
        if let Some(worker) = self.worker.take() {
            let _ = worker.join();
        }
    }
}

fn select_backend() -> Result<BackendState, String> {
    if std::env::var_os("PICOO_MACOS_VCAM_FORCE_CPU_BRIDGE").is_some() {
        BackendState::auto_with_native_failure(
            BackendCapabilities::cpu_bridge_only(),
            Some(BackendFailureReason::NativeImportUnsupported),
        )
        .map_err(|error| format!("macOS CpuBridge selection: {error:?}"))
    } else {
        BackendState::auto(BackendCapabilities {
            gpu_native: true,
            cpu_bridge: true,
        })
        .map_err(|error| format!("macOS GpuNative selection: {error:?}"))
    }
}

fn run_worker(
    shared: Arc<(Mutex<State>, Condvar)>,
    events: Arc<Mutex<Option<(u64, OutputEvent)>>>,
    backend: OutputBackend,
) {
    let mut sink = None;
    let mut resources: Option<Resources> = None;
    let mut prepared_cache: Option<(PreparedKey, PreparedImage)> = None;
    let mut privacy_reset_revision = None;
    loop {
        let (revision, pending) = {
            let mut state = shared.0.lock().unwrap();
            while !state.stopped && state.pending.is_none() {
                state = shared.1.wait(state).unwrap();
            }
            if state.stopped {
                return;
            }
            (
                state.revision,
                state.pending.clone().expect("pending request"),
            )
        };

        // A live sample may still be retained by the CMIO queue or by the
        // extension's source cache. Reset the transport before the first
        // waiting/reconnecting placeholder of this revision, so a new source
        // client can never observe that pre-privacy image after the gate.
        if pending.discontinuity
            && pending.request.kind() == ContentKind::Placeholder
            && privacy_reset_revision != Some(revision)
        {
            if !shutdown_sink(&mut sink) {
                publish_failure(
                    &events,
                    revision,
                    "CMIO sink teardown failed; output quarantined".into(),
                );
                return;
            }
            resources = None;
            prepared_cache = None;
            privacy_reset_revision = Some(revision);
        } else if pending.request.kind() == ContentKind::Live {
            privacy_reset_revision = None;
        }

        if sink.is_none() {
            match MacCmioSink::connect() {
                Ok(connected) => sink = Some(connected),
                Err(error) => {
                    publish_failure(&events, revision, error.to_string());
                    wait_for_change(&shared, revision, Duration::from_secs(1));
                    continue;
                }
            }
        }
        let layout = match sink.as_ref().expect("connected sink").layout() {
            Ok(layout) => layout,
            Err(error) => {
                publish_failure(&events, revision, error.to_string());
                if !shutdown_sink(&mut sink) {
                    publish_failure(
                        &events,
                        revision,
                        "CMIO sink teardown failed; output quarantined".into(),
                    );
                    return;
                }
                resources = None;
                prepared_cache = None;
                wait_for_change(&shared, revision, Duration::from_millis(250));
                continue;
            }
        };
        let key = PreparedKey {
            revision,
            layout,
            backend,
        };
        if !sink.as_ref().expect("connected sink").has_capacity() {
            wait_for_change(&shared, revision, Duration::from_millis(16));
            continue;
        }
        if prepared_cache
            .as_ref()
            .is_none_or(|(cached, _)| *cached != key)
        {
            let prepared = match &pending.request {
                Request::Frame(frame) => prepare(&mut resources, frame, layout, backend),
                Request::Placeholder(mode, reconnecting) => placeholder_frame(*mode, *reconnecting)
                    .map_err(PrepareError::Failed)
                    .and_then(|frame| prepare(&mut resources, &frame, layout, backend)),
            };
            match prepared {
                Ok(prepared) => prepared_cache = Some((key, prepared)),
                Err(PrepareError::Backpressure) => {
                    wait_for_change(&shared, revision, Duration::from_millis(16));
                    continue;
                }
                Err(PrepareError::Failed(error)) => {
                    publish_failure(&events, revision, error);
                    wait_for_change(&shared, revision, Duration::from_millis(16));
                    continue;
                }
            }
        }
        let prepared = &prepared_cache.as_ref().expect("prepared image").1;
        let mut state = shared.0.lock().unwrap();
        if state.stopped {
            return;
        }
        if state.revision != revision {
            continue;
        }
        let result = unsafe {
            sink.as_mut()
                .expect("connected sink")
                .submit(prepared.pixel_buffer(), pending.discontinuity)
        };
        match result {
            Ok(()) => {
                state.pending = None;
                drop(state);
                *events.lock().unwrap() = Some((revision, OutputEvent::Published));
                prepared_cache = None;
            }
            Err(CmioSinkError::QueueFull) => {
                let (_state, _timeout) = shared
                    .1
                    .wait_timeout(state, Duration::from_millis(16))
                    .unwrap();
            }
            Err(error) => {
                drop(state);
                publish_failure(&events, revision, error.to_string());
                if !shutdown_sink(&mut sink) {
                    publish_failure(
                        &events,
                        revision,
                        "CMIO sink teardown failed; output quarantined".into(),
                    );
                    return;
                }
                resources = None;
                prepared_cache = None;
                wait_for_change(&shared, revision, Duration::from_millis(250));
            }
        }
    }
}

fn shutdown_sink(sink: &mut Option<MacCmioSink>) -> bool {
    sink.take().is_none_or(MacCmioSink::shutdown)
}

fn publish_failure(events: &Mutex<Option<(u64, OutputEvent)>>, revision: u64, error: String) {
    *events.lock().unwrap() = Some((revision, OutputEvent::Failed(error)));
}

fn wait_for_change(shared: &(Mutex<State>, Condvar), revision: u64, timeout: Duration) {
    let state = shared.0.lock().unwrap();
    if !state.stopped && state.revision == revision {
        let (_state, _timeout) = shared.1.wait_timeout(state, timeout).unwrap();
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn native_is_default_and_cpu_bridge_is_explicit() {
        let native = BackendState::auto(BackendCapabilities {
            gpu_native: true,
            cpu_bridge: true,
        })
        .unwrap();
        assert_eq!(native.selection().backend, OutputBackend::GpuNative);
        let cpu = BackendState::auto_with_native_failure(
            BackendCapabilities::cpu_bridge_only(),
            Some(BackendFailureReason::NativeImportUnsupported),
        )
        .unwrap();
        assert_eq!(cpu.selection().backend, OutputBackend::CpuBridge);
    }

    #[test]
    fn latest_placeholder_replaces_pending_and_marks_live_transition() {
        let mut state = State {
            kind: ContentKind::Live,
            ..State::default()
        };
        assert!(!state
            .replace(Request::Placeholder(PlaceholderMode::Black, true))
            .unwrap());
        assert_eq!(state.revision, 1);
        let pending = state.pending.as_ref().unwrap();
        assert!(matches!(pending.request, Request::Placeholder(..)));
        assert!(pending.discontinuity);
        assert!(state
            .replace(Request::Placeholder(PlaceholderMode::Logo, false))
            .unwrap());
        assert_eq!(state.revision, 2);
        assert!(state.pending.as_ref().unwrap().discontinuity);
    }
}
