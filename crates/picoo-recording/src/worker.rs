//! Dedicated native recording ownership — REQ-PICOO-MEDIA-071.
use crate::{
    bundle::RecordingState,
    encoded::EncodedWriter,
    ingress::{self, IngressPoll, RecordingInbox, RecordingInput},
    reorder::RecordingReorder,
    RecordingError,
};
use std::{
    path::PathBuf,
    sync::{
        atomic::{AtomicBool, AtomicU8, Ordering},
        Arc, OnceLock,
    },
    time::{Duration, Instant},
};

static ACTIVE: AtomicBool = AtomicBool::new(false);
const ARMING_TIMEOUT: Duration = Duration::from_secs(5);

#[derive(Debug, Clone)]
pub struct RecordingResult {
    pub path: Option<PathBuf>,
    pub state: RecordingState,
    pub error: Option<String>,
}

struct Shared {
    path: OnceLock<PathBuf>,
    state: AtomicU8,
    refresh: AtomicBool,
    result: OnceLock<RecordingResult>,
}

/// Commands only. Drop requests a normal drain; it never joins a native thread.
/// The process-wide slot remains occupied until the worker actually exits.
pub struct RecordingWorker {
    ingress: Option<ingress::RecordingIngress>,
    shared: Arc<Shared>,
}

struct WorkerSlot;
impl Drop for WorkerSlot {
    fn drop(&mut self) {
        ACTIVE.store(false, Ordering::Release);
    }
}

impl RecordingWorker {
    pub fn start(parent: PathBuf) -> Result<Self, RecordingError> {
        ACTIVE
            .compare_exchange(false, true, Ordering::AcqRel, Ordering::Acquire)
            .map_err(|_| RecordingError::InvalidInput("recording worker already active"))?;
        let slot = WorkerSlot;
        let (sender, inbox) = ingress::channel();
        let shared = Arc::new(Shared {
            path: OnceLock::new(),
            state: AtomicU8::new(0),
            refresh: AtomicBool::new(false),
            result: OnceLock::new(),
        });
        let worker_shared = Arc::clone(&shared);
        std::thread::Builder::new()
            .name("picoo-recording".into())
            .spawn(move || {
                let _slot = slot;
                let result = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
                    run(parent, inbox, &worker_shared)
                }));
                let outcome = match result {
                    Ok(result) => result,
                    Err(_) => RecordingResult {
                        path: worker_shared.path.get().cloned(),
                        state: RecordingState::Failed,
                        error: Some("recording worker panicked".into()),
                    },
                };
                worker_shared
                    .state
                    .store(state_code(outcome.state), Ordering::Release);
                worker_shared.refresh.store(false, Ordering::Release);
                let _ = worker_shared.result.set(outcome);
            })?;
        Ok(Self {
            ingress: Some(sender),
            shared,
        })
    }

    pub fn offer(&mut self, input: RecordingInput) -> Result<(), ingress::IngressFailure> {
        self.ingress
            .as_ref()
            .ok_or(ingress::IngressFailure::WorkerStopped)?
            .offer(input)
    }

    pub fn report_gap(
        &mut self,
        reason: crate::bundle::GapReason,
        source: Option<crate::bundle::SourceRange>,
    ) -> Result<(), ingress::IngressFailure> {
        self.ingress
            .as_ref()
            .ok_or(ingress::IngressFailure::WorkerStopped)?
            .report_gap(reason, source)
    }

    pub fn is_accepting(&self) -> bool {
        self.ingress.is_some() && self.shared.result.get().is_none()
    }

    pub fn terminate(&mut self, failure: ingress::IngressFailure) {
        if let Some(ingress) = &self.ingress {
            ingress.terminate(failure);
        }
    }

    pub fn stop(&mut self) {
        self.ingress = None;
    }

    pub fn take_refresh_request(&self) -> bool {
        self.ingress.is_some() && self.shared.refresh.swap(false, Ordering::AcqRel)
    }

    pub fn result(&self) -> Option<RecordingResult> {
        self.shared.result.get().cloned()
    }

    pub fn state(&self) -> RecordingState {
        match self.shared.state.load(Ordering::Acquire) {
            0 => RecordingState::Arming,
            1 => RecordingState::Recording,
            2 => RecordingState::Complete,
            3 => RecordingState::HasGaps,
            _ => RecordingState::Failed,
        }
    }
}

fn state_code(state: RecordingState) -> u8 {
    match state {
        RecordingState::Arming => 0,
        RecordingState::Recording => 1,
        RecordingState::Complete => 2,
        RecordingState::HasGaps => 3,
        RecordingState::Failed => 4,
    }
}

fn run(parent: PathBuf, mut inbox: RecordingInbox, shared: &Shared) -> RecordingResult {
    let mut writer = match EncodedWriter::create(&parent) {
        Ok(writer) => writer,
        Err(error) => {
            return RecordingResult {
                path: None,
                state: RecordingState::Failed,
                error: Some(error.to_string()),
            }
        }
    };
    let _ = shared.path.set(writer.path().to_owned());
    let result = pump(&mut writer, &mut inbox, shared);
    if let Err(error) = &result {
        writer.abort(&error.to_string());
    }
    RecordingResult {
        path: Some(writer.path().to_owned()),
        state: writer.state(),
        error: result.err().map(|error| error.to_string()),
    }
}

fn pump(
    writer: &mut EncodedWriter,
    inbox: &mut RecordingInbox,
    shared: &Shared,
) -> Result<(), RecordingError> {
    let mut reorder = RecordingReorder::new();
    let mut waiting_since = Some(Instant::now());
    loop {
        if writer.take_refresh_request() {
            shared.refresh.store(true, Ordering::Release);
        }
        let now = Instant::now();
        let (batch, drained, idle) = match inbox.poll() {
            Ok(IngressPoll::Input(input)) => (reorder.push(input, now)?, false, false),
            Ok(IngressPoll::Gap(reason, source)) => {
                for input in reorder.drain() {
                    writer.write_ordered(input)?;
                }
                writer.gap(reason, source)?;
                (Vec::new(), false, false)
            }
            Ok(IngressPoll::Idle) => (reorder.poll(now), false, true),
            Ok(IngressPoll::Drained) => (reorder.drain(), true, false),
            Err(failure) => {
                return Err(RecordingError::Platform(format!(
                    "recording ingress: {failure:?}"
                )))
            }
        };
        for input in batch {
            writer.write_ordered(input)?;
        }
        shared
            .state
            .store(state_code(writer.state()), Ordering::Release);
        if drained {
            return writer.finish();
        }
        if writer.waiting_for_refresh() {
            let started = waiting_since.get_or_insert(now);
            if now.saturating_duration_since(*started) >= ARMING_TIMEOUT {
                return Err(RecordingError::Platform(
                    "recording random-access wait expired".into(),
                ));
            }
        } else {
            waiting_since = None;
        }
        if idle {
            std::thread::sleep(Duration::from_millis(2));
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn wait(worker: &RecordingWorker) -> RecordingResult {
        let deadline = Instant::now() + Duration::from_secs(15);
        loop {
            if let Some(result) = worker.result() {
                // The final snapshot precedes thread-local destruction. A new
                // worker may start only after the old slot is actually released.
                while ACTIVE.load(Ordering::Acquire) {
                    assert!(Instant::now() < deadline);
                    std::thread::sleep(Duration::from_millis(1));
                }
                return result;
            }
            assert!(Instant::now() < deadline, "recording did not finish");
            std::thread::sleep(Duration::from_millis(2));
        }
    }

    #[test]
    fn worker_owns_native_objects_drains_stop_and_bounds_arming_and_reuse() {
        let parent = tempfile::tempdir().unwrap();
        let mut worker = RecordingWorker::start(parent.path().to_owned()).unwrap();
        assert!(RecordingWorker::start(parent.path().to_owned()).is_err());
        worker
            .offer(crate::encoded::tests::input(0, 1, 2, 33_333))
            .unwrap();
        worker
            .offer(crate::encoded::tests::input(0, 1, 1, 0))
            .unwrap();
        worker.stop();
        let result = wait(&worker);
        assert_eq!(result.state, RecordingState::Complete);
        assert!(result.error.is_none());
        let manifest: serde_json::Value = serde_json::from_slice(
            &std::fs::read(result.path.unwrap().join("manifest.json")).unwrap(),
        )
        .unwrap();
        assert_eq!(manifest["segments"][0]["metadata"]["source"]["first_au"], 1);
        assert_eq!(manifest["segments"][0]["metadata"]["source"]["last_au"], 2);

        let mut tail_loss = RecordingWorker::start(parent.path().to_owned()).unwrap();
        tail_loss
            .offer(crate::encoded::tests::input(0, 1, 1, 0))
            .unwrap();
        tail_loss
            .report_gap(crate::bundle::GapReason::NetworkLoss, None)
            .unwrap();
        tail_loss.stop();
        assert_eq!(wait(&tail_loss).state, RecordingState::HasGaps);

        let missing = RecordingWorker::start(parent.path().join("missing")).unwrap();
        assert_eq!(wait(&missing).state, RecordingState::Failed);

        let arming = RecordingWorker::start(parent.path().to_owned()).unwrap();
        let result = wait(&arming);
        assert_eq!(result.state, RecordingState::Failed);
        assert!(result.error.unwrap().contains("random-access wait expired"));
    }
}
