//! Windows GpuNative VCam output.
//!
//! The worker owns the renderer, named-pipe server, duplicated target-process
//! handles, and native-surface leases. The Receiver owner only submits the
//! latest native frame and never waits on D3D11 or the Frame Server.

use picoo_frame_hub::{
    NativeVideoFrame, WindowsAdapterId, WindowsNativeChannel, WindowsNativeChannelAck,
    WindowsNativePipeServer, WindowsNativeWireMessage, WindowsSharedSurfaceIdentity,
};
use picoo_gpu::{OutputColor, OutputFormat, RenderSpec, RenderedImage, WindowsRenderer};
use std::os::windows::io::{AsHandle, FromRawHandle, OwnedHandle};
use std::sync::{Arc, Condvar, Mutex};
use std::thread::{self, JoinHandle};
use std::time::Duration;
use windows::Win32::System::Threading::{OpenProcess, PROCESS_DUP_HANDLE};

struct State {
    pending: Option<Arc<NativeVideoFrame>>,
    stopped: bool,
}

pub(crate) struct NativeOutput {
    shared: Arc<(Mutex<State>, Condvar)>,
    server: Arc<WindowsNativePipeServer>,
    worker: Option<JoinHandle<()>>,
}

impl NativeOutput {
    pub(crate) fn start() -> Result<Self, String> {
        let shared = Arc::new((
            Mutex::new(State {
                pending: None,
                stopped: false,
            }),
            Condvar::new(),
        ));
        let server =
            Arc::new(WindowsNativePipeServer::create().map_err(|error| error.to_string())?);
        let worker_shared = Arc::clone(&shared);
        let worker_server = Arc::clone(&server);
        let worker = thread::Builder::new()
            .name("picoo-gpu-native-vcam".into())
            .spawn(move || {
                let _ = run_worker(worker_shared, worker_server);
            })
            .map_err(|error| error.to_string())?;
        Ok(Self {
            shared,
            server,
            worker: Some(worker),
        })
    }

    pub(crate) fn submit(&self, frame: Arc<NativeVideoFrame>) {
        let mut state = self.shared.0.lock().unwrap();
        if !state.stopped {
            state.pending = Some(frame);
            self.shared.1.notify_one();
        }
    }
}

impl Drop for NativeOutput {
    fn drop(&mut self) {
        self.shared.0.lock().unwrap().stopped = true;
        self.shared.1.notify_one();
        self.server.disconnect_client();
        if let Some(worker) = self.worker.take() {
            let _ = worker.join();
        }
    }
}

struct Resources {
    owner: (u64, u64),
    spec: RenderSpec,
    resource_generation: u64,
    renderer: WindowsRenderer,
}

fn prepare(
    resources: &mut Option<Resources>,
    frame: &NativeVideoFrame,
    next_resource_generation: &mut u64,
) -> Result<RenderedImage, String> {
    let description = frame.description();
    let (mut width, mut height) = (
        description.visible_rect.width,
        description.visible_rect.height,
    );
    if matches!(
        description.transform.rotation,
        picoo_frame_hub::Rotation::Clockwise90 | picoo_frame_hub::Rotation::Clockwise270
    ) {
        std::mem::swap(&mut width, &mut height);
    }
    let spec = RenderSpec {
        width,
        height,
        rotation: description.transform.rotation,
        mirror: description.transform.mirror,
        format: OutputFormat::Nv12,
        color: OutputColor::Bt709Limited,
    };
    let identity = frame.identity();
    let owner = (identity.connection_generation, identity.decoder_generation);
    if resources
        .as_ref()
        .is_none_or(|current| current.owner != owner || current.spec != spec)
    {
        let resource_generation = next_generation(next_resource_generation)?;
        *resources = Some(Resources {
            owner,
            spec,
            resource_generation,
            renderer: WindowsRenderer::for_source(frame.image(), spec)
                .map_err(|error| error.to_string())?,
        });
    }
    resources
        .as_mut()
        .expect("native resources initialized")
        .renderer
        .render(frame)
        .map_err(|error| error.to_string())
}

fn next_generation(value: &mut u64) -> Result<u64, String> {
    *value = value
        .checked_add(1)
        .ok_or_else(|| "native output generation exhausted".to_string())?;
    Ok(*value)
}

fn run_worker(
    shared: Arc<(Mutex<State>, Condvar)>,
    server: Arc<WindowsNativePipeServer>,
) -> Result<(), String> {
    let mut next_resource_generation = 0;
    let mut next_backend_generation = 0;
    loop {
        if shared.0.lock().unwrap().stopped {
            return Ok(());
        }
        if let Err(error) = server.accept_local_service() {
            tracing::warn!(%error, "GpuNative VCam peer admission failed; retrying");
            server.disconnect_client();
            thread::sleep(Duration::from_millis(50));
            continue;
        }
        let process_id = match server.client_process_id() {
            Ok(process_id) => process_id,
            Err(error) => {
                tracing::warn!(%error, "GpuNative VCam peer PID query failed; retrying");
                server.disconnect_client();
                continue;
            }
        };
        let process = match unsafe { OpenProcess(PROCESS_DUP_HANDLE, false, process_id) } {
            Ok(process) => process,
            Err(error) => {
                tracing::warn!(%error, "GpuNative VCam target process handle unavailable; retrying");
                server.disconnect_client();
                thread::sleep(Duration::from_millis(50));
                continue;
            }
        };
        let process = unsafe { OwnedHandle::from_raw_handle(process.0 as *mut _) };
        match run_connected(
            Arc::clone(&shared),
            &server,
            process,
            &mut next_resource_generation,
            &mut next_backend_generation,
        ) {
            Ok(()) => return Ok(()),
            Err(error) => {
                server.disconnect_client();
                if shared.0.lock().unwrap().stopped {
                    return Ok(());
                }
                tracing::warn!(%error, "GpuNative VCam connection ended; waiting for reconnect");
            }
        }
    }
}

fn run_connected(
    shared: Arc<(Mutex<State>, Condvar)>,
    server: &WindowsNativePipeServer,
    process: OwnedHandle,
    next_resource_generation: &mut u64,
    next_backend_generation: &mut u64,
) -> Result<(), String> {
    let mut resources = None;
    let mut channel: Option<WindowsNativeChannel> = None;
    let mut channel_key = None;
    let mut backend_generation = None;
    loop {
        let frame = {
            let (lock, ready) = &*shared;
            let mut state = lock.lock().unwrap();
            while state.pending.is_none() && !state.stopped {
                state = ready.wait(state).unwrap();
            }
            if state.stopped {
                return Ok(());
            }
            state.pending.take().expect("pending frame checked")
        };
        let image = prepare(&mut resources, &frame, next_resource_generation)?;
        let identity = frame.identity();
        let resources = resources.as_ref().expect("native resources initialized");
        let adapter = resources.renderer.adapter_id();
        let resource_generation = resources.resource_generation;
        let output_revision = frame.description().config_revision as u64;
        let key = (
            identity.connection_generation,
            identity.stream_epoch,
            identity.decoder_generation,
            output_revision,
            adapter,
        );
        if channel_key != Some(key) {
            if channel.is_some() {
                let _ = server.write_frame(
                    &WindowsNativeWireMessage::Close
                        .encode()
                        .map_err(|error| format!("native close encode: {error:?}"))?,
                );
                return Err("native channel generation changed while connected".into());
            }
            let backend_generation =
                *backend_generation.get_or_insert(next_generation(next_backend_generation)?);
            let mut next = WindowsNativeChannel::new(
                identity.connection_generation,
                identity.stream_epoch,
                adapter,
                resource_generation,
                backend_generation,
                output_revision,
            )
            .map_err(|error| format!("native channel: {error:?}"))?;
            next.begin_handshake()
                .map_err(|error| format!("native handshake: {error:?}"))?;
            let hello = WindowsNativeWireMessage::Hello {
                source_connection_generation: identity.connection_generation,
                stream_epoch: identity.stream_epoch,
                adapter,
                resource_generation,
                backend_generation,
                output_revision,
            };
            server
                .write_frame(
                    &hello
                        .encode()
                        .map_err(|error| format!("native hello encode: {error:?}"))?,
                )
                .map_err(|error| error.to_string())?;
            let ready = WindowsNativeWireMessage::decode(
                &server
                    .read_frame_timeout(Duration::from_secs(1))
                    .map_err(|error| error.to_string())?,
            )
            .map_err(|error| format!("native ready: {error:?}"))?;
            let WindowsNativeWireMessage::Ready {
                source_connection_generation,
                stream_epoch,
                resource_generation,
                backend_generation,
                output_revision,
            } = ready
            else {
                return Err("native peer did not send Ready".into());
            };
            next.accept_ready(
                source_connection_generation,
                stream_epoch,
                resource_generation,
                backend_generation,
                output_revision,
            )
            .map_err(|error| format!("native ready rejected: {error:?}"))?;
            channel = Some(next);
            channel_key = Some(key);
        }
        let native_identity = WindowsSharedSurfaceIdentity {
            source_connection_generation: identity.connection_generation,
            stream_epoch: identity.stream_epoch,
            decoder_generation: identity.decoder_generation,
            source_frame_id: identity.frame_id,
            resource_generation,
            backend_generation: backend_generation.expect("native channel initialized"),
            output_revision,
        };
        let transfer = unsafe {
            image
                .duplicate_shared_handle_into(
                    process.as_handle(),
                    native_identity,
                    picoo_frame_hub::WindowsSharedSurfaceFormat::Nv12,
                )
                .map_err(|error| error.to_string())?
        };
        let descriptor = *transfer
            .descriptor()
            .ok_or_else(|| "native transfer missing descriptor".to_string())?;
        let offer_id = channel
            .as_mut()
            .expect("channel initialized")
            .offer_frame(descriptor)
            .map_err(|error| format!("native offer: {error:?}"))?;
        server
            .write_frame(
                &WindowsNativeWireMessage::Offer {
                    offer_id,
                    descriptor,
                }
                .encode()
                .map_err(|error| format!("native offer encode: {error:?}"))?,
            )
            .map_err(|error| error.to_string())?;
        let ack = WindowsNativeWireMessage::decode(
            &server
                .read_frame_timeout(Duration::from_secs(1))
                .map_err(|error| error.to_string())?,
        )
        .map_err(|error| format!("native imported ack: {error:?}"))?;
        match ack {
            WindowsNativeWireMessage::Ack {
                offer_id: ack_id,
                ack: WindowsNativeChannelAck::Imported,
            } if ack_id == offer_id => {
                channel
                    .as_mut()
                    .expect("channel initialized")
                    .acknowledge(offer_id, WindowsNativeChannelAck::Imported)
                    .map_err(|error| format!("native import ack: {error:?}"))?;
            }
            _ => return Err("native peer rejected surface".into()),
        }
        let release = WindowsNativeWireMessage::decode(
            &server
                .read_frame_timeout(Duration::from_secs(1))
                .map_err(|error| error.to_string())?,
        )
        .map_err(|error| format!("native release ack: {error:?}"))?;
        if !matches!(
            release,
            WindowsNativeWireMessage::Ack {
                offer_id: ack_id,
                ack: WindowsNativeChannelAck::Released,
            } if ack_id == offer_id
        ) {
            return Err("native peer did not release surface".into());
        }
        channel
            .as_mut()
            .expect("channel initialized")
            .acknowledge(offer_id, WindowsNativeChannelAck::Released)
            .map_err(|error| format!("native release ack: {error:?}"))?;
        drop(transfer.commit());
    }
}

#[cfg(test)]
mod tests {
    use super::next_generation;

    #[test]
    fn native_generations_are_monotonic_and_checked() {
        let mut generation = 0;
        assert_eq!(next_generation(&mut generation), Ok(1));
        assert_eq!(next_generation(&mut generation), Ok(2));

        let mut exhausted = u64::MAX;
        assert_eq!(
            next_generation(&mut exhausted),
            Err("native output generation exhausted".to_string())
        );
        assert_eq!(exhausted, u64::MAX);
    }
}
