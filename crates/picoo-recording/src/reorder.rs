//! Recorder-owned ordering, independent of live recovery — REQ-PICOO-MEDIA-070.
use crate::{ingress::RecordingInput, RecordingError};
use std::{
    collections::BTreeMap,
    time::{Duration, Instant},
};

const CAPACITY: usize = 16;
const HOLD: Duration = Duration::from_millis(50);
struct Pending {
    input: RecordingInput,
    arrived_at: Instant,
}

#[derive(Default)]
pub struct RecordingReorder {
    source: Option<(u64, u32)>,
    last_id: Option<u64>,
    pending: BTreeMap<u64, Pending>,
}

impl RecordingReorder {
    pub fn new() -> Self {
        Self::default()
    }

    /// Returns old-source entries before any new-source entries. A caller must
    /// write this batch in order before polling again. Errors terminate recording.
    pub fn push(
        &mut self,
        input: RecordingInput,
        now: Instant,
    ) -> Result<Vec<RecordingInput>, RecordingError> {
        let source = (input.connection_generation, input.access_unit.stream_epoch);
        if self.source.is_some_and(|current| source < current) {
            return Err(RecordingError::InvalidInput(
                "recording source moved backward",
            ));
        }
        let mut ready = Vec::new();
        if self.source != Some(source) {
            ready = self.drain();
            self.source = Some(source);
            self.last_id = None;
        }
        let id = input.access_unit.frame_id;
        if self.last_id.is_some_and(|last| id <= last) || self.pending.contains_key(&id) {
            return Err(RecordingError::InvalidInput(
                "duplicate or late recording AU",
            ));
        }
        // Do not release a batch and then return an error: that would silently
        // discard accepted complete AUs. Capacity failure terminates the owner.
        if self.pending.len() == CAPACITY {
            return Err(RecordingError::InvalidInput(
                "recording reorder capacity reached",
            ));
        }
        self.pending.insert(
            id,
            Pending {
                input,
                arrived_at: now,
            },
        );
        ready.extend(self.poll(now));
        Ok(ready)
    }

    pub fn poll(&mut self, now: Instant) -> Vec<RecordingInput> {
        let mut ready = Vec::new();
        while let Some((&first, _)) = self.pending.first_key_value() {
            let expired = self
                .pending
                .values()
                .any(|entry| now.saturating_duration_since(entry.arrived_at) >= HOLD);
            if self.last_id.and_then(|last| last.checked_add(1)) != Some(first) && !expired {
                break;
            }
            let entry = self.pending.pop_first().expect("first pending AU").1;
            self.last_id = Some(first);
            ready.push(entry.input);
        }
        ready
    }

    /// Normal stop/source change preserves every accepted complete AU. Missing
    /// IDs remain visible to the segment state machine; no synthetic AU is added.
    pub fn drain(&mut self) -> Vec<RecordingInput> {
        let pending = std::mem::take(&mut self.pending);
        let mut ready = Vec::with_capacity(pending.len());
        for (id, entry) in pending {
            self.last_id = Some(id);
            ready.push(entry.input);
        }
        ready
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use picoo_packet::AssembledAccessUnit;
    use picoo_protocol::control::StreamConfig;
    use std::sync::Arc;

    fn input(epoch: u32, id: u64) -> RecordingInput {
        RecordingInput {
            _reservation: crate::budget::reserve(1).unwrap(),
            connection_generation: 1,
            configuration: Arc::new(StreamConfig {
                stream_epoch: epoch,
                ..Default::default()
            }),
            access_unit: AssembledAccessUnit {
                data: vec![1].into(),
                frame_id: id,
                pts_us: id,
                encoded_at_us: id,
                keyframe: false,
                discardable: false,
                stream_epoch: epoch,
                fragment_count: 1,
                first_fragment_at: Instant::now(),
            },
        }
    }
    fn ids(batch: Vec<RecordingInput>) -> Vec<u64> {
        batch
            .into_iter()
            .map(|input| input.access_unit.frame_id)
            .collect()
    }

    #[test]
    fn initial_and_later_reordering_wait_for_missing_ids() {
        let now = Instant::now();
        let mut reorder = RecordingReorder::new();
        assert!(reorder.push(input(1, 2), now).unwrap().is_empty());
        assert!(reorder.push(input(1, 1), now).unwrap().is_empty());
        assert_eq!(ids(reorder.poll(now + HOLD)), vec![1, 2]);
        assert!(reorder.push(input(1, 4), now + HOLD).unwrap().is_empty());
        assert_eq!(
            ids(reorder.push(input(1, 3), now + HOLD).unwrap()),
            vec![3, 4]
        );
    }

    #[test]
    fn gap_deadline_is_not_extended_by_later_input_or_polling() {
        let now = Instant::now();
        let mut reorder = RecordingReorder::new();
        reorder.push(input(1, 1), now).unwrap();
        assert_eq!(ids(reorder.poll(now + HOLD)), vec![1]);
        reorder.push(input(1, 3), now + HOLD).unwrap();
        reorder
            .push(input(1, 4), now + HOLD + Duration::from_millis(40))
            .unwrap();
        assert!(reorder
            .poll(now + HOLD + Duration::from_millis(49))
            .is_empty());
        assert_eq!(ids(reorder.poll(now + HOLD * 2)), vec![3, 4]);
    }

    #[test]
    fn generation_change_and_stop_drain_all_accepted_complete_aus() {
        let now = Instant::now();
        let mut reorder = RecordingReorder::new();
        reorder.push(input(1, 3), now).unwrap();
        reorder.push(input(1, 1), now).unwrap();
        let old = reorder.push(input(2, 1), now).unwrap();
        assert!(old.iter().all(|input| input.access_unit.stream_epoch == 1));
        assert_eq!(ids(old), vec![1, 3]);
        assert_eq!(ids(reorder.drain()), vec![1]);
    }

    #[test]
    fn capacity_duplicate_and_old_source_fail_without_clearing_pending() {
        let now = Instant::now();
        let mut reorder = RecordingReorder::new();
        for id in 0..CAPACITY {
            reorder.push(input(2, id as u64), now).unwrap();
        }
        assert!(reorder.push(input(2, 20), now).is_err());
        assert!(reorder.push(input(2, 1), now).is_err());
        assert!(reorder.push(input(1, 30), now).is_err());
        assert_eq!(reorder.drain().len(), CAPACITY);
    }
}
