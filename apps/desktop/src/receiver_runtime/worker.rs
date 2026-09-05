use std::sync::{Arc, Mutex};

use futures_channel::oneshot;
use picoo_protocol::control::CameraCommand;
use picoo_receiver::runtime::{
    ReceiverRuntimeAdapter, ReceiverRuntimeHandle as CoreRuntimeHandle, RuntimeCommandOutcome,
    RuntimeCommandSubmitOutcome,
};
use picoo_receiver::ReceiverError;

use super::{ReceiverRuntime, ReceiverSnapshot};
use crate::prefs::DesktopPreferences;

#[cfg_attr(not(feature = "gpui-ui"), allow(dead_code))]
pub(crate) enum ReceiverCommand {
    ApplyPendingSettings(Arc<Mutex<PendingReceiverSettings>>),
    Disconnect(oneshot::Sender<Result<(), ReceiverError>>),
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

#[derive(Default)]
pub(crate) struct PendingReceiverSettings {
    display_name: Option<String>,
    auto_accept_paired: Option<bool>,
    placeholder_mode: Option<picoo_frame_hub::PlaceholderMode>,
    virtual_camera_status: Option<crate::model::VirtualCameraStatus>,
}

impl ReceiverCommand {
    fn reject(self, error: ReceiverError) {
        match self {
            Self::Disconnect(response)
            | Self::SendCameraCommand(_, response)
            | Self::RequestKeyframe(response)
            | Self::ConfirmPairing(response)
            | Self::RejectPairing(response) => {
                let _ = response.send(Err(error));
            }
            Self::RemoveTrustedDevice(_, response)
            | Self::DismissTrustedIdentityReplacement(_, response) => {
                let _ = response.send(Err(error));
            }
            Self::ReplaceTrustedIdentityHistory(_, response)
            | Self::ClearTrustedDevices(response) => {
                let _ = response.send(Err(error));
            }
            Self::ApplyPendingSettings(_) | Self::Shutdown => {}
        }
    }
}

/// GPUI-facing handle for the dedicated Receiver owner thread.
///
/// The handle carries commands and immutable snapshots only; QUIC, decoder,
/// reassembly, jitter, mDNS, and Shared Ring resources never move into a GPUI
/// entity or run inside a UI update closure.
pub struct ReceiverRuntimeHandle {
    inner: CoreRuntimeHandle<ReceiverCommand, ReceiverSnapshot>,
    pending_settings: Arc<Mutex<PendingReceiverSettings>>,
}

pub type ReceiverReply<T> = oneshot::Receiver<Result<T, ReceiverError>>;

#[cfg(feature = "gpui-ui")]
pub async fn await_receiver_reply<T>(reply: ReceiverReply<T>) -> Result<T, ReceiverError> {
    reply
        .await
        .map_err(|_| ReceiverError::Protocol("Receiver worker response channel closed".into()))?
}

#[cfg_attr(not(feature = "gpui-ui"), allow(dead_code))]
impl ReceiverRuntimeHandle {
    pub fn start(config: super::ReceiverRuntimeConfig) -> Result<Self, ReceiverError> {
        let pending_settings = Arc::new(Mutex::new(PendingReceiverSettings::default()));
        let inner = CoreRuntimeHandle::start(move || ReceiverRuntime::start(config))?;
        Ok(Self {
            inner,
            pending_settings,
        })
    }

    pub fn start_from_prefs(
        prefs: DesktopPreferences,
        virtual_camera: crate::model::VirtualCameraStatus,
    ) -> Result<Self, ReceiverError> {
        let pending_settings = Arc::new(Mutex::new(PendingReceiverSettings::default()));
        let inner = CoreRuntimeHandle::start(move || {
            let mut runtime = ReceiverRuntime::from_prefs(&prefs)?;
            runtime.set_virtual_camera_status(virtual_camera);
            Ok(runtime)
        })?;
        Ok(Self {
            inner,
            pending_settings,
        })
    }

    pub fn snapshot(&self) -> Arc<ReceiverSnapshot> {
        self.inner.snapshot()
    }

    pub fn latest_frame(&self) -> Option<Arc<picoo_frame_hub::VideoFrame>> {
        self.inner.latest_frame()
    }

    fn update_settings(&self, update: impl FnOnce(&mut PendingReceiverSettings)) {
        update(
            &mut self
                .pending_settings
                .lock()
                .unwrap_or_else(std::sync::PoisonError::into_inner),
        );
        let outcome = self
            .inner
            .submit_latest(ReceiverCommand::ApplyPendingSettings(Arc::clone(
                &self.pending_settings,
            )));
        if outcome != RuntimeCommandSubmitOutcome::Queued {
            tracing::warn!(?outcome, "Receiver settings were not queued");
        }
    }

    fn request<T>(
        &self,
        command: impl FnOnce(oneshot::Sender<Result<T, ReceiverError>>) -> ReceiverCommand,
    ) -> ReceiverReply<T> {
        let (response_tx, response_rx) = oneshot::channel();
        if let Err(rejected) = self.inner.submit(command(response_tx)) {
            let outcome = rejected.outcome();
            let message = match outcome {
                RuntimeCommandSubmitOutcome::Full => "Receiver command queue is full",
                RuntimeCommandSubmitOutcome::Closed => "Receiver worker is closed",
                RuntimeCommandSubmitOutcome::Queued => unreachable!("queued commands are Ok"),
            };
            rejected
                .into_command()
                .reject(ReceiverError::Protocol(message.into()));
        }
        response_rx
    }

    pub fn set_display_name(&self, name: String) {
        self.update_settings(|settings| settings.display_name = Some(name));
    }

    pub fn set_auto_accept_paired(&self, enabled: bool) {
        self.update_settings(|settings| settings.auto_accept_paired = Some(enabled));
    }

    pub fn set_placeholder_mode(&self, mode: picoo_frame_hub::PlaceholderMode) {
        self.update_settings(|settings| settings.placeholder_mode = Some(mode));
    }

    pub fn set_virtual_camera_status(&self, status: crate::model::VirtualCameraStatus) {
        self.update_settings(|settings| settings.virtual_camera_status = Some(status));
    }

    pub fn disconnect(&self) -> ReceiverReply<()> {
        self.request(ReceiverCommand::Disconnect)
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
        ReceiverCommand::ApplyPendingSettings(settings) => {
            let settings = std::mem::take(
                &mut *settings
                    .lock()
                    .unwrap_or_else(std::sync::PoisonError::into_inner),
            );
            if let Some(name) = settings.display_name {
                runtime.set_display_name(name);
            }
            if let Some(enabled) = settings.auto_accept_paired {
                runtime.set_auto_accept_paired(enabled);
            }
            if let Some(mode) = settings.placeholder_mode {
                runtime.set_placeholder_mode(mode);
            }
            if let Some(status) = settings.virtual_camera_status {
                runtime.set_virtual_camera_status(status);
            }
        }
        ReceiverCommand::Disconnect(response) => {
            runtime.disconnect();
            let _ = response.send(Ok(()));
        }
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
            let result = runtime.clear_trusted_devices();
            if result.is_ok() {
                runtime.disconnect();
            }
            let _ = response.send(result);
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

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::atomic::{AtomicBool, Ordering};
    use std::time::{Duration, Instant};

    struct PausedAdapter {
        wake: picoo_transport::TransportEventWake,
        pump_entered: Arc<AtomicBool>,
        release_pump: Arc<AtomicBool>,
    }

    impl ReceiverRuntimeAdapter for PausedAdapter {
        type Command = ReceiverCommand;
        type Snapshot = u64;

        fn runtime_wake(&self) -> picoo_transport::TransportEventWake {
            self.wake.clone()
        }

        fn shutdown_command() -> Self::Command {
            ReceiverCommand::Shutdown
        }

        fn apply_command(&mut self, command: Self::Command) -> RuntimeCommandOutcome {
            if matches!(command, ReceiverCommand::Shutdown) {
                RuntimeCommandOutcome::Shutdown
            } else {
                RuntimeCommandOutcome::Continue
            }
        }

        fn pump(&mut self) -> Result<(), ReceiverError> {
            self.pump_entered.store(true, Ordering::Release);
            while !self.release_pump.load(Ordering::Acquire) {
                std::thread::yield_now();
            }
            Ok(())
        }

        fn next_wake_delay(&self) -> Duration {
            Duration::from_secs(1)
        }

        fn snapshot(&self) -> Self::Snapshot {
            0
        }

        fn latest_frame(&self) -> Option<Arc<picoo_frame_hub::VideoFrame>> {
            None
        }
    }

    #[test]
    fn rejected_side_effect_command_returns_an_explicit_reply() {
        let (response, mut reply) = oneshot::channel();
        ReceiverCommand::RequestKeyframe(response)
            .reject(ReceiverError::Protocol("queue full".into()));

        let result = reply
            .try_recv()
            .expect("reply channel remains valid")
            .expect("rejection reply is immediate");
        assert!(matches!(
            result,
            Err(ReceiverError::Protocol(message)) if message == "queue full"
        ));
    }

    #[test]
    fn rejected_disconnect_returns_an_explicit_reply() {
        let (response, mut reply) = oneshot::channel();
        ReceiverCommand::Disconnect(response).reject(ReceiverError::Protocol("queue full".into()));

        let result = reply
            .try_recv()
            .expect("reply channel remains valid")
            .expect("rejection reply is immediate");
        assert!(matches!(
            result,
            Err(ReceiverError::Protocol(message)) if message == "queue full"
        ));
    }

    #[test]
    fn full_owner_queue_rejects_disconnect_with_an_explicit_reply() {
        let pump_entered = Arc::new(AtomicBool::new(false));
        let release_pump = Arc::new(AtomicBool::new(false));
        let entered = Arc::clone(&pump_entered);
        let release = Arc::clone(&release_pump);
        let runtime = CoreRuntimeHandle::start(move || {
            Ok(PausedAdapter {
                wake: picoo_transport::TransportEventWake::default(),
                pump_entered: entered,
                release_pump: release,
            })
        })
        .expect("start paused Receiver owner");
        let deadline = Instant::now() + Duration::from_secs(1);
        while !pump_entered.load(Ordering::Acquire) && Instant::now() < deadline {
            std::thread::yield_now();
        }
        assert!(pump_entered.load(Ordering::Acquire));

        let settings = Arc::new(Mutex::new(PendingReceiverSettings::default()));
        for _ in 0..128 {
            assert!(
                runtime
                    .submit(ReceiverCommand::ApplyPendingSettings(Arc::clone(&settings)))
                    .is_ok(),
                "fill bounded command queue"
            );
        }
        let (response, mut reply) = oneshot::channel();
        let rejected = runtime
            .submit(ReceiverCommand::Disconnect(response))
            .expect_err("Disconnect must be rejected when the queue is full");
        assert_eq!(rejected.outcome(), RuntimeCommandSubmitOutcome::Full);
        rejected.into_command().reject(ReceiverError::Protocol(
            "Receiver command queue is full".into(),
        ));
        release_pump.store(true, Ordering::Release);

        let result = reply
            .try_recv()
            .expect("reply channel remains valid")
            .expect("full rejection reply is immediate");
        assert!(matches!(
            result,
            Err(ReceiverError::Protocol(message))
                if message == "Receiver command queue is full"
        ));
    }
}
