//! FrameHub, Shared Frame Ring, placeholders, and H.264 decode publish.
//!
//! REQ-PICOO-FRAME-*, REQ-PICOO-MEDIA-004/006/009.

use bytes::Bytes;
use picoo_frame_hub::{
    FrameSlot, PlaceholderMode, SharedFrameRingProducer, PLACEHOLDER_HEIGHT, PLACEHOLDER_WIDTH,
};
use picoo_media_decode::AccessUnitDecoder;

use super::ReceiverSession;
use crate::{ReceiverError, DEFAULT_SHARED_RING_NAME};

impl ReceiverSession {
    /// Attach a cross-process Shared Frame Ring for VCam consumption (REQ-PICOO-FRAME-003).
    pub fn attach_shared_ring(&mut self, name: &str) -> Result<(), ReceiverError> {
        #[cfg(target_os = "macos")]
        let ring = if name == DEFAULT_SHARED_RING_NAME {
            let path = picoo_frame_hub::macos_app_group_ring_path(name)?;
            SharedFrameRingProducer::open_or_create_file(
                path,
                picoo_frame_hub::DEFAULT_MAX_FRAME_BYTES,
            )?
        } else {
            SharedFrameRingProducer::open_or_create(name, picoo_frame_hub::DEFAULT_MAX_FRAME_BYTES)?
        };
        #[cfg(not(target_os = "macos"))]
        let ring = SharedFrameRingProducer::open_or_create(
            name,
            picoo_frame_hub::DEFAULT_MAX_FRAME_BYTES,
        )?;
        self.shared_ring = Some(ring);
        self.publish_waiting_placeholder()?;
        Ok(())
    }

    pub fn publish_waiting_placeholder(&mut self) -> Result<(), ReceiverError> {
        let nv12 = self.placeholder_mode.waiting_frame();
        self.publish_nv12_frame(
            PLACEHOLDER_WIDTH,
            PLACEHOLDER_HEIGHT,
            PLACEHOLDER_WIDTH,
            0,
            0,
            &nv12,
        )
    }

    /// Publish reconnect-branded placeholder (REQ-PICOO-FRAME-005).
    pub fn publish_reconnecting_placeholder(&mut self) -> Result<(), ReceiverError> {
        let nv12 = self.placeholder_mode.reconnecting_frame();
        self.publish_nv12_frame(
            PLACEHOLDER_WIDTH,
            PLACEHOLDER_HEIGHT,
            PLACEHOLDER_WIDTH,
            0,
            0,
            &nv12,
        )
    }

    /// Prefer branded waiting frame (`true`) or solid black (`false`) — PRD §16.
    /// Prefer [`set_placeholder_mode`] for Logo / Black / Bars.
    pub fn set_use_default_placeholder(&mut self, enabled: bool) {
        self.placeholder_mode = if enabled {
            PlaceholderMode::Logo
        } else {
            PlaceholderMode::Black
        };
    }

    pub fn use_default_placeholder(&self) -> bool {
        matches!(self.placeholder_mode, PlaceholderMode::Logo)
    }

    pub fn set_placeholder_mode(&mut self, mode: PlaceholderMode) {
        self.placeholder_mode = mode;
    }

    pub fn placeholder_mode(&self) -> PlaceholderMode {
        self.placeholder_mode
    }

    /// Test-only decoder injection keeps synthetic payload support outside the
    /// production platform decoder.
    #[cfg(test)]
    pub fn set_decoder_for_test(&mut self, decoder: Box<dyn AccessUnitDecoder>) {
        self.decoder = decoder;
    }

    /// Decode H.264 access unit once → FrameHub + Shared Frame Ring.
    pub(crate) fn publish_access_unit(&mut self, access_unit: Bytes) -> Result<(), ReceiverError> {
        self.ingress.access_units += 1;
        self.ingress.decode_invocations += 1;
        let decoded = match self
            .decoder
            .decode_access_unit(&access_unit, self.current_stream_config.as_ref())
        {
            Ok(decoded) => decoded,
            Err(error) => {
                self.stats_reporter.record_decoder_drop();
                self.last_media_error = Some(error.to_string());
                tracing::warn!("H.264 access unit decode failed: {error}");
                return Ok(());
            }
        };
        match decoded {
            Some(frame) => {
                // Prefer StreamConfig.rotation from Sender when present (PUC-005 / MEDIA-009).
                let rotation = self
                    .current_stream_config
                    .as_ref()
                    .map(|c| c.rotation)
                    .unwrap_or(frame.rotation);
                self.publish_nv12_frame(
                    frame.width,
                    frame.height,
                    frame.stride,
                    rotation,
                    frame.timestamp_us,
                    &frame.nv12,
                )?;
                self.ingress.decoded_frames += 1;
                self.last_media_error = None;
            }
            None => {
                self.stats_reporter.record_decoder_drop();
            }
        }
        Ok(())
    }

    fn publish_nv12_frame(
        &mut self,
        width: u32,
        height: u32,
        stride: u32,
        rotation: u32,
        timestamp_us: u64,
        nv12: &[u8],
    ) -> Result<(), ReceiverError> {
        // REQ-PICOO-MEDIA-009: rotate pixels to upright before FrameHub / Shared Ring / VCam.
        // REQ-PICOO-MEDIA-004: then apply remote StreamConfig.mirrored in upright space.
        let rotated_buf =
            picoo_frame_hub::nv12_rotate_clockwise(width, height, stride, rotation, nv12);
        let (width, height, stride, base_pixels): (u32, u32, u32, &[u8]) = match &rotated_buf {
            Some((ow, oh, os, buf)) => (*ow, *oh, *os, buf.as_slice()),
            None => (width, height, stride, nv12),
        };

        let mirrored = self
            .current_stream_config
            .as_ref()
            .is_some_and(|c| c.mirrored);
        let mirrored_owned = if mirrored {
            let mut buf = base_pixels.to_vec();
            picoo_frame_hub::nv12_mirror_horizontal(width, height, stride, &mut buf);
            Some(buf)
        } else {
            None
        };
        let pixels = mirrored_owned.as_deref().unwrap_or(base_pixels);

        // Pixels are upright after rotation; clear metadata so VCam does not double-rotate.
        let published_rotation = 0u32;

        let index = self.frame_hub.begin_write()?;
        self.frame_hub.commit_write(
            index,
            width,
            height,
            stride,
            published_rotation,
            timestamp_us,
            Bytes::copy_from_slice(pixels),
        );
        if let Some(ring) = self.shared_ring.as_mut() {
            ring.publish_nv12(
                width,
                height,
                stride,
                published_rotation,
                timestamp_us,
                pixels,
            )?;
        }
        Ok(())
    }

    pub fn latest_frame(&self) -> Option<&FrameSlot> {
        self.frame_hub.latest_ready()
    }
}
