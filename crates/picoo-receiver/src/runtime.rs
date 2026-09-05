//! Dedicated Receiver owner loop — REQ-PICOO-SESSION-015/017.
//!
//! The Core owns command fairness, event/deadline waiting, immutable snapshot
//! publication, and teardown. Desktop code supplies only the platform adapter
//! that applies commands and derives its presentation snapshot.

use std::sync::mpsc::{self, Receiver, Sender, TryRecvError};
use std::sync::{Arc, RwLock};
use std::thread::{self, JoinHandle};
use std::time::{Duration, Instant};

use picoo_frame_hub::VideoFrame;
use picoo_transport::TransportEventWake;

use crate::ReceiverError;

const MAX_COMMANDS_PER_TURN: usize = 64;
const COMMAND_TIME_BUDGET: Duration = Duration::from_millis(2);
const SNAPSHOT_PUBLISH_INTERVAL: Duration = Duration::from_millis(100);

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum RuntimeCommandOutcome {
    Continue,
    Shutdown,
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
}

/// Command/snapshot handle for a Core-owned Receiver runtime.
pub struct ReceiverRuntimeHandle<Command, Snapshot> {
    commands: Sender<Command>,
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
        let (commands, command_rx) = mpsc::channel();
        let (startup_tx, startup_rx) = mpsc::sync_channel(1);
        let worker = thread::Builder::new()
            .name("picoo-receiver".into())
            .spawn(move || match factory() {
                Ok(adapter) => {
                    let wake = adapter.runtime_wake();
                    let shared = Arc::new(RuntimeShared {
                        snapshot: RwLock::new(Arc::new(adapter.snapshot())),
                        latest_frame: RwLock::new(adapter.latest_frame()),
                    });
                    let _ = startup_tx.send(Ok((wake.clone(), Arc::clone(&shared))));
                    run_owner_loop(adapter, command_rx, wake, shared);
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

    /// Submit a command and wake the owner. `false` means the owner has closed.
    pub fn submit(&self, command: Command) -> bool {
        if self.commands.send(command).is_err() {
            return false;
        }
        self.wake.signal();
        true
    }
}

impl<Command, Snapshot> Drop for ReceiverRuntimeHandle<Command, Snapshot> {
    fn drop(&mut self) {
        if self.commands.send((self.shutdown_command)()).is_ok() {
            self.wake.signal();
        }
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
}
