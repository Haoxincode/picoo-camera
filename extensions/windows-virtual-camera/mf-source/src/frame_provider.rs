//! Shared Frame Ring consumer and disconnect placeholder policy.

use std::time::{Duration, Instant};

use picoo_frame_hub::{
    waiting_placeholder, SharedFrameRingConsumer, DEFAULT_MAX_FRAME_BYTES, PLACEHOLDER_HEIGHT,
    PLACEHOLDER_WIDTH,
};

use crate::{format::nv12_len, DEFAULT_RING_NAME};

const LAST_FRAME_HOLD: Duration = Duration::from_millis(500);

#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) struct OwnedNv12Frame {
    pub width: u32,
    pub height: u32,
    pub stride: u32,
    pub pixels: Vec<u8>,
}

pub(crate) struct FrameProvider {
    consumer: Option<SharedFrameRingConsumer>,
    last_sequence: u64,
    last_live: Option<OwnedNv12Frame>,
    last_live_at: Option<Instant>,
}

impl FrameProvider {
    pub fn new() -> Self {
        Self {
            consumer: None,
            last_sequence: 0,
            last_live: None,
            last_live_at: None,
        }
    }

    pub fn acquire(&mut self) -> OwnedNv12Frame {
        if self.consumer.is_none() {
            self.consumer =
                SharedFrameRingConsumer::open(DEFAULT_RING_NAME, DEFAULT_MAX_FRAME_BYTES).ok();
        }

        if let Some(consumer) = &self.consumer {
            if let Some(view) = consumer.latest_frame() {
                let expected = nv12_len(view.width, view.height);
                let valid = view.sequence != self.last_sequence
                    && view.stride == view.width
                    && expected == Some(view.nv12.len());
                if valid {
                    let frame = OwnedNv12Frame {
                        width: view.width,
                        height: view.height,
                        stride: view.stride,
                        pixels: view.nv12.to_vec(),
                    };
                    self.last_sequence = view.sequence;
                    self.last_live = Some(frame.clone());
                    self.last_live_at = Some(Instant::now());
                    return frame;
                }
            }
        }

        if let (Some(frame), Some(at)) = (&self.last_live, self.last_live_at) {
            if at.elapsed() < LAST_FRAME_HOLD {
                return frame.clone();
            }
        }

        OwnedNv12Frame {
            width: PLACEHOLDER_WIDTH,
            height: PLACEHOLDER_HEIGHT,
            stride: PLACEHOLDER_WIDTH,
            pixels: waiting_placeholder(),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn starts_with_shared_branded_placeholder() {
        let mut provider = FrameProvider::new();
        let frame = provider.acquire();
        assert_eq!((frame.width, frame.height), (1280, 720));
        assert_eq!(frame.pixels, waiting_placeholder());
    }
}
