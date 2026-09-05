use gpui_kit::component::*;
use gpui_kit::*;
use picoo_discovery::DEFAULT_QUIC_PORT;

use crate::live_diagnostics::{
    format_bitrate, format_milliseconds, format_packet_loss, DiagnosticAnalysis, DiagnosticHealth,
    HistorySummary,
};
use crate::receiver_runtime::ReceiverSnapshot;

use super::connect::endpoint_label;
use super::vcam::vcam_label_zh;
use super::widgets::{network_detail_row, page_header, section_header, status_badge};
use super::PicooDesktopApp;

impl PicooDesktopApp {
    /// Desktop is the only visible layered diagnostics surface
    /// (REQ-PICOO-UI-014 / AC-D-LIVE-03).
    pub(super) fn render_network_page(
        &self,
        snapshot: &ReceiverSnapshot,
        cx: &Context<Self>,
    ) -> impl IntoElement {
        let stats = snapshot.receiver_stats.as_ref();
        let history = snapshot.metrics_history;
        let target_fps = snapshot
            .stream_config
            .as_ref()
            .map_or(0, |config| config.fps);
        let analysis = DiagnosticAnalysis::from_metrics(
            stats,
            snapshot.stream_metrics.fps,
            target_fps,
            snapshot.media_error.as_deref(),
            snapshot.status,
            snapshot.virtual_camera,
            snapshot.shared_ring_error.as_deref(),
        );
        let actual_fps = stats.map(|_| {
            if target_fps > 0 {
                format!("{} / {} FPS", snapshot.stream_metrics.fps, target_fps)
            } else {
                format!("{} FPS", snapshot.stream_metrics.fps)
            }
        });
        let sender_quic_loss = stats.and_then(|stats| {
            (stats.sender_quic_sent_packets > 0).then(|| {
                format!(
                    "{:.2}%（{} / {} 包）",
                    stats.sender_quic_lost_packets as f64 * 100.0
                        / stats.sender_quic_sent_packets as f64,
                    stats.sender_quic_lost_packets,
                    stats.sender_quic_sent_packets
                )
            })
        });

        div()
            .v_flex()
            .max_w(rems(55.))
            .mx_auto()
            .gap_5()
            .child(page_header(
                "wifi",
                "网络",
                "查看局域网连接、实时媒体链路与最近运行质量",
                cx,
            ))
            .child(section_header("activity", "当前诊断", cx))
            .child(self.render_diagnostic_analysis(&analysis, cx))
            .child(section_header("wifi", "网络与传输", cx))
            .child(
                diagnostic_group(cx)
                    .child(network_detail_row(
                        "activity",
                        "接收码率",
                        "Receiver 最近一个完整统计窗口",
                        format_bitrate(stats),
                        cx,
                    ))
                    .child(network_detail_row(
                        "radio",
                        "链路延迟",
                        "QUIC 往返时延，不是端到端拍摄延迟",
                        format_milliseconds(stats.map(|stats| stats.rtt_ms)),
                        cx,
                    ))
                    .child(network_detail_row(
                        "timer",
                        "端到端延迟",
                        "Sender 与 Receiver 时钟映射稳定后，从采集到当前统计快照",
                        format_milliseconds(stats.and_then(|stats| stats.end_to_end_latency_ms)),
                        cx,
                    ))
                    .child(network_detail_row(
                        "activity",
                        "分段延迟",
                        "采集→编码 / 编码→到达 / 抖动驻留 / 解码 / 发布后帧龄",
                        format!(
                            "{} / {} / {} / {} / {}",
                            format_milliseconds(stats.and_then(|stats| stats.capture_to_encode_ms)),
                            format_milliseconds(stats.and_then(|stats| stats.encode_to_arrival_ms)),
                            format_milliseconds(stats.and_then(|stats| stats.jitter_residence_ms)),
                            format_milliseconds(stats.and_then(|stats| stats.decode_ms)),
                            format_milliseconds(stats.and_then(|stats| stats.frame_publish_age_ms)),
                        ),
                        cx,
                    ))
                    .child(network_detail_row(
                        "monitor",
                        "时钟映射不确定度",
                        "基于低 RTT 样本拟合的当前误差上界；未稳定时不显示总延迟",
                        format_milliseconds(stats.and_then(|stats| stats.clock_uncertainty_ms)),
                        cx,
                    ))
                    .child(network_detail_row(
                        "activity",
                        "网络抖动",
                        "完整视频帧到达间隔相对 PTS 间隔的波动",
                        format_milliseconds(stats.map(|stats| stats.jitter_ms)),
                        cx,
                    ))
                    .child(network_detail_row(
                        "server",
                        "可观测丢片",
                        "已决视频帧内确认缺失的 fragment 比例，不代表网卡总丢包",
                        format_packet_loss(stats),
                        cx,
                    ))
                    .child(network_detail_row(
                        "activity",
                        "Sender QUIC 待发送",
                        "手机端 Quinn Datagram 缓冲当前占用；持续增长代表排队延迟",
                        stats
                            .map(|stats| format!("{} KB", stats.sender_video_buffered_bytes / 1024))
                            .unwrap_or_else(|| "—".into()),
                        cx,
                    ))
                    .child(network_detail_row(
                        "activity",
                        "Sender 队列丢帧",
                        "手机端因有界发送队列或 Datagram 缓冲不足而整帧丢弃的累计值",
                        stats
                            .map(|stats| format!("{} 帧", stats.sender_queue_dropped_access_units))
                            .unwrap_or_else(|| "—".into()),
                        cx,
                    ))
                    .child(network_detail_row(
                        "radio",
                        "Sender QUIC 丢包",
                        "手机端 QUIC 路径累计确认丢失的包，占累计发送包比例",
                        sender_quic_loss.unwrap_or_else(|| "—".into()),
                        cx,
                    )),
            )
            .child(section_header("monitor", "媒体处理", cx))
            .child(
                diagnostic_group(cx)
                    .child(network_detail_row(
                        "camera",
                        "实际 / 目标帧率",
                        "Receiver 解码提交帧率与当前协商目标",
                        actual_fps.unwrap_or_else(|| "—".into()),
                        cx,
                    ))
                    .child(network_detail_row(
                        "activity",
                        "播放目标",
                        "由到达变化、帧周期和解码耗时计算的当前总时序预算",
                        format_milliseconds(stats.map(|stats| stats.jitter_buffer_target_ms)),
                        cx,
                    ))
                    .child(network_detail_row(
                        "activity",
                        "实际缓冲停留",
                        "本窗口视频帧从首片到达至离开 Jitter Buffer 的平均时间",
                        format_milliseconds(stats.map(|stats| stats.jitter_buffer_actual_delay_ms)),
                        cx,
                    ))
                    .child(network_detail_row(
                        "activity",
                        "队列跨度",
                        "当前已完成视频帧队列最新与最旧 PTS 的跨度",
                        format_milliseconds(stats.map(|stats| stats.jitter_buffer_occupancy_ms)),
                        cx,
                    ))
                    .child(network_detail_row(
                        "timer",
                        "接收入口队列龄",
                        "Quinn 收到首个 Datagram 后，到 Receiver 开始处理该批次的本机等待峰值",
                        format_milliseconds(stats.map(|stats| stats.receive_queue_age_ms)),
                        cx,
                    ))
                    .child(network_detail_row(
                        "refresh",
                        "入口过期整帧",
                        "进入重组前已超过媒体截止时间、按完整 Access Unit 淘汰的累计值",
                        format!("{} 帧", snapshot.ingress.receive_queue_expired_access_units),
                        cx,
                    ))
                    .child(network_detail_row(
                        "refresh",
                        "最近重组丢弃",
                        "最近统计窗口内被重组层确认丢弃的 Access Unit",
                        format_count(stats.map(|stats| stats.reassembly_drop), "帧"),
                        cx,
                    ))
                    .child(network_detail_row(
                        "shield",
                        "FEC 已恢复数据片",
                        "Receiver 本进程内无需重传、已由校验片即时恢复的累计 fragment",
                        format!("{} 片", snapshot.ingress.fec_recovered_fragments),
                        cx,
                    ))
                    .child(network_detail_row(
                        "refresh",
                        "部分帧重组失败",
                        "至少收到一个数据片、但在自适应截止前仍无法完成的累计 Access Unit",
                        format!(
                            "{} 帧",
                            snapshot.ingress.reassembly_partial_access_unit_drops
                        ),
                        cx,
                    ))
                    .child(network_detail_row(
                        "refresh",
                        "整帧未到",
                        "由连续 frame id 推断、截止前没有任何片到达的累计 Access Unit",
                        format!(
                            "{} 帧",
                            snapshot.ingress.reassembly_whole_access_unit_gap_drops
                        ),
                        cx,
                    ))
                    .child(network_detail_row(
                        "activity",
                        "本地时序恢复",
                        "容量淘汰 / 播放后晚到 / 完整帧超时，三类累计恢复次数",
                        format!(
                            "{} / {} / {} 次",
                            snapshot.ingress.recovery_jitter_capacity,
                            snapshot.ingress.recovery_arrived_after_playout,
                            snapshot.ingress.recovery_jitter_expired
                        ),
                        cx,
                    ))
                    .child(network_detail_row(
                        "monitor",
                        "最近解码丢帧",
                        "最近统计窗口内平台解码器未提交的帧",
                        format_count(stats.map(|stats| stats.decoder_drop), "帧"),
                        cx,
                    ))
                    .child(network_detail_row(
                        "activity",
                        "本地帧龄",
                        "已解码帧在 Receiver 本地的驻留时间",
                        format_milliseconds(stats.map(|stats| stats.frame_age_ms)),
                        cx,
                    ))
                    .child(network_detail_row(
                        "refresh",
                        "关键帧恢复请求",
                        "Receiver 进程累计发送到可靠控制流的请求",
                        format!("{} 次", snapshot.ingress.keyframe_requests),
                        cx,
                    )),
            )
            .child(section_header("monitor-camera", "输出链路", cx))
            .child(
                diagnostic_group(cx)
                    .child(network_detail_row(
                        "monitor-camera",
                        "虚拟摄像头",
                        "操作系统当前枚举与激活状态",
                        vcam_label_zh(snapshot.virtual_camera).into(),
                        cx,
                    ))
                    .child(network_detail_row(
                        "server",
                        "共享帧环",
                        "Receiver 向虚拟摄像头发布 NV12 帧的进程边界",
                        snapshot
                            .shared_ring_error
                            .as_ref()
                            .map(|_| "异常".into())
                            .unwrap_or_else(|| "可用".into()),
                        cx,
                    )),
            )
            .child(section_header("activity", "最近 10 分钟", cx))
            .child(self.render_history_summary(history, cx))
            .child(section_header("radio", "自动发现", cx))
            .child(
                div()
                    .h_flex()
                    .items_center()
                    .justify_between()
                    .gap_4()
                    .p_5()
                    .rounded(cx.theme().radius_lg)
                    .border_1()
                    .border_color(cx.theme().border)
                    .bg(cx.theme().group_box)
                    .child(
                        div()
                            .v_flex()
                            .gap_1()
                            .child(
                                div()
                                    .text_sm()
                                    .font_weight(FontWeight::MEDIUM)
                                    .child("自动发现附近设备"),
                            )
                            .child(
                                div()
                                    .text_xs()
                                    .text_color(cx.theme().muted_foreground)
                                    .child("允许同一局域网中的 Picoo Camera 自动发现这台电脑。"),
                            ),
                    )
                    .child(status_badge(
                        if snapshot.discovery_available {
                            "在线"
                        } else {
                            "不可用"
                        },
                        snapshot.discovery_available,
                        cx,
                    )),
            )
            .child(section_header("tuning", "连接设置", cx))
            .child(
                diagnostic_group(cx)
                    .child(network_detail_row(
                        "server",
                        "连接端口",
                        "视频与控制连接使用的 UDP 端口",
                        DEFAULT_QUIC_PORT.to_string(),
                        cx,
                    ))
                    .child(network_detail_row(
                        "wifi",
                        "监听地址",
                        "手机自动发现不可用时可手动输入",
                        endpoint_label(snapshot),
                        cx,
                    ))
                    .child(network_detail_row(
                        "monitor",
                        "Receiver 状态",
                        "当前桌面接收端会话状态",
                        Self::status_label(snapshot.status).into(),
                        cx,
                    )),
            )
    }

    fn render_diagnostic_analysis(
        &self,
        analysis: &DiagnosticAnalysis,
        cx: &Context<Self>,
    ) -> impl IntoElement {
        div()
            .h_flex()
            .w_full()
            .items_center()
            .justify_between()
            .gap_5()
            .p_5()
            .rounded(cx.theme().radius_lg)
            .border_1()
            .border_color(cx.theme().border)
            .bg(cx.theme().group_box)
            .child(
                div()
                    .v_flex()
                    .flex_1()
                    .min_w_0()
                    .gap_1()
                    .child(
                        div()
                            .text_sm()
                            .font_weight(FontWeight::SEMIBOLD)
                            .child("主要判断"),
                    )
                    .child(
                        div()
                            .text_xs()
                            .text_color(cx.theme().muted_foreground)
                            .child(analysis.message.clone()),
                    ),
            )
            .child(diagnostic_status_badge(analysis, cx))
    }

    fn render_history_summary(
        &self,
        history: HistorySummary,
        cx: &Context<Self>,
    ) -> impl IntoElement {
        let has_samples = history.sample_count > 0;
        diagnostic_group(cx)
            .child(network_detail_row(
                "activity",
                "采样范围",
                "每秒一次，仅保存在当前桌面进程内存中",
                if has_samples {
                    format!(
                        "{} 个样本 · {}",
                        history.sample_count,
                        format_duration(history.observed_seconds)
                    )
                } else {
                    "等待样本".into()
                },
                cx,
            ))
            .child(network_detail_row(
                "radio",
                "峰值链路延迟 / 抖动",
                "窗口内用于定位瞬时 Wi-Fi 波动的峰值",
                if has_samples {
                    format!(
                        "{:.1} ms / {:.1} ms",
                        history.peak_rtt_ms, history.peak_jitter_ms
                    )
                } else {
                    "—".into()
                },
                cx,
            ))
            .child(network_detail_row(
                "server",
                "峰值可观测丢片 / 缓冲",
                "丢片仍只覆盖 Receiver 能观察到的已决视频帧",
                if has_samples {
                    format!(
                        "{:.2}% / {:.1} ms",
                        history.peak_packet_loss * 100.0,
                        history.peak_buffer_ms
                    )
                } else {
                    "—".into()
                },
                cx,
            ))
            .child(network_detail_row(
                "camera",
                "最低实际帧率",
                "窗口内 Receiver 解码提交帧率的最低值",
                if has_samples {
                    format!("{} FPS", history.minimum_fps)
                } else {
                    "—".into()
                },
                cx,
            ))
            .child(network_detail_row(
                "refresh",
                "累计重组 / 解码丢帧",
                "同一秒级窗口计数求和，不混用 Datagram 总数",
                if has_samples {
                    format!(
                        "{} / {} 帧",
                        history.reassembly_drops, history.decoder_drops
                    )
                } else {
                    "—".into()
                },
                cx,
            ))
    }
}

fn diagnostic_group(cx: &Context<PicooDesktopApp>) -> Div {
    div()
        .v_flex()
        .overflow_hidden()
        .rounded(cx.theme().radius_lg)
        .border_1()
        .border_color(cx.theme().border)
        .bg(cx.theme().group_box)
}

fn diagnostic_status_badge(
    analysis: &DiagnosticAnalysis,
    cx: &Context<PicooDesktopApp>,
) -> impl IntoElement {
    let color = match analysis.health {
        DiagnosticHealth::Waiting => cx.theme().muted_foreground,
        DiagnosticHealth::Healthy => cx.theme().success,
        DiagnosticHealth::Attention => cx.theme().warning,
        DiagnosticHealth::Poor => cx.theme().danger,
    };
    div()
        .h_flex()
        .flex_none()
        .gap_1()
        .px_2()
        .py_0p5()
        .rounded(cx.theme().radius_full())
        .border_1()
        .border_color(color.opacity(0.35))
        .bg(color.opacity(0.10))
        .text_xs()
        .text_color(color)
        .child(div().size_1p5().rounded(cx.theme().radius_full()).bg(color))
        .child(analysis.label)
}

fn format_count(value: Option<u64>, unit: &str) -> String {
    value
        .map(|value| format!("{value} {unit}"))
        .unwrap_or_else(|| "—".into())
}

fn format_duration(seconds: u64) -> String {
    let minutes = seconds / 60;
    let seconds = seconds % 60;
    if minutes == 0 {
        format!("{seconds} 秒")
    } else {
        format!("{minutes} 分 {seconds} 秒")
    }
}

#[cfg(test)]
mod tests {
    use super::{format_count, format_duration};

    #[test]
    fn diagnostic_copy_distinguishes_missing_counts_and_formats_window() {
        assert_eq!(format_count(None, "帧"), "—");
        assert_eq!(format_count(Some(0), "帧"), "0 帧");
        assert_eq!(format_duration(0), "0 秒");
        assert_eq!(format_duration(125), "2 分 5 秒");
    }
}
