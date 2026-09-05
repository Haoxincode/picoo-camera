use std::sync::Arc;

use futures_channel::oneshot;
use picoo_protocol::control::CameraCommand;
use picoo_receiver::runtime::{
    ReceiverRuntimeAdapter, ReceiverRuntimeHandle as CoreRuntimeHandle, RuntimeCommandOutcome,
};
use picoo_receiver::ReceiverError;

use super::{ReceiverRuntime, ReceiverSnapshot};
use crate::prefs::DesktopPreferences;

pub(crate) enum ReceiverCommand {
    SetDisplayName(String),
    SetAutoAcceptPaired(bool),
    SetPlaceholderMode(picoo_frame_hub::PlaceholderMode),
    SetVirtualCameraStatus(crate::model::VirtualCameraStatus),
    Disconnect,
    SendCameraCommand(CameraCommand, oneshot::Sender<Result<(), ReceiverError>>),
    RequestKeyframe(oneshot::Sender<Result<(), ReceiverError>>),
    ConfirmPairing(oneshot::Sender<Result<(), ReceiverError>>),
    RejectPairing(oneshot::Sender<Result<(), ReceiverError>>),
    RemoveTrustedDevice(String, oneshot::Sender<Result<bool, ReceiverError>>),
    ReplaceTrustedIdentityHistory(u64, oneshot::Sender<Result<usize, ReceiverError>>),
    DismissTrustedIdentityReplacement(u64, oneshot::Sender<Result<bool, ReceiverError>>),
    ClearTrustedDevices(oneshot::Sender<Result<usize, ReceiverError>>),
    Shutdown,
}

/// GPUI-facing handle for the dedicated Receiver owner thread.
///
/// The handle carries commands and immutable snapshots only; QUIC, decoder,
/// reassembly, jitter, mDNS, and Shared Ring resources never move into a GPUI
/// entity or run inside a UI update closure.
pub struct ReceiverRuntimeHandle {
    inner: CoreRuntimeHandle<ReceiverCommand, ReceiverSnapshot>,
}

pub type ReceiverReply<T> = oneshot::Receiver<Result<T, ReceiverError>>;

pub async fn await_receiver_reply<T>(reply: ReceiverReply<T>) -> Result<T, ReceiverError> {
    reply
        .await
        .map_err(|_| ReceiverError::Protocol("Receiver worker response channel closed".into()))?
}

impl ReceiverRuntimeHandle {
    pub fn start_from_prefs(
        prefs: DesktopPreferences,
        virtual_camera: crate::model::VirtualCameraStatus,
    ) -> Result<Self, ReceiverError> {
        let inner = CoreRuntimeHandle::start(move || {
            let mut runtime = ReceiverRuntime::from_prefs(&prefs)?;
            runtime.set_virtual_camera_status(virtual_camera);
            Ok(runtime)
        })?;
        Ok(Self { inner })
    }

    pub fn snapshot(&self) -> Arc<ReceiverSnapshot> {
        self.inner.snapshot()
    }

    pub fn latest_frame(&self) -> Option<Arc<picoo_frame_hub::VideoFrame>> {
        self.inner.latest_frame()
    }

    fn send(&self, command: ReceiverCommand) {
        let _ = self.inner.submit(command);
    }

    fn request<T>(
        &self,
        command: impl FnOnce(oneshot::Sender<Result<T, ReceiverError>>) -> ReceiverCommand,
    ) -> ReceiverReply<T> {
        let (response_tx, response_rx) = oneshot::channel();
        let _ = self.inner.submit(command(response_tx));
        response_rx
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

    pub fn send_camera_command(&self, command: CameraCommand) -> ReceiverReply<()> {
        self.request(|response| ReceiverCommand::SendCameraCommand(command, response))
    }

    pub fn request_keyframe(&self) -> ReceiverReply<()> {
        self.request(ReceiverCommand::RequestKeyframe)
    }

    pub fn confirm_pairing(&self) -> ReceiverReply<()> {
        self.request(ReceiverCommand::ConfirmPairing)
    }

    pub fn reject_pairing(&self) -> ReceiverReply<()> {
        self.request(ReceiverCommand::RejectPairing)
    }

    pub fn remove_trusted_device(&self, device_id: &str) -> ReceiverReply<bool> {
        self.request(|response| {
            ReceiverCommand::RemoveTrustedDevice(device_id.to_owned(), response)
        })
    }

    pub fn replace_trusted_identity_history(&self, revision: u64) -> ReceiverReply<usize> {
        self.request(|response| ReceiverCommand::ReplaceTrustedIdentityHistory(revision, response))
    }

    pub fn dismiss_trusted_identity_replacement(&self, revision: u64) -> ReceiverReply<bool> {
        self.request(|response| {
            ReceiverCommand::DismissTrustedIdentityReplacement(revision, response)
        })
    }

    pub fn clear_trusted_devices(&self) -> ReceiverReply<usize> {
        self.request(ReceiverCommand::ClearTrustedDevices)
    }
}

fn apply_receiver_command(
    runtime: &mut ReceiverRuntime,
    command: ReceiverCommand,
) -> RuntimeCommandOutcome {
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
        ReceiverCommand::Shutdown => return RuntimeCommandOutcome::Shutdown,
    }
    RuntimeCommandOutcome::Continue
}

impl ReceiverRuntimeAdapter for ReceiverRuntime {
    type Command = ReceiverCommand;
    type Snapshot = ReceiverSnapshot;

    fn runtime_wake(&self) -> picoo_transport::TransportEventWake {
        self.receiver().runtime_wake()
    }

    fn shutdown_command() -> Self::Command {
        ReceiverCommand::Shutdown
    }

    fn apply_command(&mut self, command: Self::Command) -> RuntimeCommandOutcome {
        apply_receiver_command(self, command)
    }

    fn pump(&mut self) -> Result<(), ReceiverError> {
        ReceiverRuntime::pump(self)
    }

    fn next_wake_delay(&self) -> std::time::Duration {
        self.receiver().next_wake_delay()
    }

    fn snapshot(&self) -> Self::Snapshot {
        ReceiverRuntime::snapshot(self)
    }

    fn latest_frame(&self) -> Option<Arc<picoo_frame_hub::VideoFrame>> {
        self.receiver().latest_frame().cloned()
    }
}
