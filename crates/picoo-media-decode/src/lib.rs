//! H.264 access-unit decoding — REQ-PICOO-MEDIA-005/006/012.
//!
//! Receiver decodes once; output NV12 feeds FrameHub and Shared Frame Ring.
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

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DecodedFrame {
    pub width: u32,
    pub height: u32,
    pub stride: u32,
    pub rotation: u32,
    pub timestamp_us: u64,
    pub nv12: Bytes,
}

/// Decode one H.264 access unit into NV12 for FrameHub consumption.
pub trait AccessUnitDecoder: Send {
    fn decode_access_unit(
        &mut self,
        access_unit: &[u8],
        stream_config: Option<&StreamConfig>,
    ) -> Result<Option<DecodedFrame>, DecodeError>;

    fn flush(&mut self) -> Result<Option<DecodedFrame>, DecodeError> {
        Ok(None)
    }
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
            tracing::warn!("MF decoder unavailable, falling back to stub: {err}");
            Box::new(StubDecoder::new())
        }
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
            .expect("frame");
        assert!(!frame.nv12.is_empty());
        assert_eq!(frame.width, 1280);
        assert_eq!(frame.height, 720);
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
            .expect("picture");
        assert_eq!(frame.width, width as u32);
        assert_eq!(frame.height, height as u32);
        assert_eq!(
            frame.nv12.len(),
            picoo_frame_hub::nv12_byte_size(frame.width, frame.height)
        );
        // Real decode should not be constant grey placeholder.
        assert!(frame.nv12.iter().any(|b| *b != 16 && *b != 128));
    }

    #[test]
    #[cfg(all(not(windows), not(target_vendor = "apple")))]
    fn openh264_falls_back_to_stub_for_tiny_fixture() {
        let mut decoder = create_platform_decoder();
        let frame = decoder
            .decode_access_unit(b"test-au", None)
            .expect("decode")
            .expect("frame");
        assert_eq!(frame.width, 1280);
        assert_eq!(frame.height, 720);
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
            .expect("picture");
        assert_eq!(frame.width, width as u32);
        assert_eq!(frame.height, height as u32);
    }
}
