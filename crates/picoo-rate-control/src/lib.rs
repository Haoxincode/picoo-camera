//! Adaptive bitrate — REQ-PICOO-MEDIA-007, REQ-PICOO-MEDIA-010, REQ-PICOO-SESSION (码率策略).

use picoo_metrics::ReceiverStats;

#[derive(Debug, Clone)]
pub struct BitrateController {
    current_bitrate_bps: u32,
    min_bps: u32,
    max_bps: u32,
    stable_seconds: u32,
    /// Consecutive congested updates while already at `min_bps`.
    congested_at_floor_ticks: u32,
    /// Emit DownshiftResolution once until congestion clears.
    downshift_armed: bool,
}

impl BitrateController {
    pub fn new(initial_bps: u32, min_bps: u32, max_bps: u32) -> Self {
        Self {
            current_bitrate_bps: initial_bps,
            min_bps,
            max_bps,
            stable_seconds: 0,
            congested_at_floor_ticks: 0,
            downshift_armed: true,
        }
    }

    pub fn current_bitrate_bps(&self) -> u32 {
        self.current_bitrate_bps
    }

    /// After the host applies 1080p→720p, allow another downshift if congestion returns later.
    pub fn acknowledge_resolution_downshift(&mut self) {
        self.downshift_armed = false;
        self.congested_at_floor_ticks = 0;
    }

    pub fn update(&mut self, stats: &ReceiverStats) -> BitrateAction {
        let congested = stats.packet_loss > 0.03 || stats.frame_age_ms > 200.0;
        if congested {
            self.stable_seconds = 0;
            if self.current_bitrate_bps <= self.min_bps {
                self.congested_at_floor_ticks = self.congested_at_floor_ticks.saturating_add(1);
                // PUC-006 last rung: sustained congestion at min bitrate → drop 1080p→720p.
                if self.downshift_armed && self.congested_at_floor_ticks >= 3 {
                    self.downshift_armed = false;
                    self.congested_at_floor_ticks = 0;
                    return BitrateAction::DownshiftResolution;
                }
                return BitrateAction::Hold;
            }
            let reduced = ((self.current_bitrate_bps as f64) * 0.8) as u32;
            self.current_bitrate_bps = reduced.clamp(self.min_bps, self.max_bps);
            self.congested_at_floor_ticks = 0;
            return BitrateAction::Decrease;
        }

        self.congested_at_floor_ticks = 0;
        // Healthy path re-arms downshift for a future congestion episode.
        if stats.packet_loss < 0.01 && stats.jitter_buffer_depth_ms < 80.0 {
            self.downshift_armed = true;
            self.stable_seconds += 1;
            if self.stable_seconds >= 5 {
                let increased = ((self.current_bitrate_bps as f64) * 1.1) as u32;
                self.current_bitrate_bps = increased.clamp(self.min_bps, self.max_bps);
                self.stable_seconds = 0;
                return BitrateAction::Increase;
            }
        } else {
            self.stable_seconds = 0;
        }

        BitrateAction::Hold
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum BitrateAction {
    Hold,
    Increase,
    Decrease,
    /// Prefer lowering capture/encode height (typically 1080p → 720p).
    DownshiftResolution,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn decreases_on_packet_loss() {
        let mut ctrl = BitrateController::new(6_000_000, 3_000_000, 10_000_000);
        let action = ctrl.update(&ReceiverStats {
            packet_loss: 0.05,
            ..Default::default()
        });
        assert_eq!(action, BitrateAction::Decrease);
        assert!(ctrl.current_bitrate_bps() < 6_000_000);
    }

    #[test]
    fn downshifts_resolution_after_sustained_floor_congestion() {
        let mut ctrl = BitrateController::new(3_000_000, 3_000_000, 10_000_000);
        let bad = ReceiverStats {
            packet_loss: 0.05,
            frame_age_ms: 250.0,
            ..Default::default()
        };
        assert_eq!(ctrl.update(&bad), BitrateAction::Hold);
        assert_eq!(ctrl.update(&bad), BitrateAction::Hold);
        assert_eq!(ctrl.update(&bad), BitrateAction::DownshiftResolution);
        // Not repeated until re-armed by healthy stats.
        assert_eq!(ctrl.update(&bad), BitrateAction::Hold);

        let good = ReceiverStats {
            packet_loss: 0.0,
            jitter_buffer_depth_ms: 40.0,
            ..Default::default()
        };
        let _ = ctrl.update(&good);
        assert_eq!(ctrl.update(&bad), BitrateAction::Hold);
        assert_eq!(ctrl.update(&bad), BitrateAction::Hold);
        assert_eq!(ctrl.update(&bad), BitrateAction::DownshiftResolution);
    }
}
