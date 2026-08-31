//! Shared Frame Ring consumer and disconnect placeholder policy.

use std::time::{Duration, Instant};

use picoo_frame_hub::{
    waiting_placeholder, SharedFrameRingConsumer, DEFAULT_MAX_FRAME_BYTES, PLACEHOLDER_HEIGHT,
    PLACEHOLDER_WIDTH,
};

use crate::{format::nv12_len, DEFAULT_RING_NAME};

const LAST_FRAME_HOLD: Duration = Duration::from_millis(500);
const GENERATION_PROBE_INTERVAL: Duration = Duration::from_millis(250);

#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) struct OwnedNv12Frame {
    pub width: u32,
    pub height: u32,
    pub stride: u32,
    pub pixels: Vec<u8>,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum FrameOrigin {
    Fresh,
    Cached,
    Placeholder,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) struct AcquiredNv12Frame {
    pub frame: OwnedNv12Frame,
    pub origin: FrameOrigin,
}

pub(crate) struct FrameProvider {
    ring_name: String,
    consumer: Option<SharedFrameRingConsumer>,
    last_sequence: u64,
    last_live: Option<OwnedNv12Frame>,
    last_live_at: Option<Instant>,
    next_generation_probe: Instant,
}

impl FrameProvider {
    pub fn new() -> Self {
        Self {
            ring_name: DEFAULT_RING_NAME.to_owned(),
            consumer: None,
            last_sequence: 0,
            last_live: None,
            last_live_at: None,
            next_generation_probe: Instant::now(),
        }
    }

    pub fn acquire(&mut self) -> AcquiredNv12Frame {
        let now = Instant::now();
        if now >= self.next_generation_probe {
            self.next_generation_probe = now + GENERATION_PROBE_INTERVAL;
            if self
                .consumer
                .as_ref()
                .is_some_and(|consumer| !consumer.is_current_generation())
            {
                // REQ-PICOO-FRAME-007: a new Receiver mapping starts its
                // sequence at one, so detach must reset deduplication.
                self.consumer = None;
                self.last_sequence = 0;
            }
            if self.consumer.is_none() {
                self.consumer =
                    SharedFrameRingConsumer::open(&self.ring_name, DEFAULT_MAX_FRAME_BYTES).ok();
            }
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
                    return AcquiredNv12Frame {
                        frame,
                        origin: FrameOrigin::Fresh,
                    };
                }
            }
        }

        if let (Some(frame), Some(at)) = (&self.last_live, self.last_live_at) {
            if at.elapsed() < LAST_FRAME_HOLD {
                return AcquiredNv12Frame {
                    frame: frame.clone(),
                    origin: FrameOrigin::Cached,
                };
            }
        }

        AcquiredNv12Frame {
            frame: OwnedNv12Frame {
                width: PLACEHOLDER_WIDTH,
                height: PLACEHOLDER_HEIGHT,
                stride: PLACEHOLDER_WIDTH,
                pixels: waiting_placeholder(),
            },
            origin: FrameOrigin::Placeholder,
        }
    }

    #[cfg(test)]
    fn with_ring_name(ring_name: String) -> Self {
        Self {
            ring_name,
            consumer: None,
            last_sequence: 0,
            last_live: None,
            last_live_at: None,
            next_generation_probe: Instant::now(),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use picoo_frame_hub::SharedFrameRingProducer;

    fn test_ring_name() -> String {
        format!(
            "frame-provider-{}",
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .expect("clock")
                .as_nanos()
        )
    }

    #[test]
    fn starts_with_shared_branded_placeholder() {
        let mut provider = FrameProvider::new();
        let acquired = provider.acquire();
        let frame = acquired.frame;
        assert_eq!(acquired.origin, FrameOrigin::Placeholder);
        assert_eq!((frame.width, frame.height), (1280, 720));
        assert_eq!(frame.pixels, waiting_placeholder());
    }

    #[test]
    fn reconnects_to_new_mapping_generation_even_when_sequence_restarts() {
        let ring_name = test_ring_name();
        let frame_len = nv12_len(PLACEHOLDER_WIDTH, PLACEHOLDER_HEIGHT).expect("NV12 size");
        let first_pixels = vec![1; frame_len];
        let second_pixels = vec![2; frame_len];
        let mut first_producer =
            SharedFrameRingProducer::create(&ring_name, DEFAULT_MAX_FRAME_BYTES)
                .expect("first producer");
        first_producer
            .publish_nv12(
                PLACEHOLDER_WIDTH,
                PLACEHOLDER_HEIGHT,
                PLACEHOLDER_WIDTH,
                0,
                1,
                &first_pixels,
            )
            .expect("first frame");
        let mut provider = FrameProvider::with_ring_name(ring_name.clone());
        let acquired = provider.acquire();
        assert_eq!(acquired.origin, FrameOrigin::Fresh);
        assert_eq!(acquired.frame.pixels, first_pixels);

        drop(first_producer);
        provider.next_generation_probe = Instant::now();
        assert_eq!(
            provider.acquire().frame.pixels,
            first_pixels,
            "brief generation gap keeps the last complete frame"
        );
        provider.last_live_at = Some(Instant::now() - LAST_FRAME_HOLD);
        assert_eq!(
            provider.acquire().frame.pixels,
            waiting_placeholder(),
            "an extended generation gap falls back to the placeholder"
        );

        let mut second_producer =
            SharedFrameRingProducer::create(&ring_name, DEFAULT_MAX_FRAME_BYTES)
                .expect("second producer");
        second_producer
            .publish_nv12(
                PLACEHOLDER_WIDTH,
                PLACEHOLDER_HEIGHT,
                PLACEHOLDER_WIDTH,
                0,
                2,
                &second_pixels,
            )
            .expect("second generation frame");

        provider.next_generation_probe = Instant::now();
        let acquired = provider.acquire();
        assert_eq!(acquired.origin, FrameOrigin::Fresh);
        assert_eq!(acquired.frame.pixels, second_pixels);

        drop((provider, second_producer));
        let _ = std::fs::remove_file(SharedFrameRingProducer::flink_path(&ring_name));
    }
}
