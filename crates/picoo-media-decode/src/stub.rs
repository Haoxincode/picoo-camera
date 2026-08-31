//! Placeholder decoder for fixtures and MF/OpenH264 fallback — maps test AUs into NV12.

use bytes::Bytes;
use picoo_frame_hub::waiting_placeholder_for_size;
use picoo_protocol::control::StreamConfig;

use crate::{now_timestamp_us, AccessUnitDecoder, DecodeError, DecodedFrame};

const DEFAULT_WIDTH: u32 = 1280;
const DEFAULT_HEIGHT: u32 = 720;

pub struct StubDecoder;

impl StubDecoder {
    pub fn new() -> Self {
        Self
    }

    fn dimensions(stream_config: Option<&StreamConfig>) -> (u32, u32) {
        stream_config
            .map(|cfg| (cfg.width, cfg.height))
            .filter(|(w, h)| *w > 0 && *h > 0)
            .unwrap_or((DEFAULT_WIDTH, DEFAULT_HEIGHT))
    }
}

impl Default for StubDecoder {
    fn default() -> Self {
        Self::new()
    }
}

impl AccessUnitDecoder for StubDecoder {
    fn decode_access_unit(
        &mut self,
        access_unit: &[u8],
        stream_config: Option<&StreamConfig>,
    ) -> Result<Option<DecodedFrame>, DecodeError> {
        let (width, height) = Self::dimensions(stream_config);

        let nv12 = if picoo_frame_hub::nv12_byte_size(width, height) == access_unit.len() {
            // Exact NV12 payload (tests / passthrough).
            access_unit.to_vec()
        } else if access_unit.len() <= 64 {
            let mut frame = waiting_placeholder_for_size(width, height);
            let copy_len = access_unit.len().min(frame.len());
            frame[..copy_len].copy_from_slice(&access_unit[..copy_len]);
            frame
        } else {
            // Keep VCam/UI alive without violating the negotiated NV12 dimensions.
            waiting_placeholder_for_size(width, height)
        };

        Ok(Some(DecodedFrame {
            width,
            height,
            stride: width,
            rotation: 0,
            timestamp_us: now_timestamp_us(),
            nv12: Bytes::from(nv12),
        }))
    }

    fn reset(&mut self) -> Result<(), DecodeError> {
        Ok(())
    }
}
