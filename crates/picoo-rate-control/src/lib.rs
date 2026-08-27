//! Adaptive bitrate — REQ-PICOO-MEDIA-007, REQ-PICOO-SESSION (码率策略).

use picoo_metrics::ReceiverStats;

#[derive(Debug, Clone)]
pub struct BitrateController {
    current_bitrate_bps: u32,
    min_bps: u32,
    max_bps: u32,
    stable_seconds: u32,
}

impl BitrateController {
    pub fn new(initial_bps: u32, min_bps: u32, max_bps: u32) -> Self {
        Self {
            current_bitrate_bps: initial_bps,
            min_bps,
            max_bps,
            stable_seconds: 0,
        }
    }

    pub fn current_bitrate_bps(&self) -> u32 {
        self.current_bitrate_bps
    }

    pub fn update(&mut self, stats: &ReceiverStats) -> BitrateAction {
        if stats.packet_loss > 0.03 || stats.frame_age_ms > 200.0 {
            self.stable_seconds = 0;
            let reduced = ((self.current_bitrate_bps as f64) * 0.8) as u32;
            self.current_bitrate_bps = reduced.clamp(self.min_bps, self.max_bps);
            return BitrateAction::Decrease;
        }

        if stats.packet_loss < 0.01 && stats.jitter_buffer_depth_ms < 80.0 {
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
}
