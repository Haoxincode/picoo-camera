//! Native source publication; no pixel map, transform, or export on owner.

use super::{media_publish::FrameTimeline, ReceiverSession};
use crate::ReceiverError;
use picoo_frame_hub::{
    FrameDescription, FrameIdentity, NativeVideoFrame, PresentationTransform, Rotation, SourceColor,
};
use picoo_media_decode::DecodedFrame;
use std::time::Instant;

impl ReceiverSession {
    pub(super) fn publish_decoded_frame(
        &mut self,
        timeline: FrameTimeline,
        frame: DecodedFrame,
        mirrored: bool,
    ) -> Result<(), ReceiverError> {
        let description = frame.description();
        let image = frame.into_native_image();
        let rotation = match description.rotation {
            0 => Rotation::None,
            90 => Rotation::Clockwise90,
            180 => Rotation::Clockwise180,
            270 => Rotation::Clockwise270,
            _ => {
                return Err(ReceiverError::Protocol(
                    "invalid native frame rotation".into(),
                ))
            }
        };
        let frame = NativeVideoFrame::new(
            FrameIdentity {
                connection_generation: timeline.connection_generation,
                stream_epoch: timeline.stream_generation,
                decoder_generation: timeline.decoder_generation,
                frame_id: timeline.frame_id,
            },
            timeline.source_pts_us,
            FrameDescription {
                coded_size: description.native_format.coded_size,
                visible_rect: description.native_format.visible_rect,
                pixel_aspect_ratio: description.native_format.pixel_aspect_ratio,
                color: SourceColor::Nv12Bt709Limited {
                    chroma_siting: description.native_format.chroma_siting,
                },
                transform: PresentationTransform {
                    rotation,
                    mirror: mirrored,
                },
                config_revision: timeline.config_revision,
            },
            image,
            picoo_frame_hub::FrameTimeline {
                encoded_at_us: timeline.encoded_at_us,
                received_at_us: timeline.received_at_us,
                decode_submitted_at_us: timeline.decode_submitted_at_us,
                decoded_at: timeline.decoded_at.unwrap_or_else(Instant::now),
            },
        )
        .map_err(|error| ReceiverError::Protocol(error.to_string()))?;
        if let Some(reason) = self.frames.publish(frame) {
            tracing::warn!(?reason, "native recording subscription ended");
        }
        if let Some(output) = &self.shared_ring {
            output.submit(
                self.frames
                    .latest()
                    .expect("published native frame")
                    .clone(),
            );
        }
        Ok(())
    }

    pub fn publish_waiting_placeholder(&mut self) -> Result<(), ReceiverError> {
        self.frames.clear();
        if let Some(output) = &self.shared_ring {
            output.placeholder(self.placeholder_mode, false);
        }
        Ok(())
    }

    pub fn publish_reconnecting_placeholder(&mut self) -> Result<(), ReceiverError> {
        self.frames.clear();
        if let Some(output) = &self.shared_ring {
            output.placeholder(self.placeholder_mode, true);
        }
        Ok(())
    }
}
