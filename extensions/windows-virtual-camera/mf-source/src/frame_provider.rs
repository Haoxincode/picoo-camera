//! Shared Frame Ring consumer and disconnect placeholder policy.

use std::time::{Duration, Instant};

#[cfg(test)]
use picoo_frame_hub::{waiting_placeholder, PLACEHOLDER_HEIGHT, PLACEHOLDER_WIDTH};
use picoo_frame_hub::{
    waiting_placeholder_for_size, SharedFrameRingConsumer, DEFAULT_MAX_FRAME_BYTES,
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
    live_revision: u64,
    last_live: Option<OwnedNv12Frame>,
    last_live_at: Option<Instant>,
    output_cache: Option<OutputFrameCache>,
    next_generation_probe: Instant,
}

struct OutputFrameCache {
    live_revision: Option<u64>,
    width: u32,
    height: u32,
    frame: OwnedNv12Frame,
}

impl FrameProvider {
    pub fn new() -> Self {
        Self {
            ring_name: DEFAULT_RING_NAME.to_owned(),
            consumer: None,
            last_sequence: 0,
            live_revision: 0,
            last_live: None,
            last_live_at: None,
            output_cache: None,
            next_generation_probe: Instant::now(),
        }
    }

    #[cfg(test)]
    pub fn acquire(&mut self) -> AcquiredNv12Frame {
        let origin = self.refresh_source();
        let frame = match origin {
            FrameOrigin::Fresh | FrameOrigin::Cached => self
                .last_live
                .as_ref()
                .expect("live origin requires a complete frame")
                .clone(),
            FrameOrigin::Placeholder => OwnedNv12Frame {
                width: PLACEHOLDER_WIDTH,
                height: PLACEHOLDER_HEIGHT,
                stride: PLACEHOLDER_WIDTH,
                pixels: waiting_placeholder(),
            },
        };
        AcquiredNv12Frame { frame, origin }
    }

    fn refresh_source(&mut self) -> FrameOrigin {
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
                    self.live_revision = self.live_revision.wrapping_add(1);
                    self.last_live = Some(frame);
                    self.last_live_at = Some(Instant::now());
                    return FrameOrigin::Fresh;
                }
            }
        }

        if self.last_live.is_some()
            && self
                .last_live_at
                .is_some_and(|at| at.elapsed() < LAST_FRAME_HOLD)
        {
            return FrameOrigin::Cached;
        }
        FrameOrigin::Placeholder
    }

    /// Return a stable negotiated output shape. Producer dimensions never
    /// mutate the Media Foundation type during RequestSample.
    #[cfg_attr(not(windows), allow(dead_code))]
    pub fn acquire_for_output(&mut self, width: u32, height: u32) -> AcquiredNv12Frame {
        let origin = self.refresh_source();
        let revision = match origin {
            FrameOrigin::Fresh | FrameOrigin::Cached => Some(self.live_revision),
            FrameOrigin::Placeholder => None,
        };
        if let Some(cached) = &self.output_cache {
            if cached.live_revision == revision && cached.width == width && cached.height == height
            {
                return AcquiredNv12Frame {
                    frame: cached.frame.clone(),
                    origin,
                };
            }
        }

        let frame = if origin == FrameOrigin::Placeholder {
            OwnedNv12Frame {
                width,
                height,
                stride: width,
                pixels: waiting_placeholder_for_size(width, height),
            }
        } else {
            fit_nv12(
                self.last_live
                    .as_ref()
                    .expect("live origin requires a complete frame"),
                width,
                height,
            )
            .unwrap_or_else(|| OwnedNv12Frame {
                width,
                height,
                stride: width,
                pixels: picoo_frame_hub::nv12_black(width, height),
            })
        };
        self.output_cache = Some(OutputFrameCache {
            live_revision: revision,
            width,
            height,
            frame: frame.clone(),
        });
        AcquiredNv12Frame { frame, origin }
    }

    #[cfg(test)]
    fn with_ring_name(ring_name: String) -> Self {
        Self {
            ring_name,
            consumer: None,
            last_sequence: 0,
            live_revision: 0,
            last_live: None,
            last_live_at: None,
            output_cache: None,
            next_generation_probe: Instant::now(),
        }
    }
}

fn fit_nv12(
    frame: &OwnedNv12Frame,
    output_width: u32,
    output_height: u32,
) -> Option<OwnedNv12Frame> {
    if frame.width == output_width && frame.height == output_height && frame.stride == frame.width {
        return Some(frame.clone());
    }
    if frame.width < 2
        || frame.height < 2
        || output_width < 2
        || output_height < 2
        || !frame.width.is_multiple_of(2)
        || !frame.height.is_multiple_of(2)
        || !output_width.is_multiple_of(2)
        || !output_height.is_multiple_of(2)
        || frame.stride < frame.width
        || frame.pixels.len() < (frame.stride as usize * frame.height as usize * 3 / 2)
    {
        return None;
    }

    let source_wider = u64::from(frame.width) * u64::from(output_height)
        > u64::from(output_width) * u64::from(frame.height);
    let (fit_width, fit_height) = if source_wider {
        let height =
            (u64::from(output_width) * u64::from(frame.height) / u64::from(frame.width)) as u32;
        (output_width, height.max(2) & !1)
    } else {
        let width =
            (u64::from(output_height) * u64::from(frame.width) / u64::from(frame.height)) as u32;
        (width.max(2) & !1, output_height)
    };
    let offset_x = ((output_width - fit_width) / 2) & !1;
    let offset_y = ((output_height - fit_height) / 2) & !1;
    let mut pixels = picoo_frame_hub::nv12_black(output_width, output_height);
    let src_stride = frame.stride as usize;
    let dst_stride = output_width as usize;
    let src_y_len = src_stride * frame.height as usize;
    let dst_y_len = dst_stride * output_height as usize;

    for dy in 0..fit_height {
        let sy = (u64::from(dy) * u64::from(frame.height) / u64::from(fit_height)) as usize;
        for dx in 0..fit_width {
            let sx = (u64::from(dx) * u64::from(frame.width) / u64::from(fit_width)) as usize;
            let destination = (offset_y + dy) as usize * dst_stride + (offset_x + dx) as usize;
            pixels[destination] = frame.pixels[sy * src_stride + sx];
        }
    }

    for dy in (0..fit_height).step_by(2) {
        let sy = (u64::from(dy) * u64::from(frame.height) / u64::from(fit_height)) as usize & !1;
        for dx in (0..fit_width).step_by(2) {
            let sx = (u64::from(dx) * u64::from(frame.width) / u64::from(fit_width)) as usize & !1;
            let source = src_y_len + (sy / 2) * src_stride + sx;
            let destination =
                dst_y_len + ((offset_y + dy) / 2) as usize * dst_stride + (offset_x + dx) as usize;
            pixels[destination] = frame.pixels[source];
            pixels[destination + 1] = frame.pixels[source + 1];
        }
    }

    Some(OwnedNv12Frame {
        width: output_width,
        height: output_height,
        stride: output_width,
        pixels,
    })
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

        let negotiated = provider.acquire_for_output(1920, 1080);
        assert_eq!(negotiated.origin, FrameOrigin::Placeholder);
        assert_eq!(
            (negotiated.frame.width, negotiated.frame.height),
            (1920, 1080)
        );
        assert_eq!(
            negotiated.frame.pixels,
            waiting_placeholder_for_size(1920, 1080)
        );
        let cached_pixels = provider
            .output_cache
            .as_ref()
            .expect("negotiated placeholder cache")
            .frame
            .pixels
            .as_ptr();
        let repeated = provider.acquire_for_output(1920, 1080);
        assert_eq!(repeated.origin, FrameOrigin::Placeholder);
        assert_eq!(
            provider
                .output_cache
                .as_ref()
                .expect("reused placeholder cache")
                .frame
                .pixels
                .as_ptr(),
            cached_pixels,
            "unchanged placeholder requests must not redraw the frame"
        );
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

    #[test]
    fn negotiated_output_shape_is_stable_and_letterboxes_input() {
        let source = OwnedNv12Frame {
            width: 4,
            height: 4,
            stride: 4,
            pixels: {
                let mut pixels = vec![80; nv12_len(4, 4).expect("source size")];
                pixels[16..].fill(128);
                pixels
            },
        };
        let fitted = fit_nv12(&source, 8, 4).expect("fit");
        assert_eq!((fitted.width, fitted.height, fitted.stride), (8, 4, 8));
        assert_eq!(fitted.pixels[0], 0, "left pillar is black");
        assert_eq!(fitted.pixels[2], 80, "source is centered");
        assert_eq!(fitted.pixels[7], 0, "right pillar is black");
    }
}
