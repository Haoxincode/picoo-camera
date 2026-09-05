//! Dedicated Receiver owner loop — REQ-PICOO-SESSION-015/017.
//!
//! The Core owns command fairness, event/deadline waiting, immutable snapshot
//! publication, and teardown. Desktop code supplies only the platform adapter
//! that applies commands and derives its presentation snapshot.

use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::mpsc::{self, Receiver, SyncSender, TryRecvError, TrySendError};
use std::sync::{Arc, Mutex, RwLock};
use std::thread::{self, JoinHandle};
use std::time::{Duration, Instant};

use picoo_frame_hub::VideoFrame;
use picoo_transport::TransportEventWake;

use crate::ReceiverError;

const MAX_COMMANDS_PER_TURN: usize = 64;
const MAX_PENDING_COMMANDS: usize = 128;
const COMMAND_TIME_BUDGET: Duration = Duration::from_millis(2);
const SNAPSHOT_PUBLISH_INTERVAL: Duration = Duration::from_millis(100);

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum RuntimeCommandOutcome {
    Continue,
    Shutdown,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum RuntimeCommandSubmitOutcome {
    Queued,
    Full,
    Closed,
}

#[derive(Debug)]
pub enum RuntimeCommandSubmitError<Command> {
    Full(Command),
    Closed(Command),
}

impl<Command> RuntimeCommandSubmitError<Command> {
    pub fn outcome(&self) -> RuntimeCommandSubmitOutcome {
        match self {
            Self::Full(_) => RuntimeCommandSubmitOutcome::Full,
            Self::Closed(_) => RuntimeCommandSubmitOutcome::Closed,
        }
    }

    pub fn into_command(self) -> Command {
        match self {
            Self::Full(command) | Self::Closed(command) => command,
        }
    }
}

/// Platform seam for the Core-owned Receiver loop.
///
/// Implementations are constructed and used on the owner thread. They must not
/// expose transport, decoder, reassembly, or mutable session state to the UI.
pub trait ReceiverRuntimeAdapter: 'static {
    type Command: Send + 'static;
    type Snapshot: PartialEq + Send + Sync + 'static;

    fn runtime_wake(&self) -> TransportEventWake;
    fn shutdown_command() -> Self::Command;
    fn apply_command(&mut self, command: Self::Command) -> RuntimeCommandOutcome;
    fn pump(&mut self) -> Result<(), ReceiverError>;
    fn next_wake_delay(&self) -> Duration;
    fn snapshot(&self) -> Self::Snapshot;
    fn latest_frame(&self) -> Option<Arc<VideoFrame>>;
}

struct RuntimeShared<Snapshot> {
    snapshot: RwLock<Arc<Snapshot>>,
    latest_frame: RwLock<Option<Arc<VideoFrame>>>,
    closed: AtomicBool,
}

struct LatestCommandSlot<Command> {
    command: Mutex<Option<Command>>,
}

impl<Command> Default for LatestCommandSlot<Command> {
    fn default() -> Self {
        Self {
            command: Mutex::new(None),
        }
    }
}

impl<Command> LatestCommandSlot<Command> {
    fn replace(&self, command: Command) {
        *self
            .command
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner) = Some(command);
    }

    fn take(&self) -> Option<Command> {
        self.command
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
            .take()
    }
}

/// Command/snapshot handle for a Core-owned Receiver runtime.
pub struct ReceiverRuntimeHandle<Command, Snapshot> {
    commands: SyncSender<Command>,
    latest_command: Arc<LatestCommandSlot<Command>>,
    wake: TransportEventWake,
    shared: Arc<RuntimeShared<Snapshot>>,
    shutdown_command: fn() -> Command,
    worker: Option<JoinHandle<()>>,
}

impl<Command, Snapshot> ReceiverRuntimeHandle<Command, Snapshot>
where
    Command: Send + 'static,
    Snapshot: PartialEq + Send + Sync + 'static,
{
    pub fn start<Adapter>(
        factory: impl FnOnce() -> Result<Adapter, ReceiverError> + Send + 'static,
    ) -> Result<Self, ReceiverError>
    where
        Adapter: ReceiverRuntimeAdapter<Command = Command, Snapshot = Snapshot>,
    {
        let (commands, command_rx) = mpsc::sync_channel(MAX_PENDING_COMMANDS);
        let latest_command = Arc::new(LatestCommandSlot::default());
        let worker_latest_command = Arc::clone(&latest_command);
        let (startup_tx, startup_rx) = mpsc::sync_channel(1);
        let worker = thread::Builder::new()
            .name("picoo-receiver".into())
            .spawn(move || match factory() {
                Ok(adapter) => {
                    let wake = adapter.runtime_wake();
                    let shared = Arc::new(RuntimeShared {
                        snapshot: RwLock::new(Arc::new(adapter.snapshot())),
                        latest_frame: RwLock::new(adapter.latest_frame()),
                        closed: AtomicBool::new(false),
                    });
                    let _ = startup_tx.send(Ok((wake.clone(), Arc::clone(&shared))));
                    run_owner_loop(
                        adapter,
                        command_rx,
                        worker_latest_command,
                        wake,
                        Arc::clone(&shared),
                    );
                    shared.closed.store(true, Ordering::Release);
                }
                Err(error) => {
                    let _ = startup_tx.send(Err(error));
                }
            })
            .map_err(|error| ReceiverError::Protocol(format!("start Receiver worker: {error}")))?;
        let (wake, shared) = startup_rx.recv().map_err(|_| {
            ReceiverError::Protocol("Receiver worker exited during startup".into())
        })??;
        Ok(Self {
            commands,
            latest_command,
            wake,
            shared,
            shutdown_command: Adapter::shutdown_command,
            worker: Some(worker),
        })
    }

    pub fn snapshot(&self) -> Arc<Snapshot> {
        Arc::clone(
            &self
                .shared
                .snapshot
                .read()
                .unwrap_or_else(std::sync::PoisonError::into_inner),
        )
    }

    pub fn latest_frame(&self) -> Option<Arc<VideoFrame>> {
        self.shared
            .latest_frame
            .read()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
            .clone()
    }

    /// Non-blocking bounded submission for commands whose individual outcome
    /// matters. Full and closed are explicit so callers can reject side effects.
    pub fn submit(&self, command: Command) -> Result<(), RuntimeCommandSubmitError<Command>> {
        try_send_command(&self.commands, command)?;
        self.wake.signal();
        Ok(())
    }

    /// Capacity-one latest-value path for coalescible platform settings.
    pub fn submit_latest(&self, command: Command) -> RuntimeCommandSubmitOutcome {
        if self.shared.closed.load(Ordering::Acquire) {
            return RuntimeCommandSubmitOutcome::Closed;
        }
        self.latest_command.replace(command);
        self.wake.signal();
        RuntimeCommandSubmitOutcome::Queued
    }
}

impl<Command, Snapshot> Drop for ReceiverRuntimeHandle<Command, Snapshot> {
    fn drop(&mut self) {
        let _ = self.commands.try_send((self.shutdown_command)());
        self.wake.signal();
        if let Some(worker) = self.worker.take() {
            // UI teardown never waits for platform decoder/transport cleanup.
            let _ = thread::Builder::new()
                .name("picoo-receiver-shutdown".into())
                .spawn(move || {
                    let _ = worker.join();
                });
        }
    }
}

fn run_owner_loop<Adapter>(
    mut adapter: Adapter,
    commands: Receiver<Adapter::Command>,
    latest_command: Arc<LatestCommandSlot<Adapter::Command>>,
    wake: TransportEventWake,
    shared: Arc<RuntimeShared<Adapter::Snapshot>>,
) where
    Adapter: ReceiverRuntimeAdapter,
{
    let mut observed_revision = wake.revision();
    let mut snapshot_cadence = SnapshotCadence::new(Instant::now());
    loop {
        let command_started = Instant::now();
        let mut command_count = 0_usize;
        let mut command_channel_closed = false;
        if let Some(command) = latest_command.take() {
            command_count = 1;
            if adapter.apply_command(command) == RuntimeCommandOutcome::Shutdown {
                return;
            }
        }
        while command_count < MAX_COMMANDS_PER_TURN
            && command_started.elapsed() < COMMAND_TIME_BUDGET
        {
            let command = match commands.try_recv() {
                Ok(command) => command,
                Err(TryRecvError::Empty) => break,
                Err(TryRecvError::Disconnected) => {
                    command_channel_closed = true;
                    break;
                }
            };
            command_count += 1;
            if adapter.apply_command(command) == RuntimeCommandOutcome::Shutdown {
                return;
            }
        }
        if command_channel_closed {
            return;
        }
        if command_count == MAX_COMMANDS_PER_TURN
            || (command_count > 0 && command_started.elapsed() >= COMMAND_TIME_BUDGET)
        {
            wake.signal();
        }

        if let Err(error) = adapter.pump() {
            tracing::warn!(%error, "Receiver pump failed");
        }
        let now = Instant::now();
        if snapshot_cadence.take_due(now, command_count > 0) {
            let snapshot = adapter.snapshot();
            let mut published = shared
                .snapshot
                .write()
                .unwrap_or_else(std::sync::PoisonError::into_inner);
            if snapshot != **published {
                *published = Arc::new(snapshot);
            }
        }
        let latest = adapter.latest_frame();
        let mut published = shared
            .latest_frame
            .write()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        if latest.as_ref().map(|frame| frame.sequence)
            != published.as_ref().map(|frame| frame.sequence)
        {
            *published = latest;
        }
        drop(published);

        let delay = adapter
            .next_wake_delay()
            .min(snapshot_cadence.next_delay(Instant::now()));
        observed_revision = wake.wait_after(observed_revision, delay);
    }
}

fn try_send_command<Command>(
    sender: &SyncSender<Command>,
    command: Command,
) -> Result<(), RuntimeCommandSubmitError<Command>> {
    match sender.try_send(command) {
        Ok(()) => Ok(()),
        Err(TrySendError::Full(command)) => Err(RuntimeCommandSubmitError::Full(command)),
        Err(TrySendError::Disconnected(command)) => Err(RuntimeCommandSubmitError::Closed(command)),
    }
}

struct SnapshotCadence {
    next_at: Instant,
}

impl SnapshotCadence {
    fn new(now: Instant) -> Self {
        Self {
            next_at: now + SNAPSHOT_PUBLISH_INTERVAL,
        }
    }

    fn take_due(&mut self, now: Instant, command_processed: bool) -> bool {
        if !command_processed && now < self.next_at {
            return false;
        }
        self.next_at = now + SNAPSHOT_PUBLISH_INTERVAL;
        true
    }

    fn next_delay(&self, now: Instant) -> Duration {
        self.next_at.saturating_duration_since(now)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn media_wake_storm_does_not_rebuild_full_snapshot_each_time() {
        let start = Instant::now();
        let mut cadence = SnapshotCadence::new(start);
        let builds = (1..=99)
            .filter(|millis| cadence.take_due(start + Duration::from_millis(*millis), false))
            .count();
        assert_eq!(builds, 0);
        assert!(cadence.take_due(start + SNAPSHOT_PUBLISH_INTERVAL, false));
    }

    #[test]
    fn completed_command_publishes_without_waiting_for_stats_cadence() {
        let start = Instant::now();
        let mut cadence = SnapshotCadence::new(start);
        assert!(cadence.take_due(start + Duration::from_millis(1), true));
        assert_eq!(
            cadence.next_delay(start + Duration::from_millis(1)),
            SNAPSHOT_PUBLISH_INTERVAL
        );
    }

    #[test]
    fn latest_command_slot_keeps_only_the_newest_value() {
        let slot = LatestCommandSlot::default();
        slot.replace(1_u64);
        slot.replace(2_u64);

        assert_eq!(slot.take(), Some(2));
        assert_eq!(slot.take(), None);
    }

    #[test]
    fn bounded_command_submission_reports_full_without_blocking() {
        let (sender, _receiver) = mpsc::sync_channel(1);
        assert!(try_send_command(&sender, 1_u64).is_ok());
        let error = try_send_command(&sender, 2_u64).expect_err("queue should be full");
        assert_eq!(error.outcome(), RuntimeCommandSubmitOutcome::Full);
        assert_eq!(error.into_command(), 2);
    }
}
