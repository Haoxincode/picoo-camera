//! macOS VideoToolbox H.264 decoder — REQ-PICOO-MEDIA-012.
//!
//! The Apple production path is pure Rust over generated framework bindings:
//! Annex-B/AVCC access unit → CoreMedia sample → VideoToolbox →
//! retained native NV12 for FrameBus. OpenH264 is intentionally not linked on Apple targets.

use std::ffi::c_void;
use std::ptr::{self, NonNull};
use std::sync::Mutex;

use objc2_core_foundation::{CFBoolean, CFDictionary, CFNumber, CFRetained, CFString, CFType};
use objc2_core_media::{
    CMBlockBuffer, CMFormatDescription, CMSampleBuffer,
    CMVideoFormatDescriptionCreateFromH264ParameterSets,
};
use objc2_core_video::{
    kCVPixelBufferIOSurfacePropertiesKey, kCVPixelBufferMetalCompatibilityKey,
    kCVPixelBufferPixelFormatTypeKey, kCVPixelFormatType_420YpCbCr8BiPlanarVideoRange,
    CVImageBuffer,
};
use objc2_video_toolbox::{
    kVTVideoDecoderSpecification_RequireHardwareAcceleratedVideoDecoder, VTDecodeFrameFlags,
    VTDecodeInfoFlags, VTDecompressionOutputCallbackRecord, VTDecompressionSession,
};
use picoo_bitstream::{AccessUnit, PictureKind, RandomAccessPoint};
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
    sps: Vec<u8>,
    pps: Vec<u8>,
}

impl VideoToolboxDecoder {
    pub fn new() -> Self {
        Self {
            session: None,
            format_description: None,
            output: Box::default(),
            sps: Vec::new(),
            pps: Vec::new(),
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

    fn ensure_session(&mut self, sps: &[u8], pps: &[u8]) -> Result<(), DecodeError> {
        if self.session.is_some() && self.sps == sps && self.pps == pps {
            return Ok(());
        }

        self.reset_session();
        let format_description = create_h264_format_description(sps, pps)?;

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

        self.sps = sps.to_vec();
        self.pps = pps.to_vec();
        self.format_description = Some(format_description);
        self.session = Some(session);
        Ok(())
    }

    fn decode_real_access_unit(
        &mut self,
        access_unit: &[u8],
        picture: AccessUnit<'_>,
        stream_config: Option<&StreamConfig>,
    ) -> Result<DecodeOutcome, DecodeError> {
        let contains_idr =
            picture.picture().kind == PictureKind::RandomAccess(RandomAccessPoint::AvcIdr);
        let (sps, pps) =
            parameter_sets(stream_config, &picture).ok_or(DecodeError::NotInitialized)?;
        let facts = picoo_bitstream::AvcSpsFacts::parse(&sps)
            .map_err(|error| DecodeError::Platform(error.to_string()))?;
        crate::native_format::validate_source(&facts)?;
        if stream_config.is_some_and(|config| {
            (config.width, config.height) != (facts.visible_width, facts.visible_height)
        }) {
            return Err(DecodeError::ConfigurationMismatch);
        }
        self.ensure_session(&sps, &pps)?;
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
            return Ok(DecodeOutcome::accepted_without_frame(contains_idr));
        };
        let native_format = crate::native_format::describe(&facts, &output)?;
        Ok(DecodeOutcome::frame(
            DecodedFrame::native(
                output,
                native_format,
                stream_config.map(|config| config.rotation).unwrap_or(0),
                now_timestamp_us(),
            ),
            contains_idr,
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
    fn decode_access_unit(
        &mut self,
        access_unit: &[u8],
        stream_config: Option<&StreamConfig>,
    ) -> Result<DecodeOutcome, DecodeError> {
        let picture = crate::configured_avc::validate(access_unit, stream_config)?;
        self.decode_real_access_unit(access_unit, picture, stream_config)
    }

    fn flush(&mut self) -> Result<Option<DecodedFrame>, DecodeError> {
        self.reset()?;
        Ok(None)
    }

    fn reset(&mut self) -> Result<(), DecodeError> {
        self.reset_session();
        self.sps.clear();
        self.pps.clear();
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

fn parameter_sets(
    stream_config: Option<&StreamConfig>,
    picture: &AccessUnit<'_>,
) -> Option<(Vec<u8>, Vec<u8>)> {
    match stream_config {
        // Validated before native state mutation by configured_avc::validate.
        Some(config) => {
            let configuration = crate::configured_avc::configuration(config).ok()?;
            if configuration.sps().len() != 1 || configuration.pps().len() != 1 {
                return None;
            }
            Some((
                configuration.sps()[0].to_vec(),
                configuration.pps()[0].to_vec(),
            ))
        }
        None => Some((
            picture
                .nals()
                .iter()
                .find(|nal| nal[0] & 0x1f == 7)?
                .to_vec(),
            picture
                .nals()
                .iter()
                .find(|nal| nal[0] & 0x1f == 8)?
                .to_vec(),
        )),
    }
}

fn create_h264_format_description(
    sps: &[u8],
    pps: &[u8],
) -> Result<CFRetained<CMFormatDescription>, DecodeError> {
    let mut parameter_set_pointers = [
        NonNull::new(sps.as_ptr().cast_mut())
            .ok_or_else(|| DecodeError::Platform("empty H.264 SPS".into()))?,
        NonNull::new(pps.as_ptr().cast_mut())
            .ok_or_else(|| DecodeError::Platform("empty H.264 PPS".into()))?,
    ];
    let mut parameter_set_sizes = [sps.len(), pps.len()];
    let mut raw_description: *const CMFormatDescription = ptr::null();
    // SAFETY: The parameter-set pointers and sizes remain valid for the call;
    // CoreMedia copies them into the returned format description.
    let status = unsafe {
        CMVideoFormatDescriptionCreateFromH264ParameterSets(
            None,
            parameter_set_pointers.len(),
            NonNull::new(parameter_set_pointers.as_mut_ptr())
                .expect("fixed-size pointer array is non-null"),
            NonNull::new(parameter_set_sizes.as_mut_ptr())
                .expect("fixed-size size array is non-null"),
            4,
            NonNull::from(&mut raw_description),
        )
    };
    check_status(
        "CMVideoFormatDescriptionCreateFromH264ParameterSets",
        status,
    )?;
    let raw_description = NonNull::new(raw_description.cast_mut()).ok_or_else(|| {
        DecodeError::Platform("CoreMedia returned a null format description".into())
    })?;
    // SAFETY: A successful create returns ownership of a +1 CF object.
    Ok(unsafe { CFRetained::from_raw(raw_description) })
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
mod tests {
    use super::*;
    use picoo_bitstream::avc::{extract_sps_pps, split_annex_b_nals};

    use picoo_testkit::{
        AVC_1280X720_BT709_IDR as H264_1280X720_RED_IDR, AVC_64X64_BT709_IDR as H264_64X64_RED_IDR,
    };

    fn wire(annex: &[u8]) -> Vec<u8> {
        picoo_bitstream::canonical_access_unit(
            picoo_bitstream::Codec::Avc,
            picoo_bitstream::NalFormat::AnnexB,
            annex,
        )
        .unwrap()
        .into_owned()
    }

    fn assert_native_red(frame: &DecodedFrame) {
        use objc2_core_video::*;
        // Explicit diagnostic read of one YUV sample, not a Decoder pixel API.
        unsafe {
            let buffer = frame.native_image().apple().unwrap().pixel_buffer();
            assert!(CVPixelBufferGetIOSurface(Some(buffer)).is_some());
            assert_eq!(
                CVPixelBufferLockBaseAddress(buffer, CVPixelBufferLockFlags::ReadOnly),
                0
            );
            let y = *CVPixelBufferGetBaseAddressOfPlane(buffer, 0).cast::<u8>();
            let uv = CVPixelBufferGetBaseAddressOfPlane(buffer, 1).cast::<u8>();
            let chroma = [*uv, *uv.add(1)];
            assert_eq!(
                CVPixelBufferUnlockBaseAddress(buffer, CVPixelBufferLockFlags::ReadOnly),
                0
            );
            assert!(
                y > 16 && chroma[1] > chroma[0],
                "expected decoded red fixture"
            );
        }
    }

    #[test]
    fn conflicting_in_band_configuration_does_not_replace_native_session() {
        let (sps, pps) = extract_sps_pps(H264_64X64_RED_IDR).unwrap();
        let config = StreamConfig {
            codec: picoo_protocol::control::VideoCodec::Avc as i32,
            width: 64,
            height: 64,
            codec_configuration: picoo_bitstream::CodecConfiguration::from_avc_parameter_sets(
                &sps, &pps,
            )
            .unwrap()
            .record()
            .to_vec(),
            ..Default::default()
        };
        let mut decoder = VideoToolboxDecoder::new();
        decoder
            .decode_access_unit(&wire(H264_64X64_RED_IDR), Some(&config))
            .unwrap();
        let session = decoder.session.as_ref().map(CFRetained::as_ptr);
        assert!(matches!(
            decoder.decode_access_unit(&wire(H264_1280X720_RED_IDR), Some(&config)),
            Err(DecodeError::ConfigurationMismatch)
        ));
        assert_eq!(session, decoder.session.as_ref().map(CFRetained::as_ptr));
        let frame = decoder
            .decode_access_unit(&wire(H264_64X64_RED_IDR), Some(&config))
            .unwrap()
            .frame
            .unwrap();
        assert_eq!(frame.description().width, 64);
    }

    #[test]
    fn declared_geometry_mismatch_does_not_mutate_native_session() {
        let (sps, pps) = extract_sps_pps(H264_64X64_RED_IDR).unwrap();
        let mut config = StreamConfig {
            codec: picoo_protocol::control::VideoCodec::Avc as i32,
            width: 64,
            height: 64,
            codec_configuration: picoo_bitstream::CodecConfiguration::from_avc_parameter_sets(
                &sps, &pps,
            )
            .unwrap()
            .record()
            .to_vec(),
            ..Default::default()
        };
        let mut decoder = VideoToolboxDecoder::new();
        decoder
            .decode_access_unit(&wire(H264_64X64_RED_IDR), Some(&config))
            .unwrap();
        let session = decoder.session.as_ref().map(CFRetained::as_ptr);
        config.width = 1280;
        assert!(matches!(
            decoder.decode_access_unit(&wire(H264_64X64_RED_IDR), Some(&config)),
            Err(DecodeError::ConfigurationMismatch)
        ));
        assert_eq!(session, decoder.session.as_ref().map(CFRetained::as_ptr));
    }

    #[test]
    fn unknown_native_color_is_rejected_instead_of_relabelled() {
        let mut decoder = VideoToolboxDecoder::new();
        assert!(decoder
            .decode_access_unit(&wire(picoo_testkit::H264_64X64_RED_IDR), None)
            .is_err());
    }

    #[test]
    fn videotoolbox_decodes_canonical_idr_to_native_frame() {
        let mut decoder = VideoToolboxDecoder::new();
        let frame = decoder
            .decode_access_unit(&wire(H264_64X64_RED_IDR), None)
            .expect("VideoToolbox decode")
            .frame
            .expect("decoded frame");
        assert_eq!(
            (frame.description().width, frame.description().height),
            (64, 64)
        );
        assert_native_red(&frame);
    }

    #[test]
    fn videotoolbox_decodes_avcc_idr_with_stream_config() {
        let (sps, pps) = extract_sps_pps(H264_64X64_RED_IDR).expect("parameter sets");
        let idr = split_annex_b_nals(H264_64X64_RED_IDR)
            .into_iter()
            .find(|nal| nal.first().is_some_and(|byte| byte & 0x1f == 5))
            .expect("IDR");
        let mut avcc = Vec::with_capacity(idr.len() + 4);
        avcc.extend_from_slice(&(idr.len() as u32).to_be_bytes());
        avcc.extend_from_slice(idr);
        let config = StreamConfig {
            codec: picoo_protocol::control::VideoCodec::Avc as i32,
            width: 64,
            height: 64,
            codec_configuration: picoo_bitstream::CodecConfiguration::from_avc_parameter_sets(
                &sps, &pps,
            )
            .unwrap()
            .record()
            .to_vec(),
            ..Default::default()
        };

        let mut decoder = VideoToolboxDecoder::new();
        let frame = decoder
            .decode_access_unit(&avcc, Some(&config))
            .expect("VideoToolbox decode")
            .frame
            .expect("decoded frame");
        assert_eq!(
            (frame.description().width, frame.description().height),
            (64, 64)
        );
        assert_native_red(&frame);
    }

    #[test]
    fn same_parameter_sets_reuse_session_and_flush_resets_it() {
        let (sps, pps) = extract_sps_pps(H264_64X64_RED_IDR).expect("parameter sets");
        let mut decoder = VideoToolboxDecoder::new();
        decoder.ensure_session(&sps, &pps).expect("first session");
        let first = decoder.session.as_ref().map(CFRetained::as_ptr);
        decoder.ensure_session(&sps, &pps).expect("reused session");
        assert_eq!(first, decoder.session.as_ref().map(CFRetained::as_ptr));
        decoder.flush().expect("flush");
        assert!(decoder.session.is_none());
    }

    #[test]
    fn malformed_access_unit_is_rejected_without_stub_fallback() {
        let mut decoder = VideoToolboxDecoder::new();
        let result = decoder.decode_access_unit(b"not-h264", None);
        assert!(matches!(result, Err(DecodeError::UnsupportedAccessUnit)));
        assert!(decoder.session.is_none());
    }

    #[test]
    fn in_band_parameter_change_recreates_session_and_updates_dimensions() {
        let mut decoder = VideoToolboxDecoder::new();
        let first = decoder
            .decode_access_unit(&wire(H264_64X64_RED_IDR), None)
            .expect("64x64 decode")
            .frame
            .expect("64x64 frame");
        assert_eq!(
            (first.description().width, first.description().height),
            (64, 64)
        );
        let first_sps = decoder.sps.clone();

        let second = decoder
            .decode_access_unit(&wire(H264_1280X720_RED_IDR), None)
            .expect("1280x720 decode")
            .frame
            .expect("1280x720 frame");
        assert_eq!(
            (second.description().width, second.description().height),
            (1280, 720)
        );
        assert_native_red(&first);
        assert_ne!(first_sps, decoder.sps);
        assert_eq!(
            decoder.sps,
            extract_sps_pps(H264_1280X720_RED_IDR)
                .expect("1280x720 parameter sets")
                .0
        );
    }
}
