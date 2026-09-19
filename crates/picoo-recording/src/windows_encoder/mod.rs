//! D3D11 NV12 target -> hardware asynchronous encoder MFT — REQ-PICOO-MEDIA-084.
//! The rendered-recording worker owns this component and its COM apartment.

mod factory;
mod output;

pub use output::EncodedFrame;

use crate::RecordingError;
use picoo_bitstream::Codec;
use picoo_gpu::{OutputColor, OutputFormat, RenderedImage};
use std::{
    marker::PhantomData,
    mem::ManuallyDrop,
    rc::Rc,
    time::{Duration, Instant},
};
use windows::{
    core::Interface,
    Win32::{
        Graphics::Direct3D11::{ID3D11Device, ID3D11Texture2D},
        Graphics::Dxgi::Common::DXGI_FORMAT_NV12,
        Media::MediaFoundation::*,
        System::{
            Com::{CoInitializeEx, CoUninitialize, COINIT_MULTITHREADED},
            Variant::VARIANT,
        },
    },
};

const ENCODE_DEADLINE: Duration = Duration::from_millis(250);

struct Runtime(PhantomData<Rc<()>>);

impl Runtime {
    fn new() -> Result<Self, RecordingError> {
        unsafe {
            platform(CoInitializeEx(None, COINIT_MULTITHREADED).ok())?;
            if let Err(error) = MFStartup(MF_VERSION, MFSTARTUP_FULL) {
                CoUninitialize();
                return Err(native(error));
            }
        }
        Ok(Self(PhantomData))
    }
}

impl Drop for Runtime {
    fn drop(&mut self) {
        unsafe {
            let _ = MFShutdown();
            CoUninitialize();
        }
    }
}

/// Hardware-only, one-input-at-a-time encoder. It is deliberately !Send: all
/// MFT, manager and shutdown operations stay on the rendered-recording worker.
pub struct WindowsEncoder {
    activation: Option<IMFActivate>,
    transform: Option<IMFTransform>,
    events: IMFMediaEventGenerator,
    codec_api: ICodecAPI,
    _manager: IMFDXGIDeviceManager,
    device: ID3D11Device,
    format: factory::EncoderFormat,
    input_credits: u32,
    last_pts: Option<u64>,
    failed: bool,
    // If an accepted native input times out, keep the pool lease until MFT
    // shutdown rather than allowing the renderer to overwrite that texture.
    retained_failed_input: Option<RenderedImage>,
    // Last: every native interface is released before MF/COM shutdown.
    _runtime: Runtime,
}

impl WindowsEncoder {
    pub fn new(
        image: &RenderedImage,
        codec: Codec,
        width: u32,
        height: u32,
        fps: u32,
        bitrate: u32,
    ) -> Result<Self, RecordingError> {
        let format = factory::EncoderFormat::new(codec, width, height, fps, bitrate)?;
        validate_image(image, width, height)?;
        let runtime = Runtime::new()?;
        let device = unsafe { image.texture().GetDevice().map_err(native)? };
        let selected = factory::create(&device, format)?;
        Ok(Self {
            activation: Some(selected.activation),
            transform: Some(selected.transform),
            events: selected.events,
            codec_api: selected.codec_api,
            _manager: selected.manager,
            device,
            format,
            input_credits: 0,
            last_pts: None,
            failed: false,
            retained_failed_input: None,
            _runtime: runtime,
        })
    }

    pub fn encode(
        &mut self,
        image: RenderedImage,
        pts_us: u64,
        force_idr: bool,
    ) -> Result<EncodedFrame, RecordingError> {
        if self.failed
            || self.last_pts.is_some_and(|previous| pts_us <= previous)
            || validate_image(&image, self.format.width, self.format.height).is_err()
        {
            return Err(RecordingError::InvalidInput(
                "encoder input contract mismatch",
            ));
        }
        let same_device = match same_device(&image, &self.device) {
            Ok(same_device) => same_device,
            Err(error) => {
                self.failed = true;
                return Err(error);
            }
        };
        if !same_device {
            return Err(RecordingError::InvalidInput(
                "encoder input contract mismatch",
            ));
        }
        let deadline = Instant::now() + ENCODE_DEADLINE;
        let mut accepted = false;
        let result = (|| {
            wait_for_input(&self.events, &mut self.input_credits, deadline)?;
            set_codec_value(
                &self.codec_api,
                &CODECAPI_AVEncVideoForceKeyFrame,
                VARIANT::from(u32::from(force_idr)),
            )?;
            let sample = input_sample(&image, pts_us, self.format.fps)?;
            let transform = self.transform.as_ref().expect("encoder transform");
            unsafe { transform.ProcessInput(0, &sample, 0) }.map_err(native)?;
            accepted = true;
            wait_for_output(&self.events, &mut self.input_credits, deadline)?;
            let sample = take_output(transform)?;
            let frame = output::read(
                transform,
                &sample,
                self.format.codec,
                self.format.width,
                self.format.height,
                pts_us,
                force_idr,
            )?;
            if Instant::now() >= deadline {
                Err(timeout())
            } else {
                Ok(frame)
            }
        })();
        if result.is_err() {
            self.failed = true;
            if accepted {
                self.retained_failed_input = Some(image);
            }
        } else {
            self.last_pts = Some(pts_us);
        }
        result
    }
}

impl Drop for WindowsEncoder {
    fn drop(&mut self) {
        let mut shutdown_failed = false;
        unsafe {
            if let Some(transform) = &self.transform {
                let _ = transform.ProcessMessage(MFT_MESSAGE_NOTIFY_END_OF_STREAM, 0);
                let _ = transform.ProcessMessage(MFT_MESSAGE_NOTIFY_END_STREAMING, 0);
            }
            if let Some(activation) = &self.activation {
                shutdown_failed = activation.ShutdownObject().is_err();
            }
            drop(self.transform.take());
            drop(self.activation.take());
        }
        // Field destruction releases the event/codec/manager interfaces before
        // this image. If native shutdown itself failed, keep the pool lease for
        // the rest of the process rather than risk overwriting an in-flight GPU
        // read owned by an object whose quiescence could not be established.
        if shutdown_failed {
            if let Some(image) = self.retained_failed_input.take() {
                std::mem::forget(image);
            }
        }
    }
}

fn validate_image(image: &RenderedImage, width: u32, height: u32) -> Result<(), RecordingError> {
    let spec = image.spec();
    if spec.width != width
        || spec.height != height
        || spec.color != OutputColor::Bt709Limited
        || spec.format != OutputFormat::Nv12
    {
        return Err(RecordingError::InvalidInput(
            "encoder input contract mismatch",
        ));
    }
    unsafe {
        let mut description = Default::default();
        image.texture().GetDesc(&mut description);
        if description.Width != width
            || description.Height != height
            || description.Format != DXGI_FORMAT_NV12
            || description.ArraySize != 1
            || description.MipLevels != 1
        {
            return Err(RecordingError::InvalidInput(
                "encoder target texture mismatch",
            ));
        }
    }
    Ok(())
}

fn same_device(image: &RenderedImage, expected: &ID3D11Device) -> Result<bool, RecordingError> {
    unsafe {
        Ok(image
            .texture()
            .GetDevice()
            .map_err(native)?
            .cast::<windows::core::IUnknown>()
            .map_err(native)?
            == expected.cast::<windows::core::IUnknown>().map_err(native)?)
    }
}

fn input_sample(image: &RenderedImage, pts_us: u64, fps: u32) -> Result<IMFSample, RecordingError> {
    let timestamp = timestamp(pts_us)?;
    unsafe {
        let sample = platform(MFCreateSample())?;
        let surface = platform(MFCreateDXGISurfaceBuffer(
            &ID3D11Texture2D::IID,
            image.texture(),
            0,
            false,
        ))?;
        platform(sample.AddBuffer(&surface))?;
        platform(sample.SetSampleTime(timestamp))?;
        platform(sample.SetSampleDuration(10_000_000 / i64::from(fps)))?;
        Ok(sample)
    }
}

fn take_output(transform: &IMFTransform) -> Result<IMFSample, RecordingError> {
    unsafe {
        let info = platform(transform.GetOutputStreamInfo(0))?;
        let provides = info.dwFlags & MFT_OUTPUT_STREAM_PROVIDES_SAMPLES.0 as u32 != 0;
        let sample = if provides {
            None
        } else {
            if info.cbSize == 0 || info.cbSize > 2 * 1024 * 1024 {
                return Err(RecordingError::InvalidInput(
                    "invalid encoder output allocation",
                ));
            }
            let sample = platform(MFCreateSample())?;
            let buffer = platform(MFCreateAlignedMemoryBuffer(info.cbSize, info.cbAlignment))?;
            platform(sample.AddBuffer(&buffer))?;
            Some(sample)
        };
        let mut output = MFT_OUTPUT_DATA_BUFFER {
            dwStreamID: 0,
            pSample: ManuallyDrop::new(sample),
            dwStatus: 0,
            pEvents: ManuallyDrop::new(None),
        };
        let mut status = 0;
        let processed = transform.ProcessOutput(0, std::slice::from_mut(&mut output), &mut status);
        let sample = ManuallyDrop::take(&mut output.pSample);
        drop(ManuallyDrop::take(&mut output.pEvents));
        platform(processed)?;
        if status != 0 || output.dwStatus != 0 {
            return Err(RecordingError::InvalidInput(
                "encoder returned unsupported output status",
            ));
        }
        sample.ok_or(RecordingError::InvalidInput(
            "encoder returned no output sample",
        ))
    }
}

fn wait_for_input(
    events: &IMFMediaEventGenerator,
    input_credits: &mut u32,
    deadline: Instant,
) -> Result<(), RecordingError> {
    if *input_credits != 0 {
        *input_credits -= 1;
        return Ok(());
    }
    if next_event(events, deadline)? == METransformNeedInput.0 as u32 {
        Ok(())
    } else {
        Err(unexpected_event())
    }
}

fn wait_for_output(
    events: &IMFMediaEventGenerator,
    input_credits: &mut u32,
    deadline: Instant,
) -> Result<(), RecordingError> {
    loop {
        match next_event(events, deadline)? {
            kind if kind == METransformHaveOutput.0 as u32 => return Ok(()),
            kind if kind == METransformNeedInput.0 as u32 => {
                *input_credits = input_credits.checked_add(1).ok_or_else(unexpected_event)?;
            }
            _ => return Err(unexpected_event()),
        }
    }
}

fn next_event(events: &IMFMediaEventGenerator, deadline: Instant) -> Result<u32, RecordingError> {
    loop {
        if Instant::now() >= deadline {
            return Err(timeout());
        }
        match unsafe { events.GetEvent(MF_EVENT_FLAG_NO_WAIT) } {
            Ok(event) => unsafe {
                let status = platform(event.GetStatus())?;
                platform(status.ok())?;
                return platform(event.GetType());
            },
            Err(error) if error.code() == MF_E_NO_EVENTS_AVAILABLE => {
                if Instant::now() >= deadline {
                    return Err(timeout());
                }
                std::thread::sleep(Duration::from_millis(1));
            }
            Err(error) => return Err(native(error)),
        }
    }
}

fn unexpected_event() -> RecordingError {
    RecordingError::InvalidInput("hardware encoder returned an unexpected event")
}

fn set_codec_value(
    codec: &ICodecAPI,
    key: &windows::core::GUID,
    value: VARIANT,
) -> Result<(), RecordingError> {
    unsafe {
        platform(codec.IsSupported(key))?;
        platform(codec.SetValue(key, &value))
    }
}

fn timestamp(pts_us: u64) -> Result<i64, RecordingError> {
    pts_us
        .checked_mul(10)
        .and_then(|value| i64::try_from(value).ok())
        .ok_or(RecordingError::InvalidInput("encoder timestamp overflow"))
}

fn timeout() -> RecordingError {
    RecordingError::Platform("hardware encoder event deadline expired".into())
}

fn native(error: windows::core::Error) -> RecordingError {
    RecordingError::Platform(error.to_string())
}

fn platform<T>(result: windows::core::Result<T>) -> Result<T, RecordingError> {
    result.map_err(native)
}
