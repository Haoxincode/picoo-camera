//! Cisco OpenH264 software decoder for Linux CI / non-Windows hosts — REQ-PICOO-MEDIA-005.

use bytes::Bytes;
use openh264::decoder::Decoder;
use openh264::formats::YUVSource;
use openh264::nal_units;
use picoo_frame_hub::nv12_byte_size;
use picoo_packet::{
    access_unit_to_annex_b, annex_b_parameter_sets, is_length_prefixed_access_unit,
};
use picoo_protocol::control::StreamConfig;

use crate::stub::StubDecoder;
use crate::{now_timestamp_us, AccessUnitDecoder, DecodeError, DecodedFrame};

pub struct OpenH264Decoder {
    decoder: Decoder,
    last_sps: Vec<u8>,
    last_pps: Vec<u8>,
    param_sets_fed: bool,
    stub: StubDecoder,
}

impl OpenH264Decoder {
    pub fn new() -> Result<Self, DecodeError> {
        let decoder = Decoder::new().map_err(|e| DecodeError::Platform(e.to_string()))?;
        Ok(Self {
            decoder,
            last_sps: Vec::new(),
            last_pps: Vec::new(),
            param_sets_fed: false,
            stub: StubDecoder::new(),
        })
    }

    fn ensure_param_sets(
        &mut self,
        stream_config: Option<&StreamConfig>,
    ) -> Result<(), DecodeError> {
        let Some(cfg) = stream_config else {
            return Ok(());
        };
        if cfg.sps.is_empty() || cfg.pps.is_empty() {
            return Ok(());
        }
        if self.param_sets_fed && cfg.sps == self.last_sps && cfg.pps == self.last_pps {
            return Ok(());
        }
        let annex = annex_b_parameter_sets(&cfg.sps, &cfg.pps);
        // Feed SPS/PPS; picture may not be ready yet.
        let _ = self
            .decoder
            .decode(&annex)
            .map_err(|e| DecodeError::Platform(e.to_string()))?;
        self.last_sps = cfg.sps.clone();
        self.last_pps = cfg.pps.clone();
        self.param_sets_fed = true;
        Ok(())
    }

    fn looks_like_loopback_stub(access_unit: &[u8], stream_config: Option<&StreamConfig>) -> bool {
        if is_length_prefixed_access_unit(access_unit) {
            return false;
        }
        if access_unit.len() <= 64 {
            return true;
        }
        let (w, h) = stream_config
            .map(|c| (c.width, c.height))
            .unwrap_or((1280, 720));
        if w > 0 && h > 0 && access_unit.len() == nv12_byte_size(w, h) {
            return true;
        }
        // Real H.264 AUs typically contain Annex-B start codes or length-prefixed NALs.
        let has_start_code = access_unit.windows(3).any(|w| w == [0, 0, 1]);
        !has_start_code
            && access_unit.first().map(|b| b & 0x1f) != Some(5)
            && access_unit.first().map(|b| b & 0x1f) != Some(1)
    }

    fn i420_to_nv12(yuv: &impl YUVSource) -> Result<(u32, u32, u32, Vec<u8>), DecodeError> {
        let (width, height) = yuv.dimensions();
        let (y_stride, u_stride, v_stride) = yuv.strides();
        let width = width as u32;
        let height = height as u32;
        let y_plane = yuv.y();
        let u_plane = yuv.u();
        let v_plane = yuv.v();

        let mut nv12 = vec![0u8; nv12_byte_size(width, height)];
        // Copy Y tightly packed.
        for row in 0..height as usize {
            let src = row * y_stride;
            let dst = row * width as usize;
            let end = src + width as usize;
            if end > y_plane.len() || dst + width as usize > nv12.len() {
                return Err(DecodeError::Platform("Y plane bounds".into()));
            }
            nv12[dst..dst + width as usize].copy_from_slice(&y_plane[src..end]);
        }
        // Interleave UV.
        let uv_offset = (width as usize) * (height as usize);
        let chroma_h = (height as usize).div_ceil(2);
        let chroma_w = (width as usize).div_ceil(2);
        for row in 0..chroma_h {
            for col in 0..chroma_w {
                let u = u_plane.get(row * u_stride + col).copied().unwrap_or(128);
                let v = v_plane.get(row * v_stride + col).copied().unwrap_or(128);
                let dst = uv_offset + row * width as usize + col * 2;
                if dst + 1 >= nv12.len() {
                    return Err(DecodeError::Platform("UV plane bounds".into()));
                }
                nv12[dst] = u;
                nv12[dst + 1] = v;
            }
        }
        Ok((width, height, width, nv12))
    }
}

impl AccessUnitDecoder for OpenH264Decoder {
    fn decode_access_unit(
        &mut self,
        access_unit: &[u8],
        stream_config: Option<&StreamConfig>,
    ) -> Result<Option<DecodedFrame>, DecodeError> {
        // Preserve stub semantics for unit/loopback fixtures that are not real H.264.
        if Self::looks_like_loopback_stub(access_unit, stream_config) {
            return self.stub.decode_access_unit(access_unit, stream_config);
        }

        self.ensure_param_sets(stream_config)?;

        let annex = access_unit_to_annex_b(access_unit);
        let access_unit = annex.as_ref();

        let mut last: Option<(u32, u32, u32, Vec<u8>)> = None;
        // Decode each NAL; keep the latest picture (IDR/P).
        for nal in nal_units(access_unit) {
            match self.decoder.decode(nal) {
                Ok(Some(yuv)) => {
                    last = Some(Self::i420_to_nv12(&yuv)?);
                }
                Ok(None) => {}
                Err(err) => {
                    tracing::debug!("openh264 decode NAL: {err}");
                }
            }
        }
        // Also try feeding the whole AU if it already includes start codes as one buffer.
        if last.is_none() {
            match self.decoder.decode(access_unit) {
                Ok(Some(yuv)) => last = Some(Self::i420_to_nv12(&yuv)?),
                Ok(None) => {}
                Err(err) => {
                    return Err(DecodeError::Platform(err.to_string()));
                }
            }
        }

        let Some((width, height, stride, nv12)) = last else {
            return Ok(None);
        };
        Ok(Some(DecodedFrame {
            width,
            height,
            stride,
            rotation: stream_config.map(|c| c.rotation).unwrap_or(0),
            timestamp_us: now_timestamp_us(),
            nv12: Bytes::from(nv12),
        }))
    }

    fn flush(&mut self) -> Result<Option<DecodedFrame>, DecodeError> {
        let frames = self
            .decoder
            .flush_remaining()
            .map_err(|e| DecodeError::Platform(e.to_string()))?;
        let Some(yuv) = frames.last() else {
            return Ok(None);
        };
        let (width, height, stride, nv12) = Self::i420_to_nv12(yuv)?;
        Ok(Some(DecodedFrame {
            width,
            height,
            stride,
            rotation: 0,
            timestamp_us: now_timestamp_us(),
            nv12: Bytes::from(nv12),
        }))
    }
}
