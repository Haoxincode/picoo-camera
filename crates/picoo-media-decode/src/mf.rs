//! Media Foundation H.264 decoder skeleton — REQ-PICOO-MEDIA-005.
//!
//! Full IMFTransform pipeline lands on `windows-latest`; this module initializes MF
//! and delegates decode to [`StubDecoder`] until transform graph is wired.

use picoo_protocol::control::StreamConfig;
use windows::Win32::Media::MediaFoundation::{MFStartup, MF_VERSION};
use windows::Win32::System::Com::{CoInitializeEx, COINIT_MULTITHREADED};

use crate::{AccessUnitDecoder, DecodeError, DecodedFrame, StubDecoder};

pub struct MfH264Decoder {
    _mf_started: bool,
    fallback: StubDecoder,
}

impl MfH264Decoder {
    pub fn new() -> Result<Self, DecodeError> {
        unsafe {
            CoInitializeEx(None, COINIT_MULTITHREADED)
                .ok()
                .map_err(|e| DecodeError::Platform(format!("CoInitializeEx: {e}")))?;
            MFStartup(MF_VERSION, Default::default())
                .map_err(|e| DecodeError::Platform(format!("MFStartup: {e}")))?;
        }
        Ok(Self {
            _mf_started: true,
            fallback: StubDecoder::new(),
        })
    }
}

impl Drop for MfH264Decoder {
    fn drop(&mut self) {
        if self._mf_started {
            unsafe {
                let _ = windows::Win32::Media::MediaFoundation::MFShutdown();
            }
        }
    }
}

impl AccessUnitDecoder for MfH264Decoder {
    fn decode_access_unit(
        &mut self,
        access_unit: &[u8],
        stream_config: Option<&StreamConfig>,
    ) -> Result<Option<DecodedFrame>, DecodeError> {
        // TODO(REQ-PICOO-MEDIA-005): IMFTransform H.264 → NV12 via D3D11 media type.
        let _ = access_unit;
        self.fallback.decode_access_unit(access_unit, stream_config)
    }
}
