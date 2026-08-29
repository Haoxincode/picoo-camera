//! Adaptive bitrate + resolution ladder — REQ-PICOO-MEDIA-007/010, PUC-006.

use picoo_metrics::ReceiverStats;

/// 480p ladder (weak-network floor; aligns with Android MediaBitrate).
pub const LADDER_480_MIN_BPS: u32 = 900_000;
pub const LADDER_480_MAX_BPS: u32 = 2_500_000;
pub const LADDER_480_INITIAL_BPS: u32 = 1_800_000;

/// 720p ladder (PRD FR-ENC-003 style bounds).
pub const LADDER_720_MIN_BPS: u32 = 1_500_000;
pub const LADDER_720_MAX_BPS: u32 = 5_000_000;
pub const LADDER_720_INITIAL_BPS: u32 = 3_000_000;

/// 1080p ladder.
pub const LADDER_1080_MIN_BPS: u32 = 3_000_000;
pub const LADDER_1080_MAX_BPS: u32 = 10_000_000;
pub const LADDER_1080_INITIAL_BPS: u32 = 6_000_000;

/// Snap arbitrary encode height onto the V1 ladder (1080 / 720 / 480).
pub fn normalize_height(height: u32) -> u32 {
    if height >= 1080 {
        1080
    } else if height >= 720 {
        720
    } else {
        480
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct BitrateLadder {
    pub min_bps: u32,
    pub max_bps: u32,
    pub initial_bps: u32,
}

impl BitrateLadder {
    pub fn for_height(height: u32) -> Self {
        match normalize_height(height) {
            1080 => Self {
                min_bps: LADDER_1080_MIN_BPS,
                max_bps: LADDER_1080_MAX_BPS,
                initial_bps: LADDER_1080_INITIAL_BPS,
            },
            720 => Self {
                min_bps: LADDER_720_MIN_BPS,
                max_bps: LADDER_720_MAX_BPS,
                initial_bps: LADDER_720_INITIAL_BPS,
            },
            _ => Self {
                min_bps: LADDER_480_MIN_BPS,
                max_bps: LADDER_480_MAX_BPS,
                initial_bps: LADDER_480_INITIAL_BPS,
            },
        }
    }
}

#[derive(Debug, Clone)]
pub struct BitrateController {
    current_bitrate_bps: u32,
    min_bps: u32,
    max_bps: u32,
    stable_seconds: u32,
    congested_at_floor_ticks: u32,
    downshift_armed: bool,
    upshift_armed: bool,
    /// Host thermal policy: block ABR upshift while true (REQ-PICOO-MEDIA-010).
    thermal_hold: bool,
    /// Currently encoded height (480 / 720 / 1080).
    active_height: u32,
    /// User / capability preferred height (may be 1080 while active is 720).
    preferred_height: u32,
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
            upshift_armed: false,
            thermal_hold: false,
            active_height: 1080,
            preferred_height: 1080,
        }
    }

    pub fn for_height(height: u32) -> Self {
        let h = normalize_height(height);
        let ladder = BitrateLadder::for_height(h);
        let mut ctrl = Self::new(ladder.initial_bps, ladder.min_bps, ladder.max_bps);
        ctrl.active_height = h;
        ctrl.preferred_height = h;
        ctrl
    }

    pub fn current_bitrate_bps(&self) -> u32 {
        self.current_bitrate_bps
    }

    pub fn min_bps(&self) -> u32 {
        self.min_bps
    }

    pub fn max_bps(&self) -> u32 {
        self.max_bps
    }

    pub fn active_height(&self) -> u32 {
        self.active_height
    }

    pub fn preferred_height(&self) -> u32 {
        self.preferred_height
    }

    /// Sync preferred height from StreamConfig / user Resolution toggle.
    pub fn set_preferred_height(&mut self, height: u32) {
        let h = normalize_height(height);
        self.preferred_height = h;
        if self.active_height == h {
            self.apply_ladder(BitrateLadder::for_height(h), /*reset_current*/ false);
        }
    }

    /// Host thermal overheat: block ABR 720→1080 until cleared (MEDIA-010).
    pub fn set_thermal_hold(&mut self, hold: bool) {
        self.thermal_hold = hold;
        if hold {
            self.upshift_armed = self.active_height < self.preferred_height;
        }
    }

    pub fn thermal_hold(&self) -> bool {
        self.thermal_hold
    }

    /// Host applied encode height (thermal force, user toggle, or ABR apply).
    pub fn sync_encode_height(&mut self, height: u32) {
        let h = normalize_height(height);
        if h == self.active_height {
            return;
        }
        if h < self.active_height {
            self.acknowledge_resolution_downshift();
            if self.active_height != h {
                self.active_height = h;
                self.apply_ladder(BitrateLadder::for_height(h), true);
            }
        } else {
            if self.preferred_height < h {
                self.preferred_height = h;
            }
            self.acknowledge_resolution_upshift();
            if self.active_height != h && h <= self.preferred_height {
                self.active_height = h;
                self.apply_ladder(BitrateLadder::for_height(h), true);
            }
        }
    }

    fn apply_ladder(&mut self, ladder: BitrateLadder, reset_current: bool) {
        self.min_bps = ladder.min_bps;
        self.max_bps = ladder.max_bps;
        if reset_current {
            self.current_bitrate_bps = ladder.initial_bps;
        } else {
            self.current_bitrate_bps = self
                .current_bitrate_bps
                .clamp(ladder.min_bps, ladder.max_bps);
        }
    }

    /// Host applied one ladder rung down (1080→720 or 720→480).
    pub fn acknowledge_resolution_downshift(&mut self) {
        self.active_height = if self.active_height >= 1080 { 720 } else { 480 };
        self.apply_ladder(BitrateLadder::for_height(self.active_height), true);
        self.downshift_armed = self.active_height > 480;
        self.upshift_armed = true;
        self.congested_at_floor_ticks = 0;
        self.stable_seconds = 0;
    }

    /// Host applied one ladder rung up toward preferred height.
    pub fn acknowledge_resolution_upshift(&mut self) {
        let next = if self.active_height < 720 && self.preferred_height >= 720 {
            720
        } else if self.active_height < 1080 && self.preferred_height >= 1080 {
            1080
        } else {
            self.preferred_height.max(self.active_height)
        };
        self.active_height = normalize_height(next);
        self.apply_ladder(BitrateLadder::for_height(self.active_height), true);
        self.upshift_armed = self.active_height < self.preferred_height;
        self.downshift_armed = self.active_height > 480;
        self.congested_at_floor_ticks = 0;
        self.stable_seconds = 0;
    }

    #[cfg(test)]
    fn set_current_bitrate_bps_for_test(&mut self, bps: u32) {
        self.current_bitrate_bps = bps.clamp(self.min_bps, self.max_bps);
    }

    pub fn update(&mut self, stats: &ReceiverStats) -> BitrateAction {
        let congested = stats.packet_loss > 0.03 || stats.frame_age_ms > 200.0;
        if congested {
            self.stable_seconds = 0;
            if self.current_bitrate_bps <= self.min_bps {
                self.congested_at_floor_ticks = self.congested_at_floor_ticks.saturating_add(1);
                if self.downshift_armed
                    && self.active_height > 480
                    && self.congested_at_floor_ticks >= 3
                {
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
        if stats.packet_loss < 0.01 && stats.jitter_buffer_depth_ms < 80.0 {
            self.downshift_armed = self.active_height > 480;
            self.stable_seconds += 1;
            let near_max = self.current_bitrate_bps >= (self.max_bps * 9) / 10;
            // Prefer climbing back toward preferred height before further bitrate increases.
            if self.upshift_armed
                && !self.thermal_hold
                && self.active_height < self.preferred_height
                && near_max
            {
                if self.stable_seconds >= 8 {
                    self.upshift_armed = false;
                    self.stable_seconds = 0;
                    return BitrateAction::UpshiftResolution;
                }
                return BitrateAction::Hold;
            }
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
    /// Prefer lowering capture/encode height (1080→720 or 720→480).
    DownshiftResolution,
    /// Prefer restoring preferred height one rung at a time.
    UpshiftResolution,
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
        let mut ctrl = BitrateController::for_height(1080);
        let bad = ReceiverStats {
            packet_loss: 0.05,
            frame_age_ms: 250.0,
            ..Default::default()
        };
        let mut saw = BitrateAction::Hold;
        for _ in 0..40 {
            saw = ctrl.update(&bad);
            if saw == BitrateAction::DownshiftResolution {
                break;
            }
        }
        assert_eq!(saw, BitrateAction::DownshiftResolution);
        ctrl.acknowledge_resolution_downshift();
        assert_eq!(ctrl.active_height(), 720);
        assert_eq!(ctrl.min_bps(), LADDER_720_MIN_BPS);
        assert_eq!(ctrl.max_bps(), LADDER_720_MAX_BPS);
    }

    #[test]
    fn upshifts_resolution_after_sustained_health_on_720_ladder() {
        let mut ctrl = BitrateController::for_height(1080);
        ctrl.acknowledge_resolution_downshift();
        assert_eq!(ctrl.active_height(), 720);
        // Push bitrate near 720 max.
        ctrl.set_current_bitrate_bps_for_test(LADDER_720_MAX_BPS);
        let good = ReceiverStats {
            packet_loss: 0.0,
            jitter_buffer_depth_ms: 40.0,
            ..Default::default()
        };
        let mut saw = BitrateAction::Hold;
        for _ in 0..20 {
            saw = ctrl.update(&good);
            if saw == BitrateAction::UpshiftResolution {
                break;
            }
        }
        assert_eq!(saw, BitrateAction::UpshiftResolution);
        ctrl.acknowledge_resolution_upshift();
        assert_eq!(ctrl.active_height(), 1080);
        assert_eq!(ctrl.max_bps(), LADDER_1080_MAX_BPS);
    }

    #[test]
    fn preferred_720_does_not_upshift() {
        let mut ctrl = BitrateController::for_height(720);
        ctrl.set_current_bitrate_bps_for_test(LADDER_720_MAX_BPS);
        let good = ReceiverStats {
            packet_loss: 0.0,
            jitter_buffer_depth_ms: 40.0,
            ..Default::default()
        };
        for _ in 0..20 {
            assert_ne!(ctrl.update(&good), BitrateAction::UpshiftResolution);
        }
    }

    #[test]
    fn thermal_hold_blocks_upshift_until_cleared() {
        let mut ctrl = BitrateController::for_height(1080);
        ctrl.acknowledge_resolution_downshift();
        ctrl.set_thermal_hold(true);
        ctrl.set_current_bitrate_bps_for_test(LADDER_720_MAX_BPS);
        let good = ReceiverStats {
            packet_loss: 0.0,
            jitter_buffer_depth_ms: 40.0,
            ..Default::default()
        };
        for _ in 0..30 {
            assert_ne!(
                ctrl.update(&good),
                BitrateAction::UpshiftResolution,
                "thermal hold must block ABR upshift"
            );
        }
        assert_eq!(ctrl.active_height(), 720);
        ctrl.set_thermal_hold(false);
        let mut saw = BitrateAction::Hold;
        for _ in 0..20 {
            saw = ctrl.update(&good);
            if saw == BitrateAction::UpshiftResolution {
                break;
            }
        }
        assert_eq!(saw, BitrateAction::UpshiftResolution);
    }

    #[test]
    fn sync_encode_height_forces_ladder_down() {
        let mut ctrl = BitrateController::for_height(1080);
        ctrl.sync_encode_height(720);
        assert_eq!(ctrl.active_height(), 720);
        assert_eq!(ctrl.preferred_height(), 1080);
        assert_eq!(ctrl.max_bps(), LADDER_720_MAX_BPS);
    }

    #[test]
    fn downshifts_720_to_480_after_floor_congestion() {
        let mut ctrl = BitrateController::for_height(720);
        let bad = ReceiverStats {
            packet_loss: 0.05,
            frame_age_ms: 250.0,
            ..Default::default()
        };
        let mut saw = BitrateAction::Hold;
        for _ in 0..40 {
            saw = ctrl.update(&bad);
            if saw == BitrateAction::DownshiftResolution {
                break;
            }
        }
        assert_eq!(saw, BitrateAction::DownshiftResolution);
        ctrl.acknowledge_resolution_downshift();
        assert_eq!(ctrl.active_height(), 480);
        assert_eq!(ctrl.min_bps(), LADDER_480_MIN_BPS);
        assert_eq!(ctrl.max_bps(), LADDER_480_MAX_BPS);
    }

    #[test]
    fn sync_encode_height_480_uses_480_ladder() {
        let mut ctrl = BitrateController::for_height(1080);
        ctrl.sync_encode_height(480);
        assert_eq!(ctrl.active_height(), 480);
        assert_eq!(ctrl.max_bps(), LADDER_480_MAX_BPS);
        assert_eq!(ctrl.min_bps(), LADDER_480_MIN_BPS);
    }
}
