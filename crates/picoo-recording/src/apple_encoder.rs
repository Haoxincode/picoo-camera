//! GPU target -> explicitly required hardware encoder — REQ-PICOO-MEDIA-079.
//! A synchronous component for the recording worker; never called by Receiver/UI.
use crate::RecordingError;
use block2::RcBlock;
use objc2_core_foundation::{CFBoolean, CFDictionary, CFNumber, CFRetained, CFString, CFType};
use objc2_core_media::{CMSampleBuffer, CMTime, CMTimeFlags};
use objc2_core_video::*;
use objc2_video_toolbox::*;
use picoo_bitstream::Codec;
use picoo_gpu::{OutputColor, OutputFormat, RenderedImage};
use std::{
    ptr::{self, NonNull},
    sync::mpsc,
    time::{Duration, Instant},
};

mod output;
#[cfg(test)]
mod tests;
pub use output::EncodedFrame;

pub struct AppleEncoder {
    session: CFRetained<VTCompressionSession>,
    codec: Codec,
    width: u32,
    height: u32,
    fps: u32,
    last_pts: Option<u64>,
    failed: bool,
}

impl AppleEncoder {
    pub fn new(
        codec: Codec,
        width: u32,
        height: u32,
        fps: u32,
        bitrate: u32,
    ) -> Result<Self, RecordingError> {
        if !matches!((width, height), (1280, 720) | (1920, 1080))
            || !matches!(fps, 30 | 60)
            || bitrate == 0
            || bitrate > 100_000_000
        {
            return Err(RecordingError::InvalidInput("unsupported encoder format"));
        }
        // SAFETY: Documented CF property types. The session is created, used,
        // invalidated and released on this worker; no raw callback context.
        unsafe {
            let specification = CFDictionary::<CFString, CFType>::from_slices(
                &[kVTVideoEncoderSpecification_RequireHardwareAcceleratedVideoEncoder],
                &[CFBoolean::new(true).as_ref()],
            );
            let mut raw = ptr::null_mut();
            status(
                VTCompressionSession::create(
                    None,
                    width as i32,
                    height as i32,
                    match codec {
                        Codec::Avc => u32::from_be_bytes(*b"avc1"),
                        Codec::Hevc => u32::from_be_bytes(*b"hvc1"),
                    },
                    Some(specification.as_opaque()),
                    None,
                    None,
                    None,
                    ptr::null_mut(),
                    NonNull::from(&mut raw),
                ),
                "create hardware encoder",
            )?;
            let session = CFRetained::from_raw(
                NonNull::new(raw).ok_or(RecordingError::InvalidInput("missing encoder session"))?,
            );
            let encoder = Self {
                session,
                codec,
                width,
                height,
                fps,
                last_pts: None,
                failed: false,
            };
            encoder.configure(bitrate)?;
            Ok(encoder)
        }
    }

    unsafe fn configure(&self, bitrate: u32) -> Result<(), RecordingError> {
        unsafe {
            self.set(kVTCompressionPropertyKey_RealTime, CFBoolean::new(true))?;
            self.set(
                kVTCompressionPropertyKey_AllowFrameReordering,
                CFBoolean::new(false),
            )?;
            self.set(
                kVTCompressionPropertyKey_ExpectedFrameRate,
                &CFNumber::new_i32(self.fps as i32),
            )?;
            self.set(
                kVTCompressionPropertyKey_AverageBitRate,
                &CFNumber::new_i32(bitrate as i32),
            )?;
            self.set(
                kVTCompressionPropertyKey_MaxKeyFrameInterval,
                &CFNumber::new_i32(self.fps as i32 * 2),
            )?;
            self.set(
                kVTCompressionPropertyKey_ProfileLevel,
                match self.codec {
                    Codec::Avc => kVTProfileLevel_H264_High_AutoLevel,
                    Codec::Hevc => kVTProfileLevel_HEVC_Main_AutoLevel,
                },
            )?;
            if self.codec == Codec::Hevc {
                self.set(
                    kVTCompressionPropertyKey_AllowOpenGOP,
                    CFBoolean::new(false),
                )?;
            }
            for (key, value) in [
                (
                    kVTCompressionPropertyKey_ColorPrimaries,
                    kCVImageBufferColorPrimaries_ITU_R_709_2,
                ),
                (
                    kVTCompressionPropertyKey_TransferFunction,
                    kCVImageBufferTransferFunction_ITU_R_709_2,
                ),
                (
                    kVTCompressionPropertyKey_YCbCrMatrix,
                    kCVImageBufferYCbCrMatrix_ITU_R_709_2,
                ),
            ] {
                self.set(key, value)?;
            }
            status(self.session.prepare_to_encode_frames(), "prepare encoder")?;
            let mut hardware: *const CFType = ptr::null();
            status(
                VTSessionCopyProperty(
                    &self.session,
                    kVTCompressionPropertyKey_UsingHardwareAcceleratedVideoEncoder,
                    None,
                    (&mut hardware as *mut *const CFType).cast(),
                ),
                "query actual encoder",
            )?;
            let hardware = CFRetained::<CFType>::from_raw(
                NonNull::new(hardware.cast_mut())
                    .ok_or(RecordingError::InvalidInput("missing hardware evidence"))?,
            );
            if hardware
                .downcast_ref::<CFBoolean>()
                .is_none_or(|value| !value.as_bool())
            {
                return Err(RecordingError::InvalidInput(
                    "encoder is not hardware accelerated",
                ));
            }
            Ok(())
        }
    }

    unsafe fn set(&self, key: &CFString, value: &CFType) -> Result<(), RecordingError> {
        status(
            unsafe { VTSessionSetProperty(&self.session, key, Some(value)) },
            "set encoder property",
        )
    }

    /// One in-flight input. The callback owns a target lease even if the wait
    /// expires. A timeout/driver error poisons this instance; Drop invalidates it.
    pub fn encode(
        &mut self,
        image: RenderedImage,
        pts_us: u64,
        force_idr: bool,
    ) -> Result<EncodedFrame, RecordingError> {
        let spec = image.spec();
        if self.failed
            || spec.width != self.width
            || spec.height != self.height
            || spec.color != OutputColor::Bt709Limited
            || spec.format != OutputFormat::Nv12
            || self.last_pts.is_some_and(|previous| pts_us <= previous)
        {
            return Err(RecordingError::InvalidInput(
                "encoder input contract mismatch",
            ));
        }
        let time = time(pts_us)?;
        let (send, receive) = mpsc::sync_channel(1);
        let retained = image.clone();
        let codec = self.codec;
        let callback = RcBlock::new(
            move |code: i32, flags: VTEncodeInfoFlags, sample: *mut CMSampleBuffer| {
                // Capturing the completed target keeps its pool slot leased until
                // the callback is done; merely timing out cannot recycle the slot.
                let _lease = &retained;
                let result = status(code, "encode callback").and_then(|()| {
                    if flags.contains(VTEncodeInfoFlags::FrameDropped) {
                        return Err(RecordingError::InvalidInput(
                            "hardware encoder dropped frame",
                        ));
                    }
                    unsafe { output::copy(sample, codec, pts_us) }
                });
                let _ = send.try_send(result);
            },
        );
        // SAFETY: Read-only completed target; callback retains another owner.
        // VT copies the block for asynchronous use. Configuration is immutable.
        let submitted = Instant::now();
        let code = unsafe {
            let properties = CFDictionary::<CFString, CFType>::from_slices(
                &[kVTEncodeFrameOptionKey_ForceKeyFrame],
                &[CFBoolean::new(force_idr).as_ref()],
            );
            self.session.encode_frame_with_output_handler(
                image.pixel_buffer(),
                time,
                CMTime {
                    value: 1,
                    timescale: self.fps as i32,
                    flags: CMTimeFlags::Valid,
                    epoch: 0,
                },
                Some(properties.as_opaque()),
                ptr::null_mut(),
                RcBlock::as_ptr(&callback),
            )
        };
        let timeout =
            || RecordingError::Platform("hardware encoder output deadline expired".into());
        let result = status(code, "submit encoder input")
            .and_then(|()| {
                let remaining = Duration::from_millis(250)
                    .checked_sub(submitted.elapsed())
                    .ok_or_else(timeout)?;
                receive.recv_timeout(remaining).map_err(|_| timeout())?
            })
            .and_then(|frame| {
                if submitted.elapsed() >= Duration::from_millis(250) {
                    return Err(timeout());
                }
                output::validate(&frame, self.width, self.height, force_idr)?;
                Ok(frame)
            });
        self.failed = result.is_err();
        if result.is_ok() {
            self.last_pts = Some(pts_us);
        }
        result
    }
}

impl Drop for AppleEncoder {
    fn drop(&mut self) {
        // SAFETY: The recording worker exclusively owns this session. Pending
        // callbacks retain their input independently until native use ends.
        unsafe {
            self.session.invalidate();
        }
    }
}

fn status(code: i32, operation: &str) -> Result<(), RecordingError> {
    if code == 0 {
        Ok(())
    } else {
        Err(RecordingError::Platform(format!(
            "{operation}: OSStatus {code}"
        )))
    }
}

fn time(pts: u64) -> Result<CMTime, RecordingError> {
    Ok(CMTime {
        value: i64::try_from(pts)
            .map_err(|_| RecordingError::InvalidInput("encoder PTS overflow"))?,
        timescale: 1_000_000,
        flags: CMTimeFlags::Valid,
        epoch: 0,
    })
}
