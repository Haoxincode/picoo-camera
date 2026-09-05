//! H.264 access-unit decoding — REQ-PICOO-MEDIA-005/006/012/023.
//!
//! Receiver decodes once; output NV12 feeds LatestFrameStore and Shared Frame Ring.
//! - Windows: Media Foundation (`windows-mf`)
//! - macOS: VideoToolbox through pure Rust Apple framework bindings
//! - Linux/CI: Cisco OpenH264 soft decode, with StubDecoder fallback for fixtures

mod stub;

#[cfg(all(windows, feature = "windows-mf"))]
mod mf;

#[cfg(target_os = "macos")]
mod videotoolbox;

#[cfg(all(not(windows), not(target_vendor = "apple")))]
mod openh264_dec;

use bytes::Bytes;
use picoo_protocol::control::StreamConfig;
use thiserror::Error;

pub use stub::StubDecoder;

#[derive(Debug, Error)]
pub enum DecodeError {
    #[error("decoder not initialized")]
    NotInitialized,
    #[error("unsupported access unit")]
    UnsupportedAccessUnit,
    #[error("platform decoder: {0}")]
    Platform(String),
    #[error("output too large: {0} bytes")]
    OutputTooLarge(usize),
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum VideoPixelFormat {
    Nv12,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum VideoColorMatrix {
    Bt709,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum VideoColorRange {
    Limited,
}

/// Pixel interpretation independent of the backing storage.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct DecodedFrameDescription {
    pub width: u32,
    pub height: u32,
    pub stride: u32,
    pub rotation: u32,
    pub pixel_format: VideoPixelFormat,
    pub color_matrix: VideoColorMatrix,
    pub color_range: VideoColorRange,
}

/// Owned decoder output storage.
///
/// CPU NV12 is the portable fallback and the current Shared Ring contract.
/// Future native variants must carry an explicitly transfer-safe owner; a raw
/// platform pointer is not sufficient and must never be made broadly `Send`.
#[non_exhaustive]
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum DecodedFrameStorage {
    CpuNv12(Bytes),
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DecodedFrame {
    description: DecodedFrameDescription,
    timestamp_us: u64,
    storage: DecodedFrameStorage,
}

impl DecodedFrame {
    #[allow(clippy::too_many_arguments)]
    pub fn cpu_nv12(
        width: u32,
        height: u32,
        stride: u32,
        rotation: u32,
        timestamp_us: u64,
        nv12: Bytes,
    ) -> Self {
        Self {
            description: DecodedFrameDescription {
                width,
                height,
                stride,
                rotation,
                pixel_format: VideoPixelFormat::Nv12,
                color_matrix: VideoColorMatrix::Bt709,
                color_range: VideoColorRange::Limited,
            },
            timestamp_us,
            storage: DecodedFrameStorage::CpuNv12(nv12),
        }
    }

    pub fn description(&self) -> DecodedFrameDescription {
        self.description
    }

    pub fn timestamp_us(&self) -> u64 {
        self.timestamp_us
    }

    pub fn storage(&self) -> &DecodedFrameStorage {
        &self.storage
    }

    pub fn cpu_nv12_bytes(&self) -> Option<&Bytes> {
        match &self.storage {
            DecodedFrameStorage::CpuNv12(bytes) => Some(bytes),
        }
    }

    pub fn set_rotation(&mut self, rotation: u32) {
        self.description.rotation = rotation;
    }

    pub fn into_cpu_nv12(self) -> Bytes {
        match self.storage {
            DecodedFrameStorage::CpuNv12(bytes) => bytes,
        }
    }
}

/// Result of submitting one access unit to a platform decoder.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DecodeOutcome {
    pub frame: Option<DecodedFrame>,
    /// True only when this AU contained an IDR and the platform accepted it
    /// without reporting a drop. Receiver uses this to leave AwaitingRefresh.
    pub refresh_accepted: bool,
}

impl DecodeOutcome {
    pub fn frame(frame: DecodedFrame, refresh_accepted: bool) -> Self {
        Self {
            frame: Some(frame),
            refresh_accepted,
        }
    }

    pub fn accepted_without_frame(refresh_accepted: bool) -> Self {
        Self {
            frame: None,
            refresh_accepted,
        }
    }
}

/// Decode one H.264 access unit into NV12 for LatestFrameStore consumption.
pub trait AccessUnitDecoder: Send {
    fn decode_access_unit(
        &mut self,
        access_unit: &[u8],
        stream_config: Option<&StreamConfig>,
    ) -> Result<DecodeOutcome, DecodeError>;

    fn flush(&mut self) -> Result<Option<DecodedFrame>, DecodeError> {
        Ok(None)
    }

    /// Discard all queued output and prediction/reference state.
    ///
    /// Unlike [`Self::flush`], reset must not publish delayed frames. The next
    /// accepted access unit is expected to establish a fresh decode chain
    /// (normally an IDR with the active StreamConfig parameter sets).
    fn reset(&mut self) -> Result<(), DecodeError>;
}

/// Select the native production decoder for desktop targets.
pub fn create_platform_decoder() -> Box<dyn AccessUnitDecoder> {
    create_platform_decoder_impl()
}

#[cfg(target_os = "macos")]
fn create_platform_decoder_impl() -> Box<dyn AccessUnitDecoder> {
    tracing::info!("Using VideoToolbox H.264 decoder");
    Box::new(videotoolbox::VideoToolboxDecoder::new())
}

#[cfg(all(windows, feature = "windows-mf"))]
fn create_platform_decoder_impl() -> Box<dyn AccessUnitDecoder> {
    match mf::MfH264Decoder::new() {
        Ok(decoder) => {
            tracing::info!("Using Media Foundation H.264 decoder");
            Box::new(decoder)
        }
        Err(err) => {
            tracing::error!("MF decoder unavailable: {err}");
            Box::new(UnavailableDecoder(format!(
                "Media Foundation initialization failed: {err}"
            )))
        }
    }
}

#[cfg(all(windows, feature = "windows-mf"))]
struct UnavailableDecoder(String);

#[cfg(all(windows, feature = "windows-mf"))]
impl AccessUnitDecoder for UnavailableDecoder {
    fn decode_access_unit(
        &mut self,
        _access_unit: &[u8],
        _stream_config: Option<&StreamConfig>,
    ) -> Result<DecodeOutcome, DecodeError> {
        Err(DecodeError::Platform(self.0.clone()))
    }

    fn reset(&mut self) -> Result<(), DecodeError> {
        Ok(())
    }
}

#[cfg(all(windows, not(feature = "windows-mf")))]
fn create_platform_decoder_impl() -> Box<dyn AccessUnitDecoder> {
    Box::new(StubDecoder::new())
}

#[cfg(all(target_vendor = "apple", not(target_os = "macos")))]
fn create_platform_decoder_impl() -> Box<dyn AccessUnitDecoder> {
    // Sender-only Apple targets do not own a Receiver decoder.
    Box::new(StubDecoder::new())
}

#[cfg(all(not(windows), not(target_vendor = "apple")))]
fn create_platform_decoder_impl() -> Box<dyn AccessUnitDecoder> {
    match openh264_dec::OpenH264Decoder::new() {
        Ok(decoder) => {
            tracing::info!("Using OpenH264 software H.264 decoder");
            Box::new(decoder)
        }
        Err(err) => {
            tracing::warn!("OpenH264 unavailable, falling back to stub: {err}");
            Box::new(StubDecoder::new())
        }
    }
}

pub fn now_timestamp_us() -> u64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_micros() as u64)
        .unwrap_or(0)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn stub_decodes_loopback_access_unit() {
        let mut decoder = StubDecoder::new();
        let frame = decoder
            .decode_access_unit(b"test-au", None)
            .expect("decode")
            .frame
            .expect("frame");
        assert!(!frame.cpu_nv12_bytes().expect("CPU NV12").is_empty());
        assert_eq!(frame.description().width, 1280);
        assert_eq!(frame.description().height, 720);
    }

    #[test]
    #[cfg(all(not(windows), not(target_vendor = "apple")))]
    fn openh264_roundtrip_produces_nv12() {
        use openh264::encoder::Encoder;
        use openh264::formats::YUVBuffer;

        let width = 64usize;
        let height = 64usize;
        let mut planes = vec![128u8; width * height * 3 / 2];
        for y in 0..height {
            for x in 0..width {
                planes[y * width + x] = ((x + y) % 256) as u8;
            }
        }
        let yuv = YUVBuffer::from_vec(planes, width, height);
        let mut encoder = Encoder::new().expect("encoder");
        let bitstream = encoder.encode(&yuv).expect("encode");
        let annex = bitstream.to_vec();
        assert!(
            annex.len() > 64,
            "encoded AU should exceed stub-heuristic size"
        );
        assert!(
            annex.windows(3).any(|w| w == [0, 0, 1]),
            "encoded AU must be Annex-B"
        );

        let mut decoder = create_platform_decoder();
        let frame = decoder
            .decode_access_unit(&annex, None)
            .expect("decode")
            .frame
            .expect("picture");
        let description = frame.description();
        let nv12 = frame.cpu_nv12_bytes().expect("CPU NV12");
        assert_eq!(description.width, width as u32);
        assert_eq!(description.height, height as u32);
        assert_eq!(
            nv12.len(),
            picoo_frame_hub::nv12_byte_size(description.width, description.height)
        );
        // Real decode should not be constant grey placeholder.
        assert!(nv12.iter().any(|b| *b != 16 && *b != 128));
    }

    #[test]
    #[cfg(all(not(windows), not(target_vendor = "apple")))]
    fn openh264_falls_back_to_stub_for_tiny_fixture() {
        let mut decoder = create_platform_decoder();
        let frame = decoder
            .decode_access_unit(b"test-au", None)
            .expect("decode")
            .frame
            .expect("frame");
        assert_eq!(frame.description().width, 1280);
        assert_eq!(frame.description().height, 720);
    }

    #[test]
    #[cfg(all(not(windows), not(target_vendor = "apple")))]
    fn openh264_decodes_length_prefixed_au() {
        use openh264::encoder::Encoder;
        use openh264::formats::YUVBuffer;
        use picoo_packet::{length_prefixed_to_annex_b, split_annex_b_nals};

        let width = 64usize;
        let height = 64usize;
        let mut planes = vec![128u8; width * height * 3 / 2];
        for y in 0..height {
            for x in 0..width {
                planes[y * width + x] = ((x * 7 + y) % 200 + 20) as u8;
            }
        }
        let yuv = YUVBuffer::from_vec(planes, width, height);
        let mut encoder = Encoder::new().expect("encoder");
        let annex = encoder.encode(&yuv).expect("encode").to_vec();
        // Rebuild as MediaCodec-style length-prefixed AU.
        let mut length_prefixed = Vec::new();
        for nal in split_annex_b_nals(&annex) {
            length_prefixed.extend_from_slice(&(nal.len() as u32).to_be_bytes());
            length_prefixed.extend_from_slice(nal);
        }
        assert!(
            length_prefixed_to_annex_b(&length_prefixed).is_some(),
            "fixture must look like AVCC AU"
        );

        let mut decoder = create_platform_decoder();
        let frame = decoder
            .decode_access_unit(&length_prefixed, None)
            .expect("decode")
            .frame
            .expect("picture");
        assert_eq!(frame.description().width, width as u32);
        assert_eq!(frame.description().height, height as u32);
    }
}
