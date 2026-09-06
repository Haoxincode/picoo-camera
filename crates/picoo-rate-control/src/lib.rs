//! Bounded bitrate adaptation within the committed source format — REQ-PICOO-MEDIA-027.

use picoo_metrics::ReceiverStats;

/// 720p ladder (PRD FR-ENC-003 style bounds).
pub const LADDER_720_MIN_BPS: u32 = 1_500_000;
pub const LADDER_720_MAX_BPS: u32 = 5_000_000;
pub const LADDER_720_INITIAL_BPS: u32 = 3_000_000;

/// 1080p ladder.
pub const LADDER_1080_MIN_BPS: u32 = 3_000_000;
pub const LADDER_1080_MAX_BPS: u32 = 10_000_000;
pub const LADDER_1080_INITIAL_BPS: u32 = 6_000_000;

/// Exact source-height boundary — REQ-PICOO-MEDIA-028.
pub fn is_supported_height(height: u32) -> bool {
    matches!(height, 720 | 1080)
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct BitrateLadder {
    pub min_bps: u32,
    pub max_bps: u32,
    pub initial_bps: u32,
}

impl BitrateLadder {
    pub fn for_height(height: u32) -> Option<Self> {
        Some(match height {
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
            _ => return None,
        })
    }
}

#[derive(Debug, Clone)]
pub struct BitrateController {
    current_bitrate_bps: u32,
    min_bps: u32,
    max_bps: u32,
    stable_seconds: u32,
    stale_frame_ticks: u32,
    last_sender_queue_drops: u64,
    last_sender_quic_lost_packets: u64,
    last_sender_quic_sent_packets: u64,
    /// Thermal policy can hold bitrate growth, never change the source format.
    thermal_hold: bool,
    /// Currently encoded height (720 / 1080).
    active_height: u32,
    /// User/capability preference; feedback never changes the active source height.
    preferred_height: u32,
}

impl BitrateController {
    pub fn new(initial_bps: u32, min_bps: u32, max_bps: u32) -> Self {
        Self {
            current_bitrate_bps: initial_bps,
            min_bps,
            max_bps,
            stable_seconds: 0,
            stale_frame_ticks: 0,
            last_sender_queue_drops: 0,
            last_sender_quic_lost_packets: 0,
            last_sender_quic_sent_packets: 0,
            thermal_hold: false,
            active_height: 1080,
            preferred_height: 1080,
        }
    }

    pub fn for_height(height: u32) -> Option<Self> {
        let ladder = BitrateLadder::for_height(height)?;
        let h = height;
        let mut ctrl = Self::new(ladder.initial_bps, ladder.min_bps, ladder.max_bps);
        ctrl.active_height = h;
        ctrl.preferred_height = h;
        Some(ctrl)
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
    pub fn set_preferred_height(&mut self, height: u32) -> bool {
        let Some(ladder) = BitrateLadder::for_height(height) else {
            return false;
        };
        let h = height;
        self.preferred_height = h;
        if self.active_height == h {
            self.apply_ladder(ladder, /*reset_current*/ false);
        }
        true
    }

    /// Hold bitrate growth while thermally constrained; source format is unchanged.
    pub fn set_thermal_hold(&mut self, hold: bool) {
        if self.thermal_hold != hold {
            self.thermal_hold = hold;
            self.stable_seconds = 0;
        }
    }

    pub fn thermal_hold(&self) -> bool {
        self.thermal_hold
    }

    /// Called only when an explicit native configuration transaction commits.
    pub fn sync_encode_height(&mut self, height: u32) -> bool {
        let Some(ladder) = BitrateLadder::for_height(height) else {
            return false;
        };
        if height != self.active_height {
            self.active_height = height;
            self.apply_ladder(ladder, true);
            self.stable_seconds = 0;
            self.stale_frame_ticks = 0;
        }
        true
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

    pub fn update(&mut self, stats: &ReceiverStats) -> BitrateAction {
        // ARCH-PICOO-SESSION-001 requires frame age to be *sustained* before
        // reducing quality. A single decoder/UI scheduling spike is not proof
        // that the LAN is congested.
        if stats.frame_age_ms > 200.0 {
            self.stale_frame_ticks = self.stale_frame_ticks.saturating_add(1);
        } else {
            self.stale_frame_ticks = 0;
        }
        let sender_queue_drop =
            stats.sender_queue_dropped_access_units > self.last_sender_queue_drops;
        self.last_sender_queue_drops = stats.sender_queue_dropped_access_units;
        let sender_quic_lost = stats
            .sender_quic_lost_packets
            .saturating_sub(self.last_sender_quic_lost_packets);
        let sender_quic_sent = stats
            .sender_quic_sent_packets
            .saturating_sub(self.last_sender_quic_sent_packets);
        self.last_sender_quic_lost_packets = stats.sender_quic_lost_packets;
        self.last_sender_quic_sent_packets = stats.sender_quic_sent_packets;
        let sender_quic_loss = if sender_quic_sent == 0 {
            0.0
        } else {
            sender_quic_lost as f64 / sender_quic_sent as f64
        };
        let congested = stats.packet_loss > 0.03
            || stats.reassembly_drop > 0
            || stats.decoder_drop > 0
            || stats.sender_queue_age_ms > 50.0
            || sender_queue_drop
            || stats.sender_video_buffered_bytes > 96 * 1024
            || sender_quic_loss > 0.02
            || self.stale_frame_ticks >= 3;
        if congested {
            self.stable_seconds = 0;
            if self.current_bitrate_bps <= self.min_bps {
                return BitrateAction::Hold;
            }
            let reduced = ((self.current_bitrate_bps as f64) * 0.8) as u32;
            self.current_bitrate_bps = reduced.clamp(self.min_bps, self.max_bps);
            return BitrateAction::Decrease;
        }

        if stats.packet_loss < 0.01
            && stats.reassembly_drop == 0
            && stats.decoder_drop == 0
            && stats.sender_queue_age_ms <= 50.0
            && stats.sender_video_buffered_bytes <= 32 * 1024
            && sender_quic_loss < 0.005
            && stats.frame_age_ms < 200.0
        {
            if self.thermal_hold {
                self.stable_seconds = 0;
                return BitrateAction::Hold;
            }
            self.stable_seconds = self.stable_seconds.saturating_add(1);
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
    fn unsupported_source_heights_are_rejected_without_mutation() {
        let mut controller = BitrateController::for_height(1080).unwrap();
        for height in [0, 1, 480, 719, 721, 1079, 1081, 2160, u32::MAX] {
            assert!(BitrateLadder::for_height(height).is_none());
            assert!(BitrateController::for_height(height).is_none());
            assert!(!controller.set_preferred_height(height));
            assert!(!controller.sync_encode_height(height));
            assert_eq!(controller.active_height(), 1080);
            assert_eq!(controller.preferred_height(), 1080);
            assert_eq!(controller.current_bitrate_bps(), LADDER_1080_INITIAL_BPS);
        }
    }

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
    fn decreases_on_new_sender_queue_drop_but_not_the_same_cumulative_counter_twice() {
        let mut ctrl = BitrateController::new(6_000_000, 3_000_000, 10_000_000);
        let dropped = ReceiverStats {
            sender_queue_dropped_access_units: 1,
            ..Default::default()
        };
        assert_eq!(ctrl.update(&dropped), BitrateAction::Decrease);
        assert_eq!(ctrl.update(&dropped), BitrateAction::Hold);
    }

    #[test]
    fn ignores_one_frame_age_spike_but_decreases_when_stale_is_sustained() {
        let mut ctrl = BitrateController::for_height(720).unwrap();
        let stale = ReceiverStats {
            frame_age_ms: 250.0,
            ..Default::default()
        };
        assert_eq!(ctrl.update(&stale), BitrateAction::Hold);
        assert_eq!(ctrl.update(&ReceiverStats::default()), BitrateAction::Hold);
        assert_eq!(ctrl.current_bitrate_bps(), LADDER_720_INITIAL_BPS);

        assert_eq!(ctrl.update(&stale), BitrateAction::Hold);
        assert_eq!(ctrl.update(&stale), BitrateAction::Hold);
        assert_eq!(ctrl.update(&stale), BitrateAction::Decrease);
        assert!(ctrl.current_bitrate_bps() < LADDER_720_INITIAL_BPS);
    }

    #[test]
    fn recovers_bitrate_after_five_clean_media_windows_regardless_of_occupancy() {
        let mut ctrl = BitrateController::for_height(1080).unwrap();
        assert_eq!(
            ctrl.update(&ReceiverStats {
                packet_loss: 0.05,
                ..Default::default()
            }),
            BitrateAction::Decrease
        );
        let reduced = ctrl.current_bitrate_bps();

        for occupancy_ms in [66.0, 100.0, 66.0, 100.0] {
            assert_eq!(
                ctrl.update(&ReceiverStats {
                    packet_loss: 0.0,
                    jitter_buffer_occupancy_ms: occupancy_ms,
                    frame_age_ms: 20.0,
                    ..Default::default()
                }),
                BitrateAction::Hold
            );
        }
        assert_eq!(
            ctrl.update(&ReceiverStats {
                packet_loss: 0.0,
                jitter_buffer_occupancy_ms: 100.0,
                frame_age_ms: 20.0,
                ..Default::default()
            }),
            BitrateAction::Increase
        );
        assert!(ctrl.current_bitrate_bps() > reduced);
    }

    #[test]
    fn sustained_congestion_and_recovery_keep_committed_source_format() {
        for height in [720, 1080] {
            let mut ctrl = BitrateController::for_height(height).unwrap();
            let bad = ReceiverStats {
                packet_loss: 0.2,
                frame_age_ms: 500.0,
                ..Default::default()
            };
            for _ in 0..1000 {
                ctrl.update(&bad);
                assert_eq!(ctrl.active_height(), height);
            }
            assert_eq!(ctrl.current_bitrate_bps(), ctrl.min_bps());
            assert_eq!(ctrl.update(&bad), BitrateAction::Hold);
            for _ in 0..1000 {
                ctrl.update(&ReceiverStats::default());
                assert_eq!(ctrl.active_height(), height);
            }
            assert_eq!(ctrl.current_bitrate_bps(), ctrl.max_bps());
        }
    }

    #[test]
    fn thermal_hold_blocks_growth_but_keeps_congestion_response() {
        let mut ctrl = BitrateController::for_height(1080).unwrap();
        ctrl.set_thermal_hold(true);
        let initial = ctrl.current_bitrate_bps();
        for _ in 0..30 {
            assert_eq!(ctrl.update(&ReceiverStats::default()), BitrateAction::Hold);
        }
        assert_eq!(ctrl.current_bitrate_bps(), initial);
        assert_eq!(
            ctrl.update(&ReceiverStats {
                packet_loss: 0.1,
                ..Default::default()
            }),
            BitrateAction::Decrease
        );
        let reduced = ctrl.current_bitrate_bps();
        ctrl.set_thermal_hold(false);
        for _ in 0..5 {
            ctrl.update(&ReceiverStats::default());
        }
        assert!(ctrl.current_bitrate_bps() > reduced);
        assert_eq!(ctrl.active_height(), 1080);
    }

    #[test]
    fn repeated_healthy_thermal_reports_preserve_bitrate_recovery() {
        let mut ctrl = BitrateController::for_height(1080).unwrap();
        let initial = ctrl.current_bitrate_bps();
        for _ in 0..5 {
            ctrl.set_thermal_hold(false);
            ctrl.update(&ReceiverStats::default());
        }
        assert!(ctrl.current_bitrate_bps() > initial);
    }

    #[test]
    fn explicit_format_commit_replaces_only_its_bitrate_bounds() {
        let mut ctrl = BitrateController::for_height(1080).unwrap();
        ctrl.sync_encode_height(720);
        assert_eq!(ctrl.active_height(), 720);
        assert_eq!(ctrl.current_bitrate_bps(), LADDER_720_INITIAL_BPS);
        assert_eq!(ctrl.max_bps(), LADDER_720_MAX_BPS);
        ctrl.sync_encode_height(1080);
        assert_eq!(ctrl.active_height(), 1080);
        assert_eq!(ctrl.current_bitrate_bps(), LADDER_1080_INITIAL_BPS);
    }
}
