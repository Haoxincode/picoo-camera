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
use super::vcam::vcam_label_zh;
use super::widgets::{
    connection_security_status, hardware_topology, live_hud_pill, network_status_row,
    onboarding_step, status_badge,
};
use super::{DesktopPage, PicooDesktopApp};

impl PicooDesktopApp {
    pub(super) fn render_connect(
        &self,
        snapshot: &ReceiverSnapshot,
        cx: &Context<Self>,
    ) -> impl IntoElement {
        // REQ-PICOO-UI-0001 / AC-D-LAYOUT-01: distribute all available
        // workspace width after the gap, instead of capping the page at 1160 px.
        div()
            .h_flex()
            .size_full()
            .min_h_0()
            .items_stretch()
            .gap_5()
            .child(
                div()
                    .v_flex()
                    .h_full()
                    .min_w_0()
                    .flex_basis(rems(0.))
                    .flex_grow(58.)
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
                    .min_w_0()
                    .flex_basis(rems(0.))
                    .flex_grow(42.)
                    .flex_shrink_1()
                    .justify_between()
                    .gap_4()
                    .child(self.render_device_connection_card(snapshot, cx))
                    .child(self.render_network_status_card(snapshot, cx)),
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
            .min_h(rems(35.))
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
            .child(
                div()
                    .h_flex()
                    .items_center()
                    .justify_between()
                    .gap_3()
                    .pt_3()
                    .border_t_1()
                    .border_color(cx.theme().border)
                    .text_xs()
                    .text_color(cx.theme().muted_foreground)
                    .child(
                        div()
                            .h_flex()
                            .items_center()
                            .gap_2()
                            .child(
                                div()
                                    .size_2()
                                    .flex_none()
                                    .rounded_full()
                                    .bg(cx.theme().success),
                            )
                            .child(format!("局域网监听中 · {DEFAULT_QUIC_PORT}")),
                    )
                    .child(
                        div()
                            .h_flex()
                            .items_center()
                            .gap_1p5()
                            .flex_none()
                            .child(
                                reicon_named("shield-check", cx.theme().primary).size(rems(0.875)),
                            )
                            .child("连接后使用加密传输"),
                    ),
            )
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

    pub(super) fn render_network_status_card(
        &self,
        snapshot: &ReceiverSnapshot,
        cx: &Context<Self>,
    ) -> impl IntoElement {
        let network_ready = snapshot.bind_addr.is_some()
            && !snapshot.advertise_host.is_empty()
            && snapshot.advertise_host != "127.0.0.1";
        let (security_status, security_ready) = connection_security_status(snapshot.status);
        let latency = if snapshot.stream_metrics.latency_ms <= 30.0 {
            "低"
        } else if snapshot.stream_metrics.latency_ms <= 80.0 {
            "一般"
        } else {
            "较高"
        };
        div()
            .v_flex()
            .gap_3()
            .p_5()
            .rounded(cx.theme().radius_lg)
            .border_1()
            .border_color(cx.theme().border)
            .bg(cx.theme().group_box)
            .when(cx.theme().shadow, |this| this.shadow_lg())
            .child(
                div()
                    .h_flex()
                    .gap_2()
                    .child(reicon_named("activity", cx.theme().primary))
                    .child(
                        div()
                            .text_sm()
                            .font_weight(FontWeight::SEMIBOLD)
                            .child("网络状态"),
                    ),
            )
            .child(network_status_row(
                "wifi",
                "网络",
                if network_ready {
                    "局域网可用"
                } else {
                    "未检测到局域网"
                },
                network_ready,
                cx,
            ))
            .child(network_status_row(
                "server",
                "发现服务",
                if snapshot.discovery_available {
                    "在线"
                } else {
                    "不可用"
                },
                snapshot.discovery_available,
                cx,
            ))
            .child(network_status_row(
                "activity",
                "延迟",
                latency,
                latency != "较高",
                cx,
            ))
            .child(network_status_row(
                "shield",
                "连接保护",
                security_status,
                security_ready,
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
            .map(|config| format!("{}p · {} FPS", config.height, config.fps))
            .unwrap_or_else(|| "—".into());
        let sender_name = snapshot
            .active_sender
            .as_ref()
            .map(|sender| sender.device_name.clone())
            .unwrap_or_else(|| "手机摄像头".into());
        let quality = crate::network_quality::network_quality_label(
            snapshot.stream_metrics.packet_loss,
            snapshot.stream_metrics.latency_ms,
        );
        let frame_status = snapshot
            .media_error
            .as_ref()
            .map(|error| format!("视频解码失败 · {error}"))
            .or_else(|| {
                (snapshot.ingress.decoded_frames == 0).then(|| "正在等待首个视频帧…".into())
            });

        div()
            .v_flex()
            .w_full()
            .overflow_hidden()
            .rounded(cx.theme().radius_lg)
            .border_1()
            .border_color(cx.theme().border)
            .bg(cx.theme().group_box)
            .when(cx.theme().shadow, |this| this.shadow_lg())
            .child(
                div().p_4().child(
                    div()
                        // REQ-PICOO-UI-0001 / AC-D-TECH-02: the preview surface,
                        // including its empty state, always occupies a 16:9 viewport.
                        .w_full()
                        .aspect_ratio(16. / 9.)
                        .flex_none()
                        .relative()
                        .rounded(cx.theme().radius)
                        .bg(cx.theme().muted)
                        .overflow_hidden()
                        .child(self.video_surface.render_preview())
                        .when_some(frame_status, |this, status| {
                            this.child(
                                div()
                                    .absolute()
                                    .inset_0()
                                    .flex()
                                    .items_center()
                                    .justify_center()
                                    .child(live_hud_pill(status, cx)),
                            )
                        })
                        .child(
                            div()
                                .absolute()
                                .top_3()
                                .left_4()
                                .right_4()
                                .h_flex()
                                .justify_between()
                                .child(live_hud_pill(format!("● {sender_name} · {res_label}"), cx))
                                .child(live_hud_pill(
                                    format!(
                                        "虚拟摄像头 · {}",
                                        vcam_label_zh(snapshot.virtual_camera)
                                    ),
                                    cx,
                                )),
                        ),
                ),
            )
            .child(
                div()
                    .h_flex()
                    .justify_between()
                    .items_center()
                    .p_4()
                    .border_t_1()
                    .border_color(cx.theme().border)
                    .child(
                        div()
                            .v_flex()
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
                                    .child(format!("{res_label} · 网络{quality}")),
                            ),
                    )
                    .child(
                        div()
                            .font_family(cx.theme().mono_font_family.clone())
                            .text_xs()
                            .text_color(cx.theme().success)
                            .child("● LIVE"),
                    ),
            )
    }
}

pub(super) fn endpoint_label(snapshot: &ReceiverSnapshot) -> String {
    if snapshot.advertise_host.is_empty() {
        return "—".into();
    }
    format!("{}:{DEFAULT_QUIC_PORT}", snapshot.advertise_host)
}
