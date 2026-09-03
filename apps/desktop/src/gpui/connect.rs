use gpui::prelude::FluentBuilder;
use gpui::*;
use gpui_component::button::*;
use gpui_component::notification::NotificationType;
use gpui_component::*;
use picoo_discovery::DEFAULT_QUIC_PORT;
use picoo_protocol::control::CameraCommand;

use crate::model::VirtualCameraStatus;
use crate::receiver_runtime::ReceiverSnapshot;

use super::icons::reicon_named;
use super::pairing::{connection_code_hero, format_pairing_code};
use super::widgets::{
    connection_security_status, hardware_topology, network_status_item, onboarding_step,
    status_badge, NetworkStatusState,
};
use super::{DesktopPage, PicooDesktopApp};

const CONNECT_AUXILIARY_MIN_WIDTH: Rems = rems(21.);
const CONNECT_AUXILIARY_WIDTH: Rems = rems(24.);
const CONNECT_LIVE_AUXILIARY_WIDTH: Rems = rems(18.);

impl PicooDesktopApp {
    pub(super) fn render_connect(
        &self,
        snapshot: &ReceiverSnapshot,
        cx: &Context<Self>,
    ) -> impl IntoElement {
        let is_live = self.page == DesktopPage::Live;
        let auxiliary_min_width = if is_live {
            CONNECT_LIVE_AUXILIARY_WIDTH
        } else {
            CONNECT_AUXILIARY_MIN_WIDTH
        };
        let auxiliary_width = if is_live {
            CONNECT_LIVE_AUXILIARY_WIDTH
        } else {
            CONNECT_AUXILIARY_WIDTH
        };

        // REQ-PICOO-UI-0001 / AC-D-LAYOUT-01 / AC-D-LIVE-01: the primary pane
        // consumes all surplus width. Live narrows the auxiliary inspector so the
        // camera remains the screen's largest visual object at the minimum window.
        div()
            .h_flex()
            .size_full()
            .min_w_0()
            .min_h_0()
            .items_stretch()
            .when(is_live, |this| this.gap_4())
            .when(!is_live, |this| this.gap_5())
            .child(
                div()
                    .v_flex()
                    .h_full()
                    .min_w_0()
                    .flex_1()
                    .flex_shrink_1()
                    .child(if self.page == DesktopPage::Live {
                        self.render_live(snapshot, cx).into_any_element()
                    } else {
                        self.render_waiting(snapshot, cx).into_any_element()
                    }),
            )
            .child(
                div()
                    .v_flex()
                    .h_full()
                    .w(auxiliary_width)
                    .min_w(auxiliary_min_width)
                    .max_w(auxiliary_width)
                    .flex_shrink_1()
                    .child(self.render_device_connection_card(snapshot, cx)),
            )
    }

    pub(super) fn render_waiting(
        &self,
        snapshot: &ReceiverSnapshot,
        cx: &Context<Self>,
    ) -> impl IntoElement {
        let vcam_ready = snapshot.virtual_camera == VirtualCameraStatus::Active;
        let vcam_status_label = if vcam_ready {
            "接收端已就绪"
        } else if snapshot.virtual_camera == VirtualCameraStatus::Installed {
            "等待系统发布"
        } else {
            "需要修复"
        };
        let pairing_code = snapshot
            .pairing_short_code
            .as_deref()
            .map(format_pairing_code)
            .unwrap_or_else(|| "等待请求".into());

        div()
            .v_flex()
            .flex_1()
            .h_full()
            .min_w_0()
            .min_h_0()
            .justify_between()
            .gap_5()
            .p_5()
            .rounded(cx.theme().radius_lg)
            .border_1()
            .border_color(cx.theme().border)
            .bg(cx.theme().group_box)
            .when(cx.theme().shadow, |this| this.shadow_lg())
            .child(
                div()
                    .h_flex()
                    .items_center()
                    .gap_3()
                    .pb_3()
                    .border_b_1()
                    .border_color(cx.theme().border)
                    .child(
                        div()
                            .size_6()
                            .flex()
                            .items_center()
                            .justify_center()
                            .rounded(cx.theme().radius_lg)
                            .border_1()
                            .border_color(cx.theme().border)
                            .bg(cx.theme().secondary)
                            .child(reicon_named("monitor", cx.theme().primary)),
                    )
                    .child(
                        div()
                            .h_flex()
                            .gap_2()
                            .items_center()
                            .child(
                                div()
                                    .font_weight(FontWeight::BOLD)
                                    .child(snapshot.display_name.clone()),
                            )
                            .child(status_badge(vcam_status_label, vcam_ready, cx)),
                    ),
            )
            .child(
                div()
                    .v_flex()
                    .items_center()
                    .text_center()
                    .gap_3()
                    .child(
                        div()
                            .h_flex()
                            .items_center()
                            .gap_1p5()
                            .text_xs()
                            .font_weight(FontWeight::MEDIUM)
                            .text_color(cx.theme().muted_foreground)
                            .child(reicon_named("key", cx.theme().primary).size(rems(0.875)))
                            .child("配对短码"),
                    )
                    .child(connection_code_hero(&pairing_code, cx))
                    .child(
                        div()
                            .text_xs()
                            .text_color(cx.theme().muted_foreground)
                            .child(if snapshot.pairing_short_code.is_some() {
                                "请核对手机上显示的相同数字"
                            } else {
                                "手机选择这台电脑后，配对短码将在此显示"
                            }),
                    )
                    .when(snapshot.pairing_short_code.is_some(), |this| {
                        this.child(
                            div()
                                .h_flex()
                                .items_center()
                                .gap_2()
                                .child(
                                    Button::new("confirm-pairing-inline")
                                        .primary()
                                        .label(if self.pairing_locally_confirmed {
                                            "已确认，等待手机"
                                        } else {
                                            "两端一致，确认配对"
                                        })
                                        .disabled(self.pairing_locally_confirmed)
                                        .on_click(cx.listener(|this, _, window, cx| {
                                            match this.confirm_pairing_request(cx) {
                                                Ok(()) => {
                                                    if window.has_active_dialog(cx) {
                                                        window.close_dialog(cx);
                                                    }
                                                    window.push_notification(
                                                        (
                                                            NotificationType::Success,
                                                            "电脑端已确认，正在等待手机完成配对",
                                                        ),
                                                        cx,
                                                    );
                                                }
                                                Err(message) => window.push_notification(
                                                    (NotificationType::Error, message),
                                                    cx,
                                                ),
                                            }
                                        })),
                                )
                                .child(
                                    Button::new("reject-pairing-inline")
                                        .outline()
                                        .danger()
                                        .label("拒绝")
                                        .on_click(cx.listener(|this, _, window, cx| {
                                            match this.reject_pairing_request(cx) {
                                                Ok(()) => {
                                                    if window.has_active_dialog(cx) {
                                                        window.close_dialog(cx);
                                                    }
                                                }
                                                Err(message) => window.push_notification(
                                                    (NotificationType::Error, message),
                                                    cx,
                                                ),
                                            }
                                        })),
                                ),
                        )
                    })
                    .child(self.render_manual_endpoint_card(snapshot, cx)),
            )
            .child(
                div()
                    .h_flex()
                    .items_stretch()
                    .gap_4()
                    .p_4()
                    .rounded(cx.theme().radius)
                    .border_1()
                    .border_color(cx.theme().border)
                    .bg(cx.theme().secondary.opacity(0.45))
                    .child(
                        div()
                            .v_flex()
                            .flex_1()
                            .min_w_0()
                            .gap_3()
                            .child(
                                div()
                                    .h_flex()
                                    .items_center()
                                    .gap_2()
                                    .text_sm()
                                    .font_weight(FontWeight::SEMIBOLD)
                                    .child(reicon_named("rocket", cx.theme().primary).size_4())
                                    .child("开始使用"),
                            )
                            .child(
                                div()
                                    .v_flex()
                                    .gap_2()
                                    .child(onboarding_step("1", "mobile", "打开 Picoo Camera", cx))
                                    .child(onboarding_step("2", "monitor", "选择此电脑", cx))
                                    .child(onboarding_step(
                                        "3",
                                        "play-filled",
                                        "核对短码并确认",
                                        cx,
                                    )),
                            ),
                    )
                    .child(
                        div()
                            .v_flex()
                            .flex_1()
                            .min_w_0()
                            .justify_center()
                            .p_3()
                            .rounded(cx.theme().radius)
                            .border_1()
                            .border_color(cx.theme().border)
                            .bg(cx.theme().secondary.opacity(0.55))
                            // REQ-PICOO-UI-0001 / AC-D-ONBOARDING-03:
                            // stretch against the complete title + three-step column.
                            .child(hardware_topology(cx)),
                    ),
            )
            .child(self.render_network_status_bar(snapshot, cx))
    }

    pub(super) fn render_manual_endpoint_card(
        &self,
        snapshot: &ReceiverSnapshot,
        cx: &Context<Self>,
    ) -> impl IntoElement {
        let endpoint = endpoint_label(snapshot);
        Button::new("copy-listen-endpoint")
            .outline()
            .label(endpoint.clone())
            .child(reicon_named("copy", cx.theme().primary))
            .on_click(cx.listener(move |this, _, _, cx| {
                cx.write_to_clipboard(ClipboardItem::new_string(endpoint.clone()));
                this.diagnostics_error = None;
                this.diagnostics_message = Some("监听地址已复制".into());
                cx.notify();
            }))
    }

    pub(super) fn render_network_status_bar(
        &self,
        snapshot: &ReceiverSnapshot,
        cx: &Context<Self>,
    ) -> impl IntoElement {
        let network_ready = snapshot.bind_addr.is_some()
            && !snapshot.advertise_host.is_empty()
            && snapshot.advertise_host != "127.0.0.1";
        let (security_status, security_ready) = connection_security_status(snapshot.status);
        let (latency, latency_state) = match snapshot.receiver_stats.as_ref() {
            None => ("待测", NetworkStatusState::Pending),
            Some(stats) if stats.rtt_ms <= 30.0 => ("低", NetworkStatusState::Healthy),
            Some(stats) if stats.rtt_ms <= 80.0 => ("一般", NetworkStatusState::Healthy),
            Some(_) => ("较高", NetworkStatusState::Warning),
        };
        div()
            .h_flex()
            .w_full()
            .min_w_0()
            .flex_none()
            .items_stretch()
            .px_2()
            .py_3()
            .border_t_1()
            .border_color(cx.theme().border)
            .child(network_status_item(
                "wifi",
                if network_ready {
                    "网络可用"
                } else {
                    "网络异常"
                },
                if network_ready {
                    NetworkStatusState::Healthy
                } else {
                    NetworkStatusState::Warning
                },
                false,
                cx,
            ))
            .child(network_status_item(
                "server",
                if snapshot.discovery_available {
                    "发现在线"
                } else {
                    "发现异常"
                },
                if snapshot.discovery_available {
                    NetworkStatusState::Healthy
                } else {
                    NetworkStatusState::Warning
                },
                true,
                cx,
            ))
            .child(network_status_item(
                "activity",
                format!("延迟{latency}"),
                latency_state,
                true,
                cx,
            ))
            .child(network_status_item(
                "shield",
                security_status,
                if security_ready {
                    NetworkStatusState::Healthy
                } else {
                    NetworkStatusState::Pending
                },
                true,
                cx,
            ))
    }

    pub(super) fn send_live_camera_command(&mut self, command: CameraCommand) {
        // REQ-PICOO-UI-009 / PUC-005: desktop Live remote camera controls.
        match self.runtime.send_camera_command(command) {
            Ok(()) => {
                self.diagnostics_error = None;
            }
            Err(err) => {
                tracing::warn!("CameraCommand failed: {err}");
                self.diagnostics_error = Some(format!("远程摄像头控制失败：{err}"));
            }
        }
    }

    pub(super) fn render_live(
        &self,
        snapshot: &ReceiverSnapshot,
        cx: &Context<Self>,
    ) -> impl IntoElement {
        let res_label = snapshot
            .stream_config
            .as_ref()
            .map(|config| {
                if snapshot.receiver_stats.is_some() {
                    format!(
                        "{}p · {} FPS 实测",
                        config.height, snapshot.stream_metrics.fps
                    )
                } else {
                    format!("{}p · {} FPS 目标", config.height, config.fps)
                }
            })
            .unwrap_or_else(|| "—".into());
        let quality = snapshot.receiver_stats.as_ref().map(|stats| {
            crate::network_quality::network_quality_label(stats.packet_loss, stats.rtt_ms)
        });
        let live_metrics = snapshot
            .receiver_stats
            .as_ref()
            .map(|stats| {
                format!(
                    "{} · {:.1} Mbps · {:.0} ms · 可观测丢片 {:.2}% · 网络{}",
                    res_label,
                    stats.receive_bitrate as f64 / 1_000_000.0,
                    stats.rtt_ms,
                    stats.packet_loss * 100.0,
                    quality.expect("quality exists with ReceiverStats")
                )
            })
            .unwrap_or_else(|| format!("{res_label} · 等待链路统计样本"));
        div()
            .v_flex()
            .w_full()
            .h_full()
            .min_w_0()
            .min_h_0()
            .flex_1()
            .overflow_hidden()
            .rounded(cx.theme().radius_lg)
            .border_1()
            .border_color(cx.theme().border)
            .bg(cx.theme().group_box)
            .when(cx.theme().shadow, |this| this.shadow_lg())
            .child(
                div()
                    .flex()
                    .flex_1()
                    .min_w_0()
                    .min_h_0()
                    .items_center()
                    .justify_center()
                    .p_3()
                    .child(
                        div()
                            // REQ-PICOO-UI-0001 / AC-D-LIVE-01: width proposes the
                            // largest viewport while max-height transfers through the
                            // aspect ratio, so either axis can constrain without cropping.
                            .w_full()
                            .max_h_full()
                            .aspect_ratio(16. / 9.)
                            .flex_none()
                            .relative()
                            .rounded(cx.theme().radius)
                            .bg(cx.theme().muted)
                            .overflow_hidden()
                            .child(self.video_surface.render_preview()),
                    ),
            )
            .child(
                div()
                    .h_flex()
                    .w_full()
                    .min_w_0()
                    .items_center()
                    .p_4()
                    .border_t_1()
                    .border_color(cx.theme().border)
                    .child(
                        div()
                            .v_flex()
                            .min_w_0()
                            .flex_1()
                            .gap_0p5()
                            .child(
                                div()
                                    .text_sm()
                                    .font_weight(FontWeight::SEMIBOLD)
                                    .child("实时视频监视器"),
                            )
                            .child(
                                div()
                                    .text_xs()
                                    .text_color(cx.theme().muted_foreground)
                                    .truncate()
                                    .whitespace_nowrap()
                                    .child(live_metrics),
                            ),
                    ),
            )
            .child(self.render_network_status_bar(snapshot, cx))
    }
}

pub(super) fn endpoint_label(snapshot: &ReceiverSnapshot) -> String {
    if snapshot.advertise_host.is_empty() {
        return "—".into();
    }
    format!("{}:{DEFAULT_QUIC_PORT}", snapshot.advertise_host)
}
