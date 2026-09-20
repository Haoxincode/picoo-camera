//! Header-only observation of live/placeholder state for GpuNative delivery.

use std::time::Instant;

use picoo_frame_hub::{SharedFrameKind, SharedFrameRingConsumer};

use super::{open_consumer, GENERATION_PROBE_INTERVAL, LAST_FRAME_HOLD};

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) struct LiveContentToken {
    mapping_epoch: u64,
    content_signal: u64,
}

pub(super) struct RingContentObserver {
    ring_name: String,
    consumer: Option<SharedFrameRingConsumer>,
    mapping_epoch: u64,
    next_generation_probe: Instant,
    disconnected_at: Option<Instant>,
    producer_alive: bool,
    had_live: bool,
}

impl RingContentObserver {
    pub(super) fn new(ring_name: String) -> Self {
        Self {
            ring_name,
            consumer: None,
            mapping_epoch: 0,
            next_generation_probe: Instant::now(),
            disconnected_at: None,
            producer_alive: false,
            had_live: false,
        }
    }

    pub(super) fn kind(&mut self) -> SharedFrameKind {
        let now = Instant::now();
        self.probe(now, false);
        self.observed_kind(now)
    }

    pub(super) fn live_token(&mut self) -> Option<LiveContentToken> {
        let now = Instant::now();
        // Final sample admission must observe a replaced mapping immediately;
        // a restarted producer can reuse the same in-mapping signal revision.
        self.probe(now, true);
        (self.observed_kind(now) == SharedFrameKind::Live)
            .then(|| {
                self.consumer
                    .as_ref()
                    .and_then(SharedFrameRingConsumer::content_signal)
            })
            .flatten()
            .filter(|signal| signal & 1 == SharedFrameKind::Live as u64)
            .map(|content_signal| LiveContentToken {
                mapping_epoch: self.mapping_epoch,
                content_signal,
            })
    }

    fn probe(&mut self, now: Instant, force_identity_check: bool) {
        if !force_identity_check && now < self.next_generation_probe {
            return;
        }
        self.next_generation_probe = now + GENERATION_PROBE_INTERVAL;

        if self
            .consumer
            .as_ref()
            .is_some_and(|consumer| !consumer.is_current_generation())
        {
            self.consumer = None;
            self.producer_alive = false;
            self.disconnected_at = None;
            self.had_live = false;
        }
        if self.consumer.is_none() {
            if let Ok(consumer) = open_consumer(&self.ring_name) {
                let Some(epoch) = self.mapping_epoch.checked_add(1) else {
                    return;
                };
                self.mapping_epoch = epoch;
                self.consumer = Some(consumer);
            }
        }
        self.producer_alive = self.consumer.as_ref().is_some_and(|consumer| {
            #[cfg(windows)]
            {
                consumer.has_live_producer()
            }
            #[cfg(not(windows))]
            {
                let _ = consumer;
                true
            }
        });
        if self.producer_alive {
            self.disconnected_at = None;
        } else if self.had_live && self.consumer.is_some() {
            self.disconnected_at.get_or_insert(now);
        }
    }

    fn observed_kind(&mut self, now: Instant) -> SharedFrameKind {
        let Some(kind) = self
            .consumer
            .as_ref()
            .and_then(SharedFrameRingConsumer::content_kind)
        else {
            return SharedFrameKind::Placeholder;
        };
        if kind == SharedFrameKind::Placeholder {
            self.had_live = false;
            return SharedFrameKind::Placeholder;
        }
        if self.producer_alive {
            self.had_live = true;
            return SharedFrameKind::Live;
        }
        if self.had_live
            && self
                .disconnected_at
                .is_some_and(|at| now.duration_since(at) < LAST_FRAME_HOLD)
        {
            SharedFrameKind::Live
        } else {
            SharedFrameKind::Placeholder
        }
    }

    #[cfg(test)]
    pub(super) fn force_probe(&mut self) {
        self.next_generation_probe = Instant::now();
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use picoo_frame_hub::{SharedFrameRingProducer, DEFAULT_MAX_FRAME_BYTES};
    use std::time::{Duration, SystemTime, UNIX_EPOCH};

    fn ring_name() -> String {
        format!(
            "content-observer-{}",
            SystemTime::now()
                .duration_since(UNIX_EPOCH)
                .expect("clock")
                .as_nanos()
        )
    }

    #[test]
    fn missing_initial_producer_is_placeholder_without_a_live_hold() {
        let mut observer = RingContentObserver::new(ring_name());
        assert_eq!(observer.kind(), SharedFrameKind::Placeholder);
        assert_eq!(observer.live_token(), None);
    }

    #[test]
    fn a_seen_live_generation_gets_only_the_bounded_disconnect_hold() {
        let name = ring_name();
        let producer =
            SharedFrameRingProducer::create(&name, DEFAULT_MAX_FRAME_BYTES).expect("producer");
        let mut observer = RingContentObserver::new(name.clone());
        assert_eq!(observer.kind(), SharedFrameKind::Live);

        observer.producer_alive = false;
        observer.disconnected_at = Some(Instant::now());
        observer.next_generation_probe = Instant::now() + Duration::from_secs(1);
        assert_eq!(observer.kind(), SharedFrameKind::Live);

        observer.disconnected_at = Some(Instant::now() - LAST_FRAME_HOLD);
        assert_eq!(observer.kind(), SharedFrameKind::Placeholder);

        drop((observer, producer));
        let _ = std::fs::remove_file(SharedFrameRingProducer::flink_path(&name));
    }

    #[test]
    fn replacement_mapping_never_reuses_an_equal_live_admission_token() {
        let name = ring_name();
        let first = SharedFrameRingProducer::create(&name, DEFAULT_MAX_FRAME_BYTES)
            .expect("first producer");
        let mut observer = RingContentObserver::new(name.clone());
        let old = observer.live_token().expect("first live token");
        drop(first);

        let second = SharedFrameRingProducer::create(&name, DEFAULT_MAX_FRAME_BYTES)
            .expect("replacement producer");
        observer.force_probe();
        let replacement = observer.live_token().expect("replacement live token");
        assert_ne!(replacement, old);

        drop((observer, second));
        let _ = std::fs::remove_file(SharedFrameRingProducer::flink_path(&name));
    }
}
