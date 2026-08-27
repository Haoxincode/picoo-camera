//! H.264 access-unit decoding — REQ-PICOO-MEDIA-005/006.
//!
//! Receiver decodes once; output NV12 feeds FrameHub and Shared Frame Ring.
//! Linux CI uses [`StubDecoder`]; Windows builds enable `windows-mf` for MF pipeline.

mod stub;

#[cfg(all(windows, feature = "windows-mf"))]
mod mf;

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

/// Platform decoder selection — stub on Linux; MF when `windows-mf` is enabled.
pub fn create_platform_decoder() -> Box<dyn AccessUnitDecoder> {
    #[cfg(all(windows, feature = "windows-mf"))]
    {
        match mf::MfH264Decoder::new() {
            Ok(decoder) => {
                tracing::info!("Using Media Foundation H.264 decoder");
                return Box::new(decoder);
            }
            Err(err) => {
                tracing::warn!("MF decoder unavailable, falling back to stub: {err}");
            }
        }
    }
    Box::new(StubDecoder::new())
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
}
