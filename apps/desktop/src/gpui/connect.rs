use gpui_kit::base::ElementExt;
use gpui_kit::component::button::*;
use gpui_kit::component::menu::DropdownMenu;
use gpui_kit::component::notification::NotificationType;
use gpui_kit::component::popover::Popover;
use gpui_kit::component::separator::Separator;
use gpui_kit::component::*;
use gpui_kit::prelude::FluentBuilder;
use gpui_kit::*;
use picoo_discovery::DEFAULT_QUIC_PORT;
use picoo_protocol::control::{camera_command, CameraCommand, Resolution};
use serde::Deserialize;

use crate::model::VirtualCameraStatus;
use crate::receiver_runtime::ReceiverSnapshot;

use super::icons::{reicon_button_content, reicon_named};
use super::pairing::{connection_code_hero, format_pairing_code};
use super::widgets::{
    connection_security_status, hardware_topology, live_metric_text, live_network_quality,
    live_preview_badge, metric_row, network_status_item, onboarding_step, rounded_preview_corners,
    status_badge, NetworkStatusState,
};
use super::{DesktopPage, PicooDesktopApp};

const CONNECT_AUXILIARY_MIN_WIDTH: Rems = rems(21.);
const CONNECT_AUXILIARY_WIDTH: Rems = rems(24.);

#[derive(Clone, Action, PartialEq, Eq, Deserialize)]
#[action(namespace = picoo_live_toolbar, no_json)]
enum LiveToolbarAction {
    Resolution480,
    Resolution720,
    Resolution1080,
}

impl PicooDesktopApp {
    pub(super) fn render_connect(
        &self,
        snapshot: &ReceiverSnapshot,
        cx: &Context<Self>,
    ) -> impl IntoElement {
        let is_live = self.page == DesktopPage::Live;
        // REQ-PICOO-UI-0001 / AC-D-LAYOUT-01 / AC-D-LIVE-01: waiting keeps the
        // trusted-device inspector; Live removes it and gives the entire workspace
        // to the fixed single-row command bar and camera preview.
        div()
            .h_flex()
            .size_full()
            .min_w_0()
            .min_h_0()
            .items_stretch()
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
            .when(!is_live, |this| {
                this.child(
                    div()
                        .v_flex()
                        .h_full()
                        .w(CONNECT_AUXILIARY_WIDTH)
                        .min_w(CONNECT_AUXILIARY_MIN_WIDTH)
                        .max_w(CONNECT_AUXILIARY_WIDTH)
                        .flex_shrink_1()
                        .child(self.render_device_connection_card(snapshot, cx)),
                )
            })
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
        let sender_name = snapshot
            .active_sender
            .as_ref()
            .map(|sender| sender.device_name.clone())
            .unwrap_or_else(|| "当前设备".into());
        let current_resolution_height = snapshot.stream_config.as_ref().map(|config| config.height);
        let resolution_label = current_resolution_height
            .map(|height| format!("{height}p"))
            .unwrap_or_else(|| "分辨率".into());
        let remote_mirrored = snapshot
            .stream_config
            .as_ref()
            .map(|config| config.mirrored)
            .unwrap_or(false);
        let mirror_icon_color = if remote_mirrored {
            cx.theme().primary_foreground
        } else {
            cx.theme().primary
        };
        let fps_label = snapshot
            .receiver_stats
            .as_ref()
            .map(|_| format!("{} FPS", snapshot.stream_metrics.fps))
            .unwrap_or_else(|| "— FPS".into());
        let latency_label = snapshot
            .receiver_stats
            .as_ref()
            .map(|stats| format!("{:.0} ms", stats.rtt_ms))
            .unwrap_or_else(|| "— ms".into());
        let bitrate_label = snapshot
            .receiver_stats
            .as_ref()
            .map(|stats| format!("{:.1} Mbps", stats.receive_bitrate as f64 / 1_000_000.0))
            .unwrap_or_else(|| "— Mbps".into());
        let (quality_label, quality_color, quality_ready) =
            snapshot.receiver_stats.as_ref().map_or_else(
                || ("网络待测".to_string(), cx.theme().muted_foreground, false),
                |stats| {
                    let quality = crate::network_quality::network_quality_label(
                        stats.packet_loss,
                        stats.rtt_ms,
                    )
                    .split_whitespace()
                    .next()
                    .unwrap_or("待测");
                    let ready = quality != "较差";
                    let color = if ready {
                        cx.theme().success
                    } else {
                        cx.theme().warning
                    };
                    (format!("网络{quality}"), color, ready)
                },
            );
        let active_identity = snapshot
            .active_sender
            .as_ref()
            .and_then(|sender| {
                snapshot
                    .trusted_devices
                    .iter()
                    .find(|device| device.device_id == sender.sender_id)
                    .map(|device| device.identity_prefix.clone())
            })
            .unwrap_or_else(|| "—".into());
        let video_spec = snapshot
            .stream_config
            .as_ref()
            .map(|config| format!("H.264 · {}×{}", config.width, config.height))
            .unwrap_or_else(|| "H.264 · —".into());
        let packet_loss_label = snapshot
            .receiver_stats
            .as_ref()
            .map(|stats| format!("{:.2}%", stats.packet_loss * 100.0))
            .unwrap_or_else(|| "—".into());
        let playback_buffer_label = snapshot
            .receiver_stats
            .as_ref()
            .map(|_| {
                format!(
                    "{:.1} / {:.1} ms",
                    snapshot.jitter_buffer_actual_delay_ms, snapshot.jitter_buffer_target_ms
                )
            })
            .unwrap_or_else(|| "—".into());
        let details = Popover::new("live-connection-details")
            .anchor(Anchor::TopRight)
            .w(rems(20.))
            .trigger(
                Button::new("live-connection-details-trigger")
                    .outline()
                    .small()
                    .tooltip("查看连接详情")
                    .accessibility_label("连接详情")
                    .child(reicon_button_content(
                        "连接详情",
                        "tuning",
                        cx.theme().primary,
                    )),
            )
            .child(
                div()
                    .v_flex()
                    .gap_2()
                    .child(
                        div()
                            .text_sm()
                            .font_weight(FontWeight::SEMIBOLD)
                            .child("连接详情"),
                    )
                    .child(Separator::horizontal())
                    .child(metric_row("身份", active_identity, cx))
                    .child(metric_row("视频规格", video_spec, cx))
                    .child(metric_row(
                        "虚拟摄像头",
                        super::vcam::vcam_label_zh(snapshot.virtual_camera).into(),
                        cx,
                    ))
                    .child(metric_row("可观测丢片", packet_loss_label, cx))
                    .child(metric_row(
                        "网络抖动",
                        snapshot
                            .receiver_stats
                            .as_ref()
                            .map(|_| format!("{:.1} ms", snapshot.link_jitter_ms))
                            .unwrap_or_else(|| "—".into()),
                        cx,
                    ))
                    .child(metric_row(
                        "播放缓冲 实际 / 目标",
                        playback_buffer_label,
                        cx,
                    )),
            );
        let resolution = Button::new("live-resolution-trigger")
            .outline()
            .small()
            .tooltip("选择推流分辨率")
            .accessibility_label("推流分辨率")
            .child(
                div()
                    .h_flex()
                    .gap_2()
                    .child(reicon_named("monitor", cx.theme().primary))
                    .child(resolution_label),
            )
            .dropdown_menu_with_anchor(Anchor::TopRight, move |menu, _, _| {
                menu.menu_with_check(
                    "480p",
                    current_resolution_height == Some(480),
                    Box::new(LiveToolbarAction::Resolution480),
                )
                .menu_with_check(
                    "720p",
                    current_resolution_height == Some(720),
                    Box::new(LiveToolbarAction::Resolution720),
                )
                .menu_with_check(
                    "1080p",
                    current_resolution_height == Some(1080),
                    Box::new(LiveToolbarAction::Resolution1080),
                )
            });

        let preview_viewport = self.preview_viewport.clone();
        div()
            .v_flex()
            .w_full()
            .h_full()
            .min_w_0()
            .min_h_0()
            .flex_1()
            .overflow_hidden()
            .on_action(cx.listener(|this, action: &LiveToolbarAction, _, cx| {
                let (width, height) = match action {
                    LiveToolbarAction::Resolution480 => (854, 480),
                    LiveToolbarAction::Resolution720 => (1280, 720),
                    LiveToolbarAction::Resolution1080 => (1920, 1080),
                };
                this.send_live_camera_command(CameraCommand {
                    command: camera_command::Command::SetResolution as i32,
                    resolution: Some(Resolution { width, height }),
                    mirrored: false,
                });
                cx.notify();
            }))
            .child(
                div()
                    .h_flex()
                    .w_full()
                    .min_w_0()
                    .flex_none()
                    .items_center()
                    .justify_between()
                    .gap_4()
                    .py_3()
                    .border_b_1()
                    .border_color(cx.theme().border)
                    .child(
                        div()
                            .h_flex()
                            .min_w_0()
                            .items_center()
                            .gap_2()
                            .child(
                                div()
                                    .min_w_0()
                                    .max_w(rems(12.))
                                    .truncate()
                                    .whitespace_nowrap()
                                    .text_sm()
                                    .font_weight(FontWeight::SEMIBOLD)
                                    .child(sender_name),
                            )
                            .child(live_metric_text(fps_label, cx))
                            .child(live_metric_text(latency_label, cx))
                            .child(live_metric_text(bitrate_label, cx))
                            .child(live_network_quality(
                                quality_label,
                                quality_color,
                                quality_ready,
                                cx,
                            )),
                    )
                    .child(
                        div()
                            .h_flex()
                            .flex_none()
                            .items_center()
                            .gap_2()
                            .child(details)
                            .child(resolution)
                            .child(
                                Button::new("remote-mirror-toolbar")
                                    .outline()
                                    .small()
                                    .selected(remote_mirrored)
                                    .toggled(remote_mirrored)
                                    .tooltip("镜像翻转")
                                    .accessibility_label("镜像翻转")
                                    .child(reicon_button_content(
                                        "镜像",
                                        "flip-horizontal",
                                        mirror_icon_color,
                                    ))
                                    .on_click(cx.listener(move |this, _, _, cx| {
                                        this.send_live_camera_command(CameraCommand {
                                            command: camera_command::Command::SetMirror as i32,
                                            resolution: None,
                                            mirrored: !remote_mirrored,
                                        });
                                        cx.notify();
                                    })),
                            )
                            .child(
                                Button::new("switch-camera-toolbar")
                                    .outline()
                                    .small()
                                    .tooltip("镜头切换")
                                    .accessibility_label("镜头切换")
                                    .child(reicon_button_content(
                                        "切换",
                                        "camera-rotate",
                                        cx.theme().primary,
                                    ))
                                    .on_click(cx.listener(|this, _, _, cx| {
                                        this.send_live_camera_command(CameraCommand {
                                            command: camera_command::Command::SwitchCamera as i32,
                                            resolution: None,
                                            mirrored: false,
                                        });
                                        cx.notify();
                                    })),
                            )
                            .child(
                                Button::new("request-idr-toolbar")
                                    .outline()
                                    .small()
                                    .tooltip("请求关键帧以修复卡顿或花屏")
                                    .accessibility_label("画面修复")
                                    .child(reicon_button_content(
                                        "修复",
                                        "refresh",
                                        cx.theme().primary,
                                    ))
                                    .on_click(cx.listener(|this, _, _, cx| {
                                        if let Err(err) = this.runtime.request_keyframe() {
                                            this.diagnostics_error =
                                                Some(format!("请求关键帧失败：{err}"));
                                        }
                                        cx.notify();
                                    })),
                            )
                            .child(
                                Button::new("disconnect-active-device")
                                    .custom(
                                        ButtonCustomVariant::new(cx)
                                            .foreground(cx.theme().danger)
                                            .hover(cx.theme().danger.opacity(0.08))
                                            .active(cx.theme().danger.opacity(0.16)),
                                    )
                                    .small()
                                    .tooltip("断开设备")
                                    .accessibility_label("断开")
                                    .child(reicon_button_content(
                                        "断开",
                                        "xmark",
                                        cx.theme().danger,
                                    ))
                                    .on_click(cx.listener(|this, _, _, cx| {
                                        this.disconnect_active_sender(cx);
                                    })),
                            ),
                    ),
            )
            .child(
                div()
                    .flex()
                    .flex_1()
                    .min_w_0()
                    .min_h_0()
                    // AC-D-LIVE-01: collapsing the Sidebar makes the preview
                    // height-constrained. Keep one spacing-token safety inset
                    // so the rounded frame never touches the toolbar divider.
                    .when(self.sidebar_collapsed, |this| this.p_4())
                    .items_center()
                    .justify_center()
                    .child(
                        div()
                            // AC-D-LIVE-01: the large fixed window keeps the complete
                            // command bar visible while the 16:9 preview consumes all
                            // remaining space without a secondary inspector.
                            .w_full()
                            .max_h_full()
                            .aspect_ratio(16. / 9.)
                            .flex_none()
                            .relative()
                            .on_prepaint(move |bounds, window, _| {
                                let scale = window.scale_factor();
                                preview_viewport.record_painted(
                                    bounds.size.width.as_f32() * scale,
                                    bounds.size.height.as_f32() * scale,
                                );
                            })
                            .rounded(cx.theme().radius_lg)
                            .bg(cx.theme().muted)
                            .overflow_hidden()
                            .child(self.video_surface.render_preview())
                            .child(rounded_preview_corners(cx))
                            .child(live_preview_badge(cx)),
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

#[cfg(test)]
mod tests {
    #[test]
    fn desktop_disconnect_is_bound_to_the_owner_command_without_a_dialog() {
        let source = include_str!("connect.rs");
        let button = source
            .lines()
            .skip_while(|line| !line.contains("Button::new(\"disconnect-active-device\")"))
            .take(32)
            .collect::<Vec<_>>()
            .join("\n");

        assert!(!button.is_empty(), "Live disconnect button");
        assert!(button.contains("this.disconnect_active_sender(cx)"));
        assert!(!button.contains("open_disconnect_dialog"));
    }
}
