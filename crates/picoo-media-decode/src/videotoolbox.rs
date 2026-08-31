//! macOS VideoToolbox H.264 decoder — REQ-PICOO-MEDIA-012.
//!
//! The Apple production path is pure Rust over generated framework bindings:
//! Annex-B/AVCC access unit → CoreMedia sample → VideoToolbox → tightly packed
//! NV12 for FrameHub. OpenH264 is intentionally not linked on Apple targets.

use std::ffi::c_void;
use std::ptr::{self, NonNull};
use std::sync::Mutex;

use bytes::Bytes;
use objc2_core_foundation::{CFBoolean, CFDictionary, CFNumber, CFRetained};
use objc2_core_media::{
    CMBlockBuffer, CMFormatDescription, CMSampleBuffer,
    CMVideoFormatDescriptionCreateFromH264ParameterSets,
};
use objc2_core_video::{
    kCVPixelBufferPixelFormatTypeKey, kCVPixelFormatType_420YpCbCr8BiPlanarVideoRange,
    CVImageBuffer, CVPixelBufferGetBaseAddressOfPlane, CVPixelBufferGetBytesPerRowOfPlane,
    CVPixelBufferGetHeight, CVPixelBufferGetHeightOfPlane, CVPixelBufferGetPixelFormatType,
    CVPixelBufferGetPlaneCount, CVPixelBufferGetWidth, CVPixelBufferGetWidthOfPlane,
    CVPixelBufferLockBaseAddress, CVPixelBufferLockFlags, CVPixelBufferUnlockBaseAddress,
};
use objc2_video_toolbox::{
    kVTVideoDecoderSpecification_RequireHardwareAcceleratedVideoDecoder, VTDecodeFrameFlags,
    VTDecodeInfoFlags, VTDecompressionOutputCallbackRecord, VTDecompressionSession,
};
use picoo_frame_hub::DEFAULT_MAX_FRAME_BYTES;
use picoo_packet::{
    access_unit_contains_idr, annex_b_to_length_prefixed, extract_sps_pps,
    is_length_prefixed_access_unit, split_annex_b_nals,
};
use picoo_protocol::control::StreamConfig;

use crate::{now_timestamp_us, AccessUnitDecoder, DecodeError, DecodeOutcome, DecodedFrame};

struct CopiedNv12 {
    width: u32,
    height: u32,
    stride: u32,
    bytes: Vec<u8>,
}

type DecodeOutput = Result<Option<CopiedNv12>, DecodeError>;

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

// The Receiver owns a decoder on one thread. VideoToolbox may invoke the
// output callback on an internal thread; OutputContext is synchronized.
unsafe impl Send for VideoToolboxDecoder {}

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
        let image_attributes = CFDictionary::from_slices(
            &[unsafe { kCVPixelBufferPixelFormatTypeKey }],
            &[&*nv12_format],
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
        stream_config: Option<&StreamConfig>,
    ) -> Result<DecodeOutcome, DecodeError> {
        // Validate and normalize the AU before touching decoder state. This
        // keeps malformed protocol payloads observable instead of turning
        // them into a misleading "decoder not initialized" result.
        let avcc = access_unit_to_avcc(access_unit)?;
        let contains_idr = access_unit_contains_idr(access_unit);
        let (sps, pps) =
            parameter_sets(stream_config, access_unit).ok_or(DecodeError::NotInitialized)?;
        self.ensure_session(&sps, &pps)?;
        let sample = create_sample_buffer(&avcc, self.format_description.as_deref())?;

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
        Ok(DecodeOutcome::frame(
            DecodedFrame {
                width: output.width,
                height: output.height,
                stride: output.stride,
                rotation: stream_config.map(|config| config.rotation).unwrap_or(0),
                timestamp_us: now_timestamp_us(),
                nv12: Bytes::from(output.bytes),
            },
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
        self.decode_real_access_unit(access_unit, stream_config)
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

fn normalize_parameter_set(data: &[u8], nal_type: u8) -> Option<Vec<u8>> {
    if data.first().is_some_and(|byte| byte & 0x1f == nal_type) {
        return Some(data.to_vec());
    }
    if data.len() > 4 {
        let length = u32::from_be_bytes(data[..4].try_into().ok()?) as usize;
        if length == data.len() - 4 && data[4] & 0x1f == nal_type {
            return Some(data[4..].to_vec());
        }
    }
    split_annex_b_nals(data)
        .into_iter()
        .find(|nal| nal.first().is_some_and(|byte| byte & 0x1f == nal_type))
        .map(ToOwned::to_owned)
}

fn parameter_sets(
    stream_config: Option<&StreamConfig>,
    access_unit: &[u8],
) -> Option<(Vec<u8>, Vec<u8>)> {
    // In-band parameter sets describe this AU most directly. Prefer them so
    // a legal mid-stream format change cannot be shadowed by stale config.
    if let Some(parameter_sets) = extract_sps_pps(access_unit) {
        return Some(parameter_sets);
    }
    if let Some(config) = stream_config {
        if let (Some(sps), Some(pps)) = (
            normalize_parameter_set(&config.sps, 7),
            normalize_parameter_set(&config.pps, 8),
        ) {
            return Some((sps, pps));
        }
        if let Some(parameter_sets) = extract_sps_pps(&config.sps) {
            return Some(parameter_sets);
        }
    }
    None
}

fn access_unit_to_avcc(access_unit: &[u8]) -> Result<Vec<u8>, DecodeError> {
    if is_length_prefixed_access_unit(access_unit) {
        return Ok(access_unit.to_vec());
    }
    annex_b_to_length_prefixed(access_unit).ok_or(DecodeError::UnsupportedAccessUnit)
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
        unsafe { copy_pixel_buffer(image_buffer.as_ref()).map(Some) }
    }))
    .unwrap_or_else(|_| {
        Err(DecodeError::Platform(
            "panic while copying VideoToolbox output".into(),
        ))
    });
    // SAFETY: `output_refcon` points to the boxed OutputContext retained by the
    // decoder until after session invalidation.
    if let Ok(mut slot) = unsafe { context.as_ref() }.result.lock() {
        *slot = Some(result);
    }
}

unsafe fn copy_pixel_buffer(image_buffer: &CVImageBuffer) -> Result<CopiedNv12, DecodeError> {
    if CVPixelBufferGetPixelFormatType(image_buffer)
        != kCVPixelFormatType_420YpCbCr8BiPlanarVideoRange
    {
        return Err(DecodeError::Platform(format!(
            "VideoToolbox output is not NV12/420v: {:#010x}",
            CVPixelBufferGetPixelFormatType(image_buffer)
        )));
    }
    if CVPixelBufferGetPlaneCount(image_buffer) != 2 {
        return Err(DecodeError::Platform(
            "VideoToolbox NV12 output does not have two planes".into(),
        ));
    }

    let width = CVPixelBufferGetWidth(image_buffer);
    let height = CVPixelBufferGetHeight(image_buffer);
    if width == 0 || height == 0 || !width.is_multiple_of(2) || !height.is_multiple_of(2) {
        return Err(DecodeError::Platform(format!(
            "invalid NV12 dimensions {width}x{height}"
        )));
    }
    let width_u32 = u32::try_from(width)
        .map_err(|_| DecodeError::OutputTooLarge(width.saturating_mul(height)))?;
    let height_u32 = u32::try_from(height)
        .map_err(|_| DecodeError::OutputTooLarge(width.saturating_mul(height)))?;
    let output_len = width
        .checked_mul(height)
        .and_then(|luma| luma.checked_add(luma / 2))
        .ok_or(DecodeError::OutputTooLarge(usize::MAX))?;
    if output_len > DEFAULT_MAX_FRAME_BYTES {
        return Err(DecodeError::OutputTooLarge(output_len));
    }
    if CVPixelBufferGetWidthOfPlane(image_buffer, 0) < width
        || CVPixelBufferGetHeightOfPlane(image_buffer, 0) < height
        || CVPixelBufferGetHeightOfPlane(image_buffer, 1) < height / 2
    {
        return Err(DecodeError::Platform(
            "VideoToolbox NV12 plane dimensions are smaller than the frame".into(),
        ));
    }

    let lock_flags = CVPixelBufferLockFlags::ReadOnly;
    check_status(
        "CVPixelBufferLockBaseAddress",
        CVPixelBufferLockBaseAddress(image_buffer, lock_flags),
    )?;
    let copy_result = (|| {
        let y_base = CVPixelBufferGetBaseAddressOfPlane(image_buffer, 0).cast::<u8>();
        let uv_base = CVPixelBufferGetBaseAddressOfPlane(image_buffer, 1).cast::<u8>();
        let y_stride = CVPixelBufferGetBytesPerRowOfPlane(image_buffer, 0);
        let uv_stride = CVPixelBufferGetBytesPerRowOfPlane(image_buffer, 1);
        if y_base.is_null() || uv_base.is_null() || y_stride < width || uv_stride < width {
            return Err(DecodeError::Platform(
                "invalid VideoToolbox NV12 plane storage".into(),
            ));
        }

        let mut bytes = vec![0u8; output_len];
        for row in 0..height {
            // SAFETY: Locked CoreVideo plane metadata guarantees at least
            // `stride` bytes for every reported row.
            let source = unsafe { std::slice::from_raw_parts(y_base.add(row * y_stride), width) };
            bytes[row * width..(row + 1) * width].copy_from_slice(source);
        }
        let uv_offset = width * height;
        for row in 0..height / 2 {
            // SAFETY: Same plane guarantees as the luma copy above.
            let source = unsafe { std::slice::from_raw_parts(uv_base.add(row * uv_stride), width) };
            let destination = uv_offset + row * width;
            bytes[destination..destination + width].copy_from_slice(source);
        }
        Ok(CopiedNv12 {
            width: width_u32,
            height: height_u32,
            stride: width_u32,
            bytes,
        })
    })();
    let unlock_status = CVPixelBufferUnlockBaseAddress(image_buffer, lock_flags);
    check_status("CVPixelBufferUnlockBaseAddress", unlock_status)?;
    copy_result
}

#[cfg(test)]
mod tests {
    use super::*;

    use picoo_testkit::{H264_1280X720_RED_IDR, H264_64X64_RED_IDR};

    #[test]
    fn videotoolbox_decodes_annex_b_idr_to_nv12() {
        let mut decoder = VideoToolboxDecoder::new();
        let frame = decoder
            .decode_access_unit(H264_64X64_RED_IDR, None)
            .expect("VideoToolbox decode")
            .frame
            .expect("decoded frame");
        assert_eq!((frame.width, frame.height, frame.stride), (64, 64, 64));
        assert_eq!(frame.nv12.len(), 64 * 64 * 3 / 2);
        assert!(frame.nv12.iter().any(|byte| *byte != 16 && *byte != 128));
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
            width: 64,
            height: 64,
            sps,
            pps,
            ..Default::default()
        };

        let mut decoder = VideoToolboxDecoder::new();
        let frame = decoder
            .decode_access_unit(&avcc, Some(&config))
            .expect("VideoToolbox decode")
            .frame
            .expect("decoded frame");
        assert_eq!((frame.width, frame.height, frame.stride), (64, 64, 64));
        assert_eq!(frame.nv12.len(), 64 * 64 * 3 / 2);
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
            .decode_access_unit(H264_64X64_RED_IDR, None)
            .expect("64x64 decode")
            .frame
            .expect("64x64 frame");
        assert_eq!((first.width, first.height), (64, 64));
        let first_sps = decoder.sps.clone();

        let second = decoder
            .decode_access_unit(H264_1280X720_RED_IDR, None)
            .expect("1280x720 decode")
            .frame
            .expect("1280x720 frame");
        assert_eq!((second.width, second.height), (1280, 720));
        assert_ne!(first_sps, decoder.sps);
        assert_eq!(
            decoder.sps,
            extract_sps_pps(H264_1280X720_RED_IDR)
                .expect("1280x720 parameter sets")
                .0
        );
    }
}
