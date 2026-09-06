//! macOS VideoToolbox AVC/HEVC decoder — REQ-PICOO-MEDIA-012.
//!
//! The Apple production path is pure Rust over generated framework bindings:
//! Annex-B/AVCC access unit → CoreMedia sample → VideoToolbox →
//! retained native NV12 for FrameBus. OpenH264 is intentionally not linked on Apple targets.

#[cfg(test)]
use crate::DecodeFixture as _;
use std::ffi::c_void;
use std::ptr::{self, NonNull};
use std::sync::Mutex;

use objc2_core_foundation::{CFBoolean, CFDictionary, CFNumber, CFRetained, CFString, CFType};
use objc2_core_media::{CMBlockBuffer, CMFormatDescription, CMSampleBuffer};
use objc2_core_video::{
    kCVPixelBufferIOSurfacePropertiesKey, kCVPixelBufferMetalCompatibilityKey,
    kCVPixelBufferPixelFormatTypeKey, kCVPixelFormatType_420YpCbCr8BiPlanarVideoRange,
    CVImageBuffer,
};
use objc2_video_toolbox::{
    kVTVideoDecoderSpecification_RequireHardwareAcceleratedVideoDecoder, VTDecodeFrameFlags,
    VTDecodeInfoFlags, VTDecompressionOutputCallbackRecord, VTDecompressionSession,
};
use picoo_bitstream::{AccessUnit, Codec, CodecConfiguration, PictureKind, RandomAccessPoint};

mod format;
use picoo_frame_hub::{ApplePixelBufferLease, NativeImage};
use picoo_protocol::control::StreamConfig;

use crate::{now_timestamp_us, AccessUnitDecoder, DecodeError, DecodeOutcome, DecodedFrame};

type DecodeOutput = Result<Option<NativeImage>, DecodeError>;

#[derive(Default)]
struct OutputContext {
    result: Mutex<Option<DecodeOutput>>,
}

pub struct VideoToolboxDecoder {
    // Drop the session before the callback context whose address it stores.
    session: Option<CFRetained<VTDecompressionSession>>,
    format_description: Option<CFRetained<CMFormatDescription>>,
    output: Box<OutputContext>,
    configuration: Option<CodecConfiguration>,
}

impl VideoToolboxDecoder {
    pub fn new() -> Self {
        Self {
            session: None,
            format_description: None,
            output: Box::default(),
            configuration: None,
        }
    }

    fn reset_session(&mut self) {
        if let Some(session) = self.session.take() {
            // SAFETY: The retained session is valid and exclusively owned by
            // this decoder while ReceiverSession invokes it serially.
            unsafe { session.invalidate() };
        }
        self.format_description = None;
        if let Ok(mut result) = self.output.result.lock() {
            *result = None;
        }
    }

    fn ensure_session(&mut self, configuration: &CodecConfiguration) -> Result<(), DecodeError> {
        if self.session.is_some()
            && self.configuration.as_ref().is_some_and(|current| {
                current.codec() == configuration.codec()
                    && current.record() == configuration.record()
            })
        {
            return Ok(());
        }

        let format_description = format::create(configuration)?;

        let require_hardware = CFBoolean::new(true);
        let decoder_specification = CFDictionary::from_slices(
            &[unsafe { kVTVideoDecoderSpecification_RequireHardwareAcceleratedVideoDecoder }],
            &[require_hardware],
        );
        let nv12_format = CFNumber::new_i64(kCVPixelFormatType_420YpCbCr8BiPlanarVideoRange as i64);
        let surface = CFDictionary::<CFString, CFType>::empty();
        let metal = CFBoolean::new(true);
        let image_attributes = CFDictionary::<CFString, CFType>::from_slices(
            &unsafe {
                [
                    kCVPixelBufferPixelFormatTypeKey,
                    kCVPixelBufferIOSurfacePropertiesKey,
                    kCVPixelBufferMetalCompatibilityKey,
                ]
            },
            &[nv12_format.as_ref(), surface.as_ref(), metal.as_ref()],
        );
        let callback = VTDecompressionOutputCallbackRecord {
            decompressionOutputCallback: Some(decompression_output_callback),
            decompressionOutputRefCon: (&*self.output as *const OutputContext)
                .cast_mut()
                .cast::<c_void>(),
        };
        let mut raw_session: *mut VTDecompressionSession = ptr::null_mut();

        // SAFETY: All dictionaries contain the CoreFoundation value types
        // required by VideoToolbox. The callback context is boxed and remains
        // stable until after the retained session is invalidated and dropped.
        let status = unsafe {
            VTDecompressionSession::create(
                None,
                &format_description,
                Some(decoder_specification.as_opaque()),
                Some(image_attributes.as_opaque()),
                &callback,
                NonNull::from(&mut raw_session),
            )
        };
        check_status("VTDecompressionSessionCreate", status)?;
        let raw_session = NonNull::new(raw_session)
            .ok_or_else(|| DecodeError::Platform("VideoToolbox returned a null session".into()))?;
        // SAFETY: A successful create returns ownership of a +1 CF object.
        let session = unsafe { CFRetained::from_raw(raw_session) };

        self.reset_session();
        self.configuration = Some(configuration.clone());
        self.format_description = Some(format_description);
        self.session = Some(session);
        Ok(())
    }

    fn decode_real_access_unit(
        &mut self,
        access_unit: &[u8],
        picture: AccessUnit<'_>,
        stream_config: Option<&StreamConfig>,
        token: std::sync::Arc<crate::DecodeToken>,
    ) -> Result<DecodeOutcome, DecodeError> {
        // CRA recovery requires explicit leading-picture ownership. This backend
        // admits closed IDR sequences until that separate contract is verified.
        if matches!(
            picture.picture().kind,
            PictureKind::RandomAccess(RandomAccessPoint::HevcCra)
                | PictureKind::HevcRasl
                | PictureKind::HevcRadl
        ) {
            return Err(DecodeError::UnsupportedAccessUnit);
        }
        let contains_refresh = matches!(
            picture.picture().kind,
            PictureKind::RandomAccess(RandomAccessPoint::AvcIdr | RandomAccessPoint::HevcIdr)
        );
        let configuration = format::configuration(stream_config, &picture)?;
        let facts = format::source_facts(&configuration)?;
        crate::native_format::validate_source(&facts)?;
        if stream_config.is_some_and(|config| {
            (config.width, config.height) != (facts.visible_width, facts.visible_height)
        }) {
            return Err(DecodeError::ConfigurationMismatch);
        }
        self.ensure_session(&configuration)?;
        let sample = create_sample_buffer(access_unit, self.format_description.as_deref())?;

        {
            let mut result =
                self.output.result.lock().map_err(|_| {
                    DecodeError::Platform("VideoToolbox output lock poisoned".into())
                })?;
            *result = None;
        }

        let mut info_flags = VTDecodeInfoFlags::empty();
        let session = self.session.as_deref().ok_or(DecodeError::NotInitialized)?;
        // Empty flags make this decode synchronous: Apple guarantees the
        // callback completes before the function returns.
        // SAFETY: `sample` and `session` are retained for the call and the
        // callback record was installed with a stable boxed context.
        let status = unsafe {
            session.decode_frame(
                &sample,
                VTDecodeFrameFlags::empty(),
                ptr::null_mut(),
                &mut info_flags,
            )
        };
        check_status("VTDecompressionSessionDecodeFrame", status)?;
        if info_flags.contains(VTDecodeInfoFlags::FrameDropped) {
            return Ok(DecodeOutcome::accepted_without_frame(false));
        }

        let output = self
            .output
            .result
            .lock()
            .map_err(|_| DecodeError::Platform("VideoToolbox output lock poisoned".into()))?
            .take()
            .unwrap_or(Ok(None))?;
        let Some(output) = output else {
            return Ok(DecodeOutcome::accepted_without_frame(contains_refresh));
        };
        let native_format = crate::native_format::describe(&facts, &output)?;
        Ok(DecodeOutcome::frame(
            token,
            DecodedFrame::native(
                output,
                native_format,
                stream_config.map(|config| config.rotation).unwrap_or(0),
                now_timestamp_us(),
            ),
            contains_refresh,
        ))
    }
}

impl Default for VideoToolboxDecoder {
    fn default() -> Self {
        Self::new()
    }
}

impl Drop for VideoToolboxDecoder {
    fn drop(&mut self) {
        self.reset_session();
    }
}

impl AccessUnitDecoder for VideoToolboxDecoder {
    fn submit(
        &mut self,
        submission: crate::DecodeSubmission<'_>,
    ) -> Result<DecodeOutcome, DecodeError> {
        let access_unit = submission.access_unit;
        let stream_config = submission.token.stream_config.as_deref();

        let codec = stream_config
            .map(crate::configured_picture::configured_codec)
            .transpose()?
            .unwrap_or(Codec::Avc);
        let picture = crate::configured_picture::validate(codec, access_unit, stream_config)?;
        self.decode_real_access_unit(
            access_unit,
            picture,
            stream_config,
            submission.token.clone(),
        )
    }

    fn reset(&mut self) -> Result<(), DecodeError> {
        self.reset_session();
        self.configuration = None;
        Ok(())
    }
}

fn check_status(operation: &str, status: i32) -> Result<(), DecodeError> {
    if status == 0 {
        Ok(())
    } else {
        Err(DecodeError::Platform(format!(
            "{operation} failed with OSStatus {status}"
        )))
    }
}

fn create_sample_buffer(
    avcc: &[u8],
    format_description: Option<&CMFormatDescription>,
) -> Result<CFRetained<CMSampleBuffer>, DecodeError> {
    let format_description = format_description.ok_or(DecodeError::NotInitialized)?;
    let mut raw_block: *mut CMBlockBuffer = ptr::null_mut();
    // SAFETY: Passing a null memory block asks CoreMedia to allocate `len`
    // bytes with the default allocator; output storage is valid.
    let status = unsafe {
        CMBlockBuffer::create_with_memory_block(
            None,
            ptr::null_mut(),
            avcc.len(),
            None,
            ptr::null(),
            0,
            avcc.len(),
            0,
            NonNull::from(&mut raw_block),
        )
    };
    check_status("CMBlockBufferCreateWithMemoryBlock", status)?;
    let raw_block = NonNull::new(raw_block)
        .ok_or_else(|| DecodeError::Platform("CoreMedia returned a null block buffer".into()))?;
    // SAFETY: A successful create returns ownership of a +1 CF object.
    let block = unsafe { CFRetained::from_raw(raw_block) };
    let source = NonNull::new(avcc.as_ptr().cast_mut().cast::<c_void>())
        .ok_or(DecodeError::UnsupportedAccessUnit)?;
    // SAFETY: `source` references `avcc.len()` initialized bytes and CoreMedia
    // owns an equally sized destination block.
    let status = unsafe { CMBlockBuffer::replace_data_bytes(source, &block, 0, avcc.len()) };
    check_status("CMBlockBufferReplaceDataBytes", status)?;

    let mut raw_sample: *mut CMSampleBuffer = ptr::null_mut();
    let sample_size = avcc.len();
    // SAFETY: The retained block and format description outlive sample
    // creation; one sample and one matching size entry are provided.
    let status = unsafe {
        CMSampleBuffer::create_ready(
            None,
            Some(&block),
            Some(format_description),
            1,
            0,
            ptr::null(),
            1,
            &sample_size,
            NonNull::from(&mut raw_sample),
        )
    };
    check_status("CMSampleBufferCreateReady", status)?;
    let raw_sample = NonNull::new(raw_sample)
        .ok_or_else(|| DecodeError::Platform("CoreMedia returned a null sample buffer".into()))?;
    // SAFETY: A successful create returns ownership of a +1 CF object.
    Ok(unsafe { CFRetained::from_raw(raw_sample) })
}

unsafe extern "C-unwind" fn decompression_output_callback(
    output_refcon: *mut c_void,
    _source_frame_refcon: *mut c_void,
    status: i32,
    info_flags: VTDecodeInfoFlags,
    image_buffer: *mut CVImageBuffer,
    _presentation_timestamp: objc2_core_media::CMTime,
    _presentation_duration: objc2_core_media::CMTime,
) {
    let Some(context) = NonNull::new(output_refcon.cast::<OutputContext>()) else {
        return;
    };
    let result = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
        check_status("VideoToolbox output callback", status)?;
        if info_flags.contains(VTDecodeInfoFlags::FrameDropped) {
            return Ok(None);
        }
        let image_buffer = NonNull::new(image_buffer)
            .ok_or_else(|| DecodeError::Platform("VideoToolbox returned no image buffer".into()))?;
        // SAFETY: VideoToolbox guarantees the image buffer remains valid for
        // the duration of this callback.
        unsafe { ApplePixelBufferLease::retain_completed(image_buffer.as_ref()) }
            .map(|image| Some(NativeImage::Apple(image)))
            .map_err(|error| DecodeError::Platform(error.to_string()))
    }))
    .unwrap_or_else(|_| {
        Err(DecodeError::Platform(
            "panic while retaining VideoToolbox output".into(),
        ))
    });
    // SAFETY: `output_refcon` points to the boxed OutputContext retained by the
    // decoder until after session invalidation.
    if let Ok(mut slot) = unsafe { context.as_ref() }.result.lock() {
        *slot = Some(result);
    }
}

#[cfg(test)]
mod tests;
