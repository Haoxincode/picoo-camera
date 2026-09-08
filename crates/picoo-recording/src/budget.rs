//! One compressed-input budget across recording stages — REQ-PICOO-MEDIA-077.
use crate::ingress::IngressFailure;
use std::sync::{
    atomic::{AtomicUsize, Ordering},
    Arc, OnceLock,
};

pub(crate) const MAX_CONFIGURATION_BYTES: usize = 64 * 1024;
const LIMIT: usize = 16 * 1024 * 1024;

#[derive(Debug)]
struct Budget {
    used: AtomicUsize,
    limit: usize,
}

#[derive(Debug)]
pub(crate) struct Reservation {
    budget: Arc<Budget>,
    bytes: usize,
}

impl Budget {
    fn reserve(self: &Arc<Self>, bytes: usize) -> Result<Reservation, IngressFailure> {
        self.used
            .fetch_update(Ordering::AcqRel, Ordering::Acquire, |used| {
                used.checked_add(bytes).filter(|total| *total <= self.limit)
            })
            .map_err(|_| IngressFailure::Capacity)?;
        Ok(Reservation {
            budget: self.clone(),
            bytes,
        })
    }
}

pub(crate) fn reserve(payload_bytes: usize) -> Result<Reservation, IngressFailure> {
    static SHARED: OnceLock<Arc<Budget>> = OnceLock::new();
    let bytes = payload_bytes
        .checked_add(MAX_CONFIGURATION_BYTES)
        .ok_or(IngressFailure::Capacity)?;
    SHARED
        .get_or_init(|| {
            Arc::new(Budget {
                used: AtomicUsize::new(0),
                limit: LIMIT,
            })
        })
        .reserve(bytes)
}

impl Drop for Reservation {
    fn drop(&mut self) {
        self.budget.used.fetch_sub(self.bytes, Ordering::AcqRel);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn channel_reorder_and_drained_batch_keep_the_same_reservations() {
        use crate::{
            ingress::{self, IngressPoll, RecordingInput},
            reorder::RecordingReorder,
        };
        use picoo_packet::AssembledAccessUnit;
        use picoo_protocol::control::StreamConfig;
        use std::time::Instant;

        let bytes = 1 + MAX_CONFIGURATION_BYTES;
        let budget = Arc::new(Budget {
            used: AtomicUsize::new(0),
            limit: 2 * bytes,
        });
        let input = |id| RecordingInput {
            connection_generation: 1,
            configuration: Arc::new(StreamConfig {
                codec_configuration: vec![1],
                stream_epoch: 1,
                ..Default::default()
            }),
            access_unit: AssembledAccessUnit {
                data: vec![1].into(),
                frame_id: id,
                pts_us: id,
                encoded_at_us: id,
                keyframe: false,
                discardable: false,
                stream_epoch: 1,
                fragment_count: 1,
                first_fragment_at: Instant::now(),
            },
            _reservation: budget.reserve(bytes).unwrap(),
        };
        let (sender, mut inbox) = ingress::channel();
        sender.offer(input(1)).unwrap();
        let IngressPoll::Input(first) = inbox.poll().unwrap() else {
            panic!()
        };
        let mut reorder = RecordingReorder::new();
        assert!(reorder.push(first, Instant::now()).unwrap().is_empty());
        sender.offer(input(2)).unwrap();
        assert_eq!(budget.reserve(1).unwrap_err(), IngressFailure::Capacity);
        let writing = reorder.drain();
        assert_eq!(budget.used.load(Ordering::Acquire), 2 * bytes);
        std::thread::spawn(move || drop(writing)).join().unwrap();
        assert_eq!(budget.used.load(Ordering::Acquire), bytes);
        sender.terminate(IngressFailure::Capacity);
        assert_eq!(inbox.poll().unwrap_err(), IngressFailure::Capacity);
        assert_eq!(budget.used.load(Ordering::Acquire), 0);
    }

    #[test]
    fn queued_and_in_flight_ownership_share_a_limit_until_cross_thread_release() {
        let budget = Arc::new(Budget {
            used: AtomicUsize::new(0),
            limit: 10,
        });
        let awaiting_config = budget.reserve(3).unwrap();
        let queued = budget.reserve(4).unwrap();
        let in_flight = budget.reserve(3).unwrap();
        assert_eq!(budget.reserve(1).unwrap_err(), IngressFailure::Capacity);
        assert_eq!(
            budget.reserve(usize::MAX).unwrap_err(),
            IngressFailure::Capacity
        );
        let moved_to_reorder = awaiting_config;
        assert_eq!(budget.used.load(Ordering::Acquire), 10);
        std::thread::spawn(move || drop(in_flight)).join().unwrap();
        let replacement = budget.reserve(3).unwrap();
        assert_eq!(budget.reserve(1).unwrap_err(), IngressFailure::Capacity);
        drop((moved_to_reorder, queued, replacement));
        assert_eq!(budget.used.load(Ordering::Acquire), 0);
        assert!(budget.reserve(10).is_ok());
    }
}
