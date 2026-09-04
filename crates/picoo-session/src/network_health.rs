//! Network health episodes and UI hysteresis — REQ-PICOO-SESSION-013.

use std::time::Instant;

pub const BAD_WINDOWS_TO_ENTER: u32 = 2;
pub const CLEAN_WINDOWS_TO_RECOVER: u32 = 5;
pub const BAD_PACKET_LOSS_THRESHOLD: f64 = 0.03;
pub const CLEAN_PACKET_LOSS_THRESHOLD: f64 = 0.01;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum NetworkDegradation {
    FragmentLoss,
}

#[derive(Debug, Clone)]
pub struct NetworkEpisode {
    pub started_at: Instant,
    pub causes: Vec<NetworkDegradation>,
    pub bad_windows: u32,
    pub clean_windows: u32,
    pub worst_packet_loss: f64,
}

#[derive(Debug, Clone, Default)]
pub enum NetworkHealth {
    #[default]
    Healthy,
    Degraded(NetworkEpisode),
}

impl NetworkHealth {
    pub fn is_degraded(&self) -> bool {
        matches!(self, Self::Degraded(_))
    }

    pub fn episode(&self) -> Option<&NetworkEpisode> {
        match self {
            Self::Healthy => None,
            Self::Degraded(episode) => Some(episode),
        }
    }
}

#[derive(Debug, Default)]
pub struct NetworkHealthTracker {
    health: NetworkHealth,
    pending_bad_windows: u32,
    pending_started_at: Option<Instant>,
    pending_worst_packet_loss: f64,
}

impl NetworkHealthTracker {
    pub fn health(&self) -> &NetworkHealth {
        &self.health
    }

    pub fn observe_packet_loss(&mut self, packet_loss: f64, observed_at: Instant) {
        if !packet_loss.is_finite() {
            match &mut self.health {
                NetworkHealth::Healthy => self.clear_pending(),
                NetworkHealth::Degraded(episode) => episode.clean_windows = 0,
            }
            return;
        }
        let packet_loss = packet_loss.clamp(0.0, 1.0);
        match &mut self.health {
            NetworkHealth::Healthy => {
                if packet_loss > BAD_PACKET_LOSS_THRESHOLD {
                    if self.pending_bad_windows == 0 {
                        self.pending_started_at = Some(observed_at);
                        self.pending_worst_packet_loss = packet_loss;
                    }
                    self.pending_bad_windows = self.pending_bad_windows.saturating_add(1);
                    self.pending_worst_packet_loss =
                        self.pending_worst_packet_loss.max(packet_loss);
                    if self.pending_bad_windows >= BAD_WINDOWS_TO_ENTER {
                        self.health = NetworkHealth::Degraded(NetworkEpisode {
                            started_at: self.pending_started_at.unwrap_or(observed_at),
                            causes: vec![NetworkDegradation::FragmentLoss],
                            bad_windows: self.pending_bad_windows,
                            clean_windows: 0,
                            worst_packet_loss: self.pending_worst_packet_loss,
                        });
                        self.clear_pending();
                    }
                } else {
                    self.clear_pending();
                }
            }
            NetworkHealth::Degraded(episode) => {
                if packet_loss > BAD_PACKET_LOSS_THRESHOLD {
                    episode.bad_windows = episode.bad_windows.saturating_add(1);
                    episode.clean_windows = 0;
                    episode.worst_packet_loss = episode.worst_packet_loss.max(packet_loss);
                    if !episode.causes.contains(&NetworkDegradation::FragmentLoss) {
                        episode.causes.push(NetworkDegradation::FragmentLoss);
                    }
                } else if packet_loss < CLEAN_PACKET_LOSS_THRESHOLD {
                    episode.clean_windows = episode.clean_windows.saturating_add(1);
                    if episode.clean_windows >= CLEAN_WINDOWS_TO_RECOVER {
                        self.health = NetworkHealth::Healthy;
                    }
                } else {
                    episode.clean_windows = 0;
                }
            }
        }
    }

    pub fn reset(&mut self) {
        self.health = NetworkHealth::Healthy;
        self.clear_pending();
    }

    fn clear_pending(&mut self) {
        self.pending_bad_windows = 0;
        self.pending_started_at = None;
        self.pending_worst_packet_loss = 0.0;
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::time::Duration;

    #[test]
    fn two_consecutive_bad_windows_open_one_episode_from_the_first_window() {
        let started_at = Instant::now();
        let mut tracker = NetworkHealthTracker::default();
        tracker.observe_packet_loss(0.04, started_at);
        assert!(!tracker.health().is_degraded());

        tracker.observe_packet_loss(0.06, started_at + Duration::from_secs(1));
        let episode = tracker.health().episode().expect("degraded episode");
        assert_eq!(episode.started_at, started_at);
        assert_eq!(episode.bad_windows, 2);
        assert_eq!(episode.clean_windows, 0);
        assert_eq!(episode.worst_packet_loss, 0.06);
        assert_eq!(episode.causes, vec![NetworkDegradation::FragmentLoss]);
    }

    #[test]
    fn a_non_bad_window_resets_the_entry_streak() {
        let now = Instant::now();
        let mut tracker = NetworkHealthTracker::default();
        tracker.observe_packet_loss(0.04, now);
        tracker.observe_packet_loss(0.02, now + Duration::from_secs(1));
        tracker.observe_packet_loss(0.04, now + Duration::from_secs(2));
        assert!(!tracker.health().is_degraded());
        tracker.observe_packet_loss(0.04, now + Duration::from_secs(3));
        assert!(tracker.health().is_degraded());
    }

    #[test]
    fn five_consecutive_clean_windows_close_the_episode() {
        let now = Instant::now();
        let mut tracker = NetworkHealthTracker::default();
        tracker.observe_packet_loss(0.04, now);
        tracker.observe_packet_loss(0.05, now);
        for offset in 1..CLEAN_WINDOWS_TO_RECOVER {
            tracker.observe_packet_loss(0.0, now + Duration::from_secs(u64::from(offset)));
            assert!(tracker.health().is_degraded());
        }
        tracker.observe_packet_loss(0.0, now + Duration::from_secs(5));
        assert!(!tracker.health().is_degraded());
    }

    #[test]
    fn neutral_or_bad_windows_break_the_recovery_streak() {
        let now = Instant::now();
        let mut tracker = NetworkHealthTracker::default();
        tracker.observe_packet_loss(0.04, now);
        tracker.observe_packet_loss(0.05, now);
        tracker.observe_packet_loss(0.0, now);
        tracker.observe_packet_loss(0.02, now);
        for _ in 0..4 {
            tracker.observe_packet_loss(0.0, now);
        }
        assert!(tracker.health().is_degraded());
        tracker.observe_packet_loss(0.0, now);
        assert!(!tracker.health().is_degraded());
    }

    #[test]
    fn invalid_measurement_never_counts_as_a_clean_window() {
        let now = Instant::now();
        let mut tracker = NetworkHealthTracker::default();
        tracker.observe_packet_loss(0.04, now);
        tracker.observe_packet_loss(0.05, now);
        for _ in 0..4 {
            tracker.observe_packet_loss(0.0, now);
        }
        tracker.observe_packet_loss(f64::NAN, now);
        tracker.observe_packet_loss(0.0, now);
        assert!(tracker.health().is_degraded());
    }
}
