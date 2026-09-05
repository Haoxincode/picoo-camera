use std::sync::mpsc::{self, SyncSender};
use std::sync::{Arc, RwLock};
use std::thread::{self, JoinHandle};
use std::time::{Duration, Instant};

use picoo_protocol::control::CameraCommand;
use picoo_receiver::ReceiverError;

use super::{ReceiverRuntime, ReceiverSnapshot};
use crate::prefs::DesktopPreferences;

enum ReceiverCommand {
    SetDisplayName(String),
    SetAutoAcceptPaired(bool),
    SetPlaceholderMode(picoo_frame_hub::PlaceholderMode),
    SetVirtualCameraStatus(crate::model::VirtualCameraStatus),
    Disconnect,
    SendCameraCommand(CameraCommand, SyncSender<Result<(), ReceiverError>>),
    RequestKeyframe(SyncSender<Result<(), ReceiverError>>),
    ConfirmPairing(SyncSender<Result<(), ReceiverError>>),
    RejectPairing(SyncSender<Result<(), ReceiverError>>),
    RemoveTrustedDevice(String, SyncSender<Result<bool, ReceiverError>>),
    ReplaceTrustedIdentityHistory(u64, SyncSender<Result<usize, ReceiverError>>),
    DismissTrustedIdentityReplacement(u64, SyncSender<Result<bool, ReceiverError>>),
    ClearTrustedDevices(SyncSender<Result<usize, ReceiverError>>),
    Shutdown,
}

struct ReceiverWorkerShared {
    snapshot: RwLock<ReceiverSnapshot>,
    latest_frame: RwLock<Option<Arc<picoo_frame_hub::VideoFrame>>>,
}

/// GPUI-facing handle for the dedicated Receiver owner thread.
///
/// The handle carries commands and immutable snapshots only; QUIC, decoder,
/// reassembly, jitter, mDNS, and Shared Ring resources never move into a GPUI
/// entity or run inside a UI update closure.
pub struct ReceiverRuntimeHandle {
    commands: mpsc::Sender<ReceiverCommand>,
    wake: picoo_transport::TransportEventWake,
    shared: Arc<ReceiverWorkerShared>,
    worker: Option<JoinHandle<()>>,
}

impl ReceiverRuntimeHandle {
    pub fn start_from_prefs(
        prefs: DesktopPreferences,
        virtual_camera: crate::model::VirtualCameraStatus,
    ) -> Result<Self, ReceiverError> {
        let (commands, command_rx) = mpsc::channel();
        let (startup_tx, startup_rx) = mpsc::sync_channel(1);
        let worker = thread::Builder::new()
            .name("picoo-receiver".into())
            .spawn(move || {
                let startup = ReceiverRuntime::from_prefs(&prefs).map(|mut runtime| {
                    runtime.set_virtual_camera_status(virtual_camera);
                    let wake = runtime.receiver().runtime_wake();
                    let shared = Arc::new(ReceiverWorkerShared {
                        snapshot: RwLock::new(runtime.snapshot()),
                        latest_frame: RwLock::new(runtime.receiver().latest_frame().cloned()),
                    });
                    let _ = startup_tx.send(Ok((wake.clone(), Arc::clone(&shared))));
                    run_receiver_worker(runtime, command_rx, wake, shared);
                });
                if let Err(error) = startup {
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
            worker: Some(worker),
        })
    }

    pub fn snapshot(&self) -> ReceiverSnapshot {
        self.shared
            .snapshot
            .read()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
            .clone()
    }

    pub fn latest_frame(&self) -> Option<Arc<picoo_frame_hub::VideoFrame>> {
        self.shared
            .latest_frame
            .read()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
            .clone()
    }

    fn send(&self, command: ReceiverCommand) {
        if self.commands.send(command).is_ok() {
            self.wake.signal();
        }
    }

    fn request<T>(
        &self,
        command: impl FnOnce(SyncSender<Result<T, ReceiverError>>) -> ReceiverCommand,
    ) -> Result<T, ReceiverError> {
        let (response_tx, response_rx) = mpsc::sync_channel(1);
        self.commands.send(command(response_tx)).map_err(|_| {
            ReceiverError::Protocol("Receiver worker command channel closed".into())
        })?;
        self.wake.signal();
        response_rx.recv().map_err(|_| {
            ReceiverError::Protocol("Receiver worker response channel closed".into())
        })?
    }

    pub fn set_display_name(&self, name: String) {
        self.send(ReceiverCommand::SetDisplayName(name));
    }

    pub fn set_auto_accept_paired(&self, enabled: bool) {
        self.send(ReceiverCommand::SetAutoAcceptPaired(enabled));
    }

    pub fn set_placeholder_mode(&self, mode: picoo_frame_hub::PlaceholderMode) {
        self.send(ReceiverCommand::SetPlaceholderMode(mode));
    }

    pub fn set_virtual_camera_status(&self, status: crate::model::VirtualCameraStatus) {
        self.send(ReceiverCommand::SetVirtualCameraStatus(status));
    }

    pub fn disconnect(&self) {
        self.send(ReceiverCommand::Disconnect);
    }

    pub fn send_camera_command(&self, command: CameraCommand) -> Result<(), ReceiverError> {
        self.request(|response| ReceiverCommand::SendCameraCommand(command, response))
    }

    pub fn request_keyframe(&self) -> Result<(), ReceiverError> {
        self.request(ReceiverCommand::RequestKeyframe)
    }

    pub fn confirm_pairing(&self) -> Result<(), ReceiverError> {
        self.request(ReceiverCommand::ConfirmPairing)
    }

    pub fn reject_pairing(&self) -> Result<(), ReceiverError> {
        self.request(ReceiverCommand::RejectPairing)
    }

    pub fn remove_trusted_device(&self, device_id: &str) -> Result<bool, ReceiverError> {
        self.request(|response| {
            ReceiverCommand::RemoveTrustedDevice(device_id.to_owned(), response)
        })
    }

    pub fn replace_trusted_identity_history(&self, revision: u64) -> Result<usize, ReceiverError> {
        self.request(|response| ReceiverCommand::ReplaceTrustedIdentityHistory(revision, response))
    }

    pub fn dismiss_trusted_identity_replacement(
        &self,
        revision: u64,
    ) -> Result<bool, ReceiverError> {
        self.request(|response| {
            ReceiverCommand::DismissTrustedIdentityReplacement(revision, response)
        })
    }

    pub fn clear_trusted_devices(&self) -> Result<usize, ReceiverError> {
        self.request(ReceiverCommand::ClearTrustedDevices)
    }
}

impl Drop for ReceiverRuntimeHandle {
    fn drop(&mut self) {
        self.send(ReceiverCommand::Shutdown);
        if let Some(worker) = self.worker.take() {
            let _ = worker.join();
        }
    }
}

fn run_receiver_worker(
    mut runtime: ReceiverRuntime,
    commands: mpsc::Receiver<ReceiverCommand>,
    wake: picoo_transport::TransportEventWake,
    shared: Arc<ReceiverWorkerShared>,
) {
    let mut observed_revision = wake.revision();
    loop {
        let command_started = Instant::now();
        let mut command_count = 0_usize;
        let mut shutdown = false;
        while command_count < 64 && command_started.elapsed() < Duration::from_millis(2) {
            let Ok(command) = commands.try_recv() else {
                break;
            };
            command_count += 1;
            if apply_receiver_command(&mut runtime, command) {
                shutdown = true;
                break;
            }
        }
        if shutdown {
            return;
        }
        if command_count == 64 {
            wake.signal();
        }

        if let Err(error) = runtime.pump() {
            tracing::warn!(%error, "Receiver pump failed");
        }
        let snapshot = runtime.snapshot();
        *shared
            .snapshot
            .write()
            .unwrap_or_else(std::sync::PoisonError::into_inner) = snapshot;
        let latest = runtime.receiver().latest_frame().cloned();
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

        let delay = runtime.receiver().next_wake_delay();
        observed_revision = wake.wait_after(observed_revision, delay);
    }
}

fn apply_receiver_command(runtime: &mut ReceiverRuntime, command: ReceiverCommand) -> bool {
    match command {
        ReceiverCommand::SetDisplayName(name) => runtime.set_display_name(name),
        ReceiverCommand::SetAutoAcceptPaired(enabled) => runtime.set_auto_accept_paired(enabled),
        ReceiverCommand::SetPlaceholderMode(mode) => runtime.set_placeholder_mode(mode),
        ReceiverCommand::SetVirtualCameraStatus(status) => {
            runtime.set_virtual_camera_status(status)
        }
        ReceiverCommand::Disconnect => runtime.disconnect(),
        ReceiverCommand::SendCameraCommand(command, response) => {
            let _ = response.send(runtime.send_camera_command(command));
        }
        ReceiverCommand::RequestKeyframe(response) => {
            let _ = response.send(runtime.request_keyframe());
        }
        ReceiverCommand::ConfirmPairing(response) => {
            let _ = response.send(runtime.confirm_pairing());
        }
        ReceiverCommand::RejectPairing(response) => {
            let _ = response.send(runtime.reject_pairing());
        }
        ReceiverCommand::RemoveTrustedDevice(device_id, response) => {
            let _ = response.send(runtime.remove_trusted_device(&device_id));
        }
        ReceiverCommand::ReplaceTrustedIdentityHistory(revision, response) => {
            let _ = response.send(runtime.replace_trusted_identity_history(revision));
        }
        ReceiverCommand::DismissTrustedIdentityReplacement(revision, response) => {
            let _ = response.send(runtime.dismiss_trusted_identity_replacement(revision));
        }
        ReceiverCommand::ClearTrustedDevices(response) => {
            let _ = response.send(runtime.clear_trusted_devices());
        }
        ReceiverCommand::Shutdown => return true,
    }
    false
}
