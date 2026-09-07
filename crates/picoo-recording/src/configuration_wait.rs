//! Bounded late-configuration gate — REQ-PICOO-MEDIA-073.
//! Standard VecDeque owns complete AUs independently of live recovery storage.
use crate::ingress::{IngressFailure, RecordingInput};
use picoo_packet::AssembledAccessUnit;
use picoo_protocol::control::StreamConfig;
use std::{
    collections::VecDeque,
    sync::Arc,
    time::{Duration, Instant},
};

const CAPACITY: usize = 16;
const DEADLINE: Duration = Duration::from_millis(250);
struct Pending {
    generation: u64,
    au: AssembledAccessUnit,
    configuration: Option<Arc<StreamConfig>>,
    accepted_at: Instant,
}

#[derive(Default)]
pub struct ConfigurationWait {
    pending: VecDeque<Pending>,
}
impl ConfigurationWait {
    pub fn push(
        &mut self,
        generation: u64,
        au: AssembledAccessUnit,
        configuration: &Arc<StreamConfig>,
        now: Instant,
    ) -> Result<(), IngressFailure> {
        if self.pending.len() == CAPACITY {
            return Err(IngressFailure::Capacity);
        }
        if au.data.len() > picoo_protocol::MAX_MEDIA_ACCESS_UNIT_BYTES as usize
            || configuration.codec_configuration.len() > 64 * 1024
        {
            return Err(IngressFailure::InvalidInput);
        }
        let configuration =
            (au.stream_epoch == configuration.stream_epoch).then(|| Arc::clone(configuration));
        self.pending.push_back(Pending {
            generation,
            au,
            configuration,
            accepted_at: now,
        });
        Ok(())
    }

    pub fn resolve(
        &mut self,
        generation: u64,
        configuration: &Arc<StreamConfig>,
        now: Instant,
    ) -> Result<Vec<RecordingInput>, IngressFailure> {
        if self
            .pending
            .iter()
            .any(|entry| now.saturating_duration_since(entry.accepted_at) > DEADLINE)
        {
            return Err(IngressFailure::ConfigurationUnavailable);
        }
        for entry in &mut self.pending {
            if entry.generation == generation && entry.au.stream_epoch == configuration.stream_epoch
            {
                entry
                    .configuration
                    .get_or_insert_with(|| Arc::clone(configuration));
            }
        }
        let mut ready = Vec::new();
        while self
            .pending
            .front()
            .is_some_and(|entry| entry.configuration.is_some())
        {
            let entry = self.pending.pop_front().expect("ready entry");
            ready.push(RecordingInput {
                connection_generation: entry.generation,
                configuration: entry.configuration.expect("ready configuration"),
                access_unit: entry.au,
            });
        }
        Ok(ready)
    }

    pub fn is_empty(&self) -> bool {
        self.pending.is_empty()
    }
    pub fn clear(&mut self) {
        self.pending.clear();
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    fn config(epoch: u32) -> Arc<StreamConfig> {
        Arc::new(StreamConfig {
            stream_epoch: epoch,
            ..Default::default()
        })
    }
    fn au(epoch: u32, id: u64) -> AssembledAccessUnit {
        AssembledAccessUnit {
            data: vec![1].into(),
            frame_id: id,
            pts_us: id,
            encoded_at_us: id,
            keyframe: false,
            discardable: false,
            stream_epoch: epoch,
            fragment_count: 1,
            first_fragment_at: Instant::now(),
        }
    }
    #[test]
    fn late_configuration_releases_all_complete_aus_with_original_identity() {
        let now = Instant::now();
        let mut gate = ConfigurationWait::default();
        gate.push(7, au(2, 2), &config(1), now).unwrap();
        gate.push(7, au(2, 1), &config(1), now).unwrap();
        assert!(gate.resolve(8, &config(2), now).unwrap().is_empty());
        let ready = gate.resolve(7, &config(2), now).unwrap();
        assert_eq!(
            ready
                .iter()
                .map(|entry| entry.access_unit.frame_id)
                .collect::<Vec<_>>(),
            vec![2, 1]
        );
        assert!(
            ready
                .iter()
                .all(|entry| entry.configuration.stream_epoch == 2
                    && entry.connection_generation == 7)
        );
        assert!(gate.is_empty());
    }
    #[test]
    fn later_input_never_restarts_configuration_deadline_and_capacity_is_bounded() {
        let now = Instant::now();
        let mut gate = ConfigurationWait::default();
        gate.push(1, au(2, 1), &config(1), now).unwrap();
        gate.push(1, au(2, 2), &config(1), now + DEADLINE).unwrap();
        assert_eq!(
            gate.resolve(1, &config(2), now + DEADLINE + Duration::from_nanos(1))
                .unwrap_err(),
            IngressFailure::ConfigurationUnavailable
        );
        gate.clear();
        for id in 0..CAPACITY {
            gate.push(1, au(2, id as u64), &config(1), now).unwrap();
        }
        assert_eq!(
            gate.push(1, au(2, 20), &config(1), now),
            Err(IngressFailure::Capacity)
        );
    }
}
