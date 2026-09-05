//! Desktop-only live diagnostics model — REQ-PICOO-UI-014.
//!
//! Receiver owns measurements; this module owns bounded presentation history
//! and fault attribution. GPUI views only render the resulting snapshot.

use std::collections::VecDeque;
use std::time::{Duration, Instant};

use picoo_metrics::ReceiverStats;
use picoo_session::ReceiverStatus;

use crate::model::VirtualCameraStatus;

const HISTORY_WINDOW: Duration = Duration::from_secs(10 * 60);

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum DiagnosticHealth {
    Waiting,
    Healthy,
    Attention,
    Poor,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DiagnosticAnalysis {
    pub health: DiagnosticHealth,
    pub label: &'static str,
    pub message: String,
}

impl DiagnosticAnalysis {
    #[cfg_attr(not(feature = "gpui-ui"), allow(dead_code))]
    pub fn from_metrics(
        stats: Option<&ReceiverStats>,
        actual_fps: u32,
        target_fps: u32,
        media_error: Option<&str>,
        status: ReceiverStatus,
        virtual_camera: VirtualCameraStatus,
        shared_ring_error: Option<&str>,
    ) -> Self {
        analyze(
            stats,
            actual_fps,
            target_fps,
            media_error,
            status,
            virtual_camera,
            shared_ring_error,
        )
    }
}

fn analyze(
    stats: Option<&ReceiverStats>,
    actual_fps: u32,
    target_fps: u32,
    media_error: Option<&str>,
    status: ReceiverStatus,
    virtual_camera: VirtualCameraStatus,
    shared_ring_error: Option<&str>,
) -> DiagnosticAnalysis {
    if let Some(error) = media_error {
        return DiagnosticAnalysis {
            health: DiagnosticHealth::Poor,
            label: "解码异常",
            message: format!("平台解码器报告错误：{error}"),
        };
    }

    if let Some(error) = shared_ring_error {
        return DiagnosticAnalysis {
            health: DiagnosticHealth::Poor,
            label: "输出发布异常",
            message: format!("Receiver 无法向虚拟摄像头发布共享帧：{error}"),
        };
    }

    match virtual_camera {
        VirtualCameraStatus::NotInstalled => {
            return DiagnosticAnalysis {
                health: DiagnosticHealth::Poor,
                label: "输出不可用",
                message: "系统尚未安装或枚举 Picoo 虚拟摄像头，会议软件无法使用当前画面。".into(),
            };
        }
        VirtualCameraStatus::Installed => {
            return DiagnosticAnalysis {
                health: DiagnosticHealth::Attention,
                label: "等待系统发布",
                message: "虚拟摄像头已注册，但系统尚未将其发布到当前用户会话。".into(),
            };
        }
        VirtualCameraStatus::Bundled
        | VirtualCameraStatus::AwaitingApproval
        | VirtualCameraStatus::RestartRequired
        | VirtualCameraStatus::Uninstalling => {
            return DiagnosticAnalysis {
                health: DiagnosticHealth::Attention,
                label: "输出未就绪",
                message: "虚拟摄像头仍在激活、批准、重启或移除流程中。".into(),
            };
        }
        VirtualCameraStatus::Unknown | VirtualCameraStatus::Active => {}
    }

    let Some(stats) = stats else {
        return DiagnosticAnalysis {
            health: DiagnosticHealth::Waiting,
            label: "等待样本",
            message: "收到首个完整统计窗口后，将在这里判断网络与媒体瓶颈。".into(),
        };
    };

    if stats.packet_loss >= 0.05 {
        return DiagnosticAnalysis {
            health: DiagnosticHealth::Poor,
            label: "丢片严重",
            message: "已决视频帧中的缺失分片达到 5%，参考链可能反复等待关键帧恢复。".into(),
        };
    }

    if stats.rtt_ms >= 120.0 {
        return DiagnosticAnalysis {
            health: DiagnosticHealth::Poor,
            label: "链路延迟高",
            message: "QUIC 往返延迟已达到 120 ms，优先检查 Wi-Fi 信号、频段和 AP 负载。".into(),
        };
    }

    if stats.packet_loss > 0.03 || status == ReceiverStatus::NetworkUnstable {
        return DiagnosticAnalysis {
            health: DiagnosticHealth::Attention,
            label: "网络不稳定",
            message: "可观测丢片达到降码率阈值，Receiver 会要求 Sender 快速降低码率。".into(),
        };
    }

    if stats.decoder_drop > 0 {
        return DiagnosticAnalysis {
            health: DiagnosticHealth::Attention,
            label: "解码丢帧",
            message: "最近统计窗口出现解码丢帧；若持续发生，应先排查参考帧恢复和平台解码器。"
                .into(),
        };
    }

    if stats.reassembly_drop > 0 {
        return DiagnosticAnalysis {
            health: DiagnosticHealth::Attention,
            label: "重组丢帧",
            message: "最近统计窗口有 Access Unit 被重组层确认丢弃。".into(),
        };
    }

    if stats.jitter_ms >= 30.0
        || stats.jitter_buffer_target_ms >= 80.0
        || stats.jitter_buffer_actual_delay_ms >= 100.0
        || stats.receive_queue_age_ms >= 100.0
    {
        return DiagnosticAnalysis {
            health: DiagnosticHealth::Attention,
            label: "抖动偏高",
            message: "播放控制器已接近低延迟上限，或实际缓冲停留偏高；不会继续扩大缓冲掩盖问题。"
                .into(),
        };
    }

    if target_fps > 0 && actual_fps.saturating_mul(4) < target_fps.saturating_mul(3) {
        return DiagnosticAnalysis {
            health: DiagnosticHealth::Attention,
            label: "帧率不足",
            message: "实际解码帧率低于协商目标的 75%，瓶颈可能位于发送、网络或解码阶段。".into(),
        };
    }

    if virtual_camera == VirtualCameraStatus::Unknown {
        return DiagnosticAnalysis {
            health: DiagnosticHealth::Waiting,
            label: "确认输出状态",
            message: "网络与媒体指标正常，虚拟摄像头的系统枚举状态仍在确认。".into(),
        };
    }

    DiagnosticAnalysis {
        health: DiagnosticHealth::Healthy,
        label: "运行正常",
        message: "当前链路与媒体处理指标均在实时视频的正常范围内。".into(),
    }
}

#[derive(Debug, Clone, Copy, PartialEq)]
struct LiveMetricSample {
    captured_at: Instant,
    rtt_ms: f64,
    packet_loss: f64,
    jitter_ms: f64,
    buffer_ms: f64,
    actual_fps: u32,
    reassembly_drop: u64,
    decoder_drop: u64,
}

impl LiveMetricSample {
    fn from_values(stats: &ReceiverStats, actual_fps: u32, captured_at: Instant) -> Self {
        Self {
            captured_at,
            rtt_ms: stats.rtt_ms,
            packet_loss: stats.packet_loss,
            jitter_ms: stats.jitter_ms,
            buffer_ms: stats.jitter_buffer_actual_delay_ms,
            actual_fps,
            reassembly_drop: stats.reassembly_drop,
            decoder_drop: stats.decoder_drop,
        }
    }
}

#[derive(Debug, Clone, Copy, Default, PartialEq)]
pub struct HistorySummary {
    pub sample_count: usize,
    pub observed_seconds: u64,
    pub peak_rtt_ms: f64,
    pub peak_packet_loss: f64,
    pub peak_jitter_ms: f64,
    pub peak_buffer_ms: f64,
    pub minimum_fps: u32,
    pub reassembly_drops: u64,
    pub decoder_drops: u64,
}

#[derive(Debug, Default)]
pub struct LiveMetricsHistory {
    samples: VecDeque<LiveMetricSample>,
    last_stats_revision: Option<u64>,
}

impl LiveMetricsHistory {
    pub fn observe(
        &mut self,
        stats: Option<&ReceiverStats>,
        stats_revision: u64,
        actual_fps: u32,
        now: Instant,
    ) {
        if let Some(stats) = stats {
            if self.last_stats_revision != Some(stats_revision) {
                self.samples
                    .push_back(LiveMetricSample::from_values(stats, actual_fps, now));
                self.last_stats_revision = Some(stats_revision);
            }
        }
        self.discard_expired(now);
    }

    pub fn summary(&self) -> HistorySummary {
        let Some(first) = self.samples.front() else {
            return HistorySummary::default();
        };
        let last = self
            .samples
            .back()
            .expect("front proves history is non-empty");
        let mut summary = HistorySummary {
            sample_count: self.samples.len(),
            observed_seconds: last
                .captured_at
                .saturating_duration_since(first.captured_at)
                .as_secs(),
            minimum_fps: u32::MAX,
            ..HistorySummary::default()
        };

        for sample in &self.samples {
            summary.peak_rtt_ms = summary.peak_rtt_ms.max(sample.rtt_ms);
            summary.peak_packet_loss = summary.peak_packet_loss.max(sample.packet_loss);
            summary.peak_jitter_ms = summary.peak_jitter_ms.max(sample.jitter_ms);
            summary.peak_buffer_ms = summary.peak_buffer_ms.max(sample.buffer_ms);
            summary.minimum_fps = summary.minimum_fps.min(sample.actual_fps);
            summary.reassembly_drops = summary
                .reassembly_drops
                .saturating_add(sample.reassembly_drop);
            summary.decoder_drops = summary.decoder_drops.saturating_add(sample.decoder_drop);
        }
        summary
    }

    fn discard_expired(&mut self, now: Instant) {
        while self.samples.front().is_some_and(|sample| {
            now.saturating_duration_since(sample.captured_at) > HISTORY_WINDOW
        }) {
            self.samples.pop_front();
        }
    }

    #[cfg(test)]
    fn record_for_test(&mut self, sample: LiveMetricSample, now: Instant) {
        self.samples.push_back(sample);
        self.discard_expired(now);
    }
}

#[cfg_attr(not(feature = "gpui-ui"), allow(dead_code))]
pub fn format_bitrate(stats: Option<&ReceiverStats>) -> String {
    stats
        .map(|stats| format!("{:.1} Mbps", stats.receive_bitrate as f64 / 1_000_000.0))
        .unwrap_or_else(|| "—".into())
}

#[cfg_attr(not(feature = "gpui-ui"), allow(dead_code))]
pub fn format_milliseconds(value: Option<f64>) -> String {
    value
        .filter(|value| value.is_finite())
        .map(|value| format!("{value:.1} ms"))
        .unwrap_or_else(|| "—".into())
}

#[cfg_attr(not(feature = "gpui-ui"), allow(dead_code))]
pub fn format_packet_loss(stats: Option<&ReceiverStats>) -> String {
    stats
        .map(|stats| format!("{:.2}%", stats.packet_loss * 100.0))
        .unwrap_or_else(|| "—".into())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn stats() -> ReceiverStats {
        ReceiverStats {
            rtt_ms: 24.0,
            packet_loss: 0.0,
            jitter_ms: 2.0,
            reassembly_drop: 0,
            decoder_drop: 0,
            frame_age_ms: 18.0,
            receive_bitrate: 4_600_000,
            jitter_buffer_target_ms: 33.0,
            jitter_buffer_actual_delay_ms: 23.0,
            jitter_buffer_occupancy_ms: 33.0,
            capture_to_encode_ms: None,
            encode_to_arrival_ms: None,
            jitter_residence_ms: None,
            decode_ms: None,
            frame_publish_age_ms: None,
            end_to_end_latency_ms: None,
            clock_uncertainty_ms: None,
            receive_queue_age_ms: 0.0,
            sender_queue_age_ms: 0.0,
            sender_queue_dropped_access_units: 0,
            sender_quic_lost_packets: 0,
            sender_quic_sent_packets: 0,
            sender_video_buffered_bytes: 0,
        }
    }

    #[test]
    fn missing_sample_is_not_reported_as_excellent_or_zero_loss() {
        let analysis = analyze(
            None,
            0,
            30,
            None,
            ReceiverStatus::Streaming,
            VirtualCameraStatus::Active,
            None,
        );
        assert_eq!(analysis.health, DiagnosticHealth::Waiting);
        assert_eq!(analysis.label, "等待样本");
        assert_eq!(format_packet_loss(None), "—");
        assert_eq!(format_milliseconds(None), "—");
    }

    #[test]
    fn analysis_prioritizes_loss_and_decoder_faults() {
        let mut loss = stats();
        loss.packet_loss = 0.06;
        assert_eq!(
            analyze(
                Some(&loss),
                30,
                30,
                None,
                ReceiverStatus::Streaming,
                VirtualCameraStatus::Active,
                None,
            )
            .health,
            DiagnosticHealth::Poor
        );

        let mut decode = stats();
        decode.decoder_drop = 1;
        assert_eq!(
            analyze(
                Some(&decode),
                30,
                30,
                None,
                ReceiverStatus::Streaming,
                VirtualCameraStatus::Active,
                None,
            )
            .label,
            "解码丢帧"
        );
    }

    #[test]
    fn low_actual_fps_is_visible_after_network_checks_pass() {
        let analysis = analyze(
            Some(&stats()),
            20,
            30,
            None,
            ReceiverStatus::Streaming,
            VirtualCameraStatus::Active,
            None,
        );
        assert_eq!(analysis.health, DiagnosticHealth::Attention);
        assert_eq!(analysis.label, "帧率不足");
    }

    #[test]
    fn loss_attention_matches_the_strict_three_percent_control_threshold() {
        let mut at_boundary = stats();
        at_boundary.packet_loss = 0.03;
        assert_eq!(
            analyze(
                Some(&at_boundary),
                30,
                30,
                None,
                ReceiverStatus::Streaming,
                VirtualCameraStatus::Active,
                None,
            )
            .health,
            DiagnosticHealth::Healthy
        );

        at_boundary.packet_loss = 0.030_001;
        assert_eq!(
            analyze(
                Some(&at_boundary),
                30,
                30,
                None,
                ReceiverStatus::Streaming,
                VirtualCameraStatus::Active,
                None,
            )
            .label,
            "网络不稳定"
        );
    }

    #[test]
    fn output_and_decoder_faults_override_healthy_network_or_missing_stats() {
        let media = analyze(
            None,
            0,
            30,
            Some("decoder failed"),
            ReceiverStatus::Streaming,
            VirtualCameraStatus::Active,
            None,
        );
        assert_eq!(media.label, "解码异常");
        assert_eq!(media.health, DiagnosticHealth::Poor);

        let ring = analyze(
            Some(&stats()),
            30,
            30,
            None,
            ReceiverStatus::Streaming,
            VirtualCameraStatus::Active,
            Some("permission denied"),
        );
        assert_eq!(ring.label, "输出发布异常");

        let unavailable = analyze(
            Some(&stats()),
            30,
            30,
            None,
            ReceiverStatus::Streaming,
            VirtualCameraStatus::NotInstalled,
            None,
        );
        assert_eq!(unavailable.label, "输出不可用");
        assert_ne!(unavailable.health, DiagnosticHealth::Healthy);
    }

    #[test]
    fn history_records_each_receiver_stats_revision_once() {
        let start = Instant::now();
        let mut history = LiveMetricsHistory::default();
        let stats = stats();

        history.observe(Some(&stats), 7, 30, start);
        history.observe(Some(&stats), 7, 30, start + Duration::from_secs(1));
        assert_eq!(history.summary().sample_count, 1);

        history.observe(Some(&stats), 8, 30, start + Duration::from_secs(2));
        assert_eq!(history.summary().sample_count, 2);

        history.observe(None, 8, 0, start + Duration::from_secs(3));
        assert_eq!(history.summary().sample_count, 2);
    }

    #[test]
    fn history_is_bounded_and_summarizes_same_unit_windows() {
        let start = Instant::now();
        let mut history = LiveMetricsHistory::default();
        let sample =
            |captured_at, rtt_ms, loss, fps, reassembly_drop, decoder_drop| LiveMetricSample {
                captured_at,
                rtt_ms,
                packet_loss: loss,
                jitter_ms: rtt_ms / 10.0,
                buffer_ms: rtt_ms,
                actual_fps: fps,
                reassembly_drop,
                decoder_drop,
            };

        history.record_for_test(sample(start, 10.0, 0.01, 30, 1, 0), start);
        history.record_for_test(
            sample(start + Duration::from_secs(300), 80.0, 0.04, 21, 2, 1),
            start + Duration::from_secs(300),
        );
        history.record_for_test(
            sample(start + Duration::from_secs(601), 40.0, 0.02, 25, 3, 0),
            start + Duration::from_secs(601),
        );

        let summary = history.summary();
        assert_eq!(summary.sample_count, 2);
        assert_eq!(summary.observed_seconds, 301);
        assert_eq!(summary.peak_rtt_ms, 80.0);
        assert_eq!(summary.peak_packet_loss, 0.04);
        assert_eq!(summary.minimum_fps, 21);
        assert_eq!(summary.reassembly_drops, 5);
        assert_eq!(summary.decoder_drops, 1);
    }
}
