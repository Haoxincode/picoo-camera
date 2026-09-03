use gpui::prelude::FluentBuilder;
use gpui::*;
use gpui_component::button::*;
use gpui_component::dialog::DialogButtonProps;
use gpui_component::notification::NotificationType;
use gpui_component::scroll::ScrollableElement;
use gpui_component::switch::*;
use gpui_component::*;
use picoo_protocol::control::{camera_command, CameraCommand, Resolution};
use picoo_session::ReceiverStatus;

use crate::receiver_runtime::ReceiverSnapshot;

use super::icons::{reicon_button_content, reicon_named};
use super::vcam::vcam_label_zh;
use super::widgets::{metric_row, status_badge};
use super::{DesktopPage, PicooDesktopApp};

impl PicooDesktopApp {
    pub(super) fn render_device_connection_card(
        &self,
        snapshot: &ReceiverSnapshot,
        cx: &Context<Self>,
    ) -> impl IntoElement {
        let streaming = matches!(snapshot.status, ReceiverStatus::Streaming);
        let sender_name = snapshot
            .active_sender
            .as_ref()
            .map(|sender| sender.device_name.clone())
            .unwrap_or_else(|| "当前设备".into());

        div()
            .v_flex()
            .flex_1()
            .min_h_0()
            .gap_4()
            .p_5()
            .rounded(cx.theme().radius_lg)
            .border_1()
            .border_color(cx.theme().border)
            .bg(cx.theme().group_box)
            .when(cx.theme().shadow, |this| this.shadow_lg())
            .child(
                div()
                    .h_flex()
                    .justify_between()
                    .items_center()
                    .pb_3()
                    .border_b_1()
                    .border_color(cx.theme().border)
                    .child(
                        div()
                            .h_flex()
                            .items_center()
                            .gap_2()
                            .text_sm()
                            .font_weight(FontWeight::SEMIBOLD)
                            .child(reicon_named("iphone", cx.theme().primary))
                            .child("设备与连接"),
                    )
                    .child(
                        div()
                            .px_2()
                            .py_0p5()
                            .rounded(cx.theme().radius_full())
                            .border_1()
                            .border_color(if streaming {
                                cx.theme().success.opacity(0.4)
                            } else {
                                cx.theme().border
                            })
                            .bg(if streaming {
                                cx.theme().success.opacity(0.12)
                            } else {
                                cx.theme().secondary
                            })
                            .font_family(cx.theme().mono_font_family.clone())
                            .text_xs()
                            .text_color(if streaming {
                                cx.theme().success
                            } else {
                                cx.theme().muted_foreground
                            })
                            .child(if streaming {
                                "Live"
                            } else if snapshot.trusted_device_count == 0 {
                                "等待设备"
                            } else {
                                "已信任设备"
                            }),
                    ),
            )
            .when(streaming, |this| {
                let disconnect_sender_name = sender_name.clone();
                let bitrate = snapshot.stream_metrics.bitrate_bps as f64 / 1_000_000.0;
                let remote_mirrored = snapshot
                    .stream_config
                    .as_ref()
                    .map(|config| config.mirrored)
                    .unwrap_or(false);
                let current_resolution_height =
                    snapshot.stream_config.as_ref().map(|config| config.height);
                let mirror_icon_color = if remote_mirrored {
                    cx.theme().primary_foreground
                } else {
                    cx.theme().primary
                };
                let mirror_button = Button::new("remote-mirror-card")
                    .outline()
                    .small()
                    .flex_1()
                    .min_w_0()
                    .selected(remote_mirrored)
                    .tooltip("镜像翻转")
                    .accessibility_label("镜像翻转")
                    .child(reicon_button_content(
                        "镜像",
                        "flip-horizontal",
                        mirror_icon_color,
                    ));
                let mirror_button = mirror_button.on_click(cx.listener(move |this, _, _, cx| {
                    this.send_live_camera_command(CameraCommand {
                        command: camera_command::Command::SetMirror as i32,
                        resolution: None,
                        mirrored: !remote_mirrored,
                    });
                    cx.notify();
                }));
                this.child(
                    div()
                        .v_flex()
                        .flex_1()
                        .min_h_0()
                        .gap_3()
                        .overflow_y_scrollbar()
                        .child(
                            div()
                                .h_flex()
                                .min_w_0()
                                .justify_between()
                                .items_center()
                                .gap_2()
                                .p_3()
                                .rounded(cx.theme().radius)
                                .border_1()
                                .border_color(cx.theme().primary.opacity(0.45))
                                .bg(cx.theme().secondary)
                                .child(
                                    div().v_flex().min_w_0().flex_1().child(
                                        div()
                                            .min_w_0()
                                            .text_sm()
                                            .font_weight(FontWeight::SEMIBOLD)
                                            .truncate()
                                            .child(sender_name),
                                    ),
                                )
                                .child(
                                    Button::new("disconnect-active-device")
                                        .ghost()
                                        .flex_none()
                                        .tooltip("断开设备")
                                        .accessibility_label("断开")
                                        .child(
                                            reicon_named("xmark", cx.theme().danger)
                                                .size(rems(0.875)),
                                        )
                                        .on_click(cx.listener(move |_, _, window, cx| {
                                            PicooDesktopApp::open_disconnect_dialog(
                                                cx.entity().downgrade(),
                                                disconnect_sender_name.clone(),
                                                window,
                                                cx,
                                            );
                                        })),
                                ),
                        )
                        .child(
                            div()
                                .v_flex()
                                .gap_2()
                                .p_3()
                                .rounded(cx.theme().radius)
                                .border_1()
                                .border_color(cx.theme().border)
                                .bg(cx.theme().secondary.opacity(0.45))
                                .child(metric_row(
                                    "虚拟摄像头",
                                    vcam_label_zh(snapshot.virtual_camera).into(),
                                    cx,
                                ))
                                .child(metric_row(
                                    "视频规格",
                                    snapshot
                                        .stream_config
                                        .as_ref()
                                        .map(|config| {
                                            format!(
                                                "{}p · {} FPS 实测",
                                                config.height, snapshot.stream_metrics.fps
                                            )
                                        })
                                        .unwrap_or_else(|| "—".into()),
                                    cx,
                                ))
                                .child(metric_row("接收码率", format!("{bitrate:.1} Mbps"), cx))
                                .child(metric_row(
                                    "RTT / 网络抖动",
                                    format!(
                                        "{:.0} ms · {:.1} ms",
                                        snapshot.stream_metrics.latency_ms, snapshot.link_jitter_ms
                                    ),
                                    cx,
                                ))
                                .child(metric_row(
                                    "播放缓冲 实际 / 目标",
                                    format!(
                                        "{:.1} / {:.1} ms",
                                        snapshot.jitter_buffer_actual_delay_ms,
                                        snapshot.jitter_buffer_target_ms
                                    ),
                                    cx,
                                )),
                        )
                        .child(
                            div()
                                .v_flex()
                                .gap_2()
                                .child(
                                    div()
                                        .text_xs()
                                        .text_color(cx.theme().muted_foreground)
                                        .child("推流分辨率"),
                                )
                                .child(div().h_flex().gap_2().children(
                                    [(1280, 720, "720p"), (1920, 1080, "1080p")].map(
                                        |(width, height, label)| {
                                            Button::new(format!("resolution-{height}"))
                                                .outline()
                                                .small()
                                                .flex_1()
                                                .selected(current_resolution_height == Some(height))
                                                .label(label)
                                                .on_click(cx.listener(move |this, _, _, cx| {
                                                    this.send_live_camera_command(CameraCommand {
                                                        command:
                                                            camera_command::Command::SetResolution
                                                                as i32,
                                                        resolution: Some(Resolution {
                                                            width,
                                                            height,
                                                        }),
                                                        mirrored: false,
                                                    });
                                                    cx.notify();
                                                }))
                                                .into_any_element()
                                        },
                                    ),
                                ))
                                .child(
                                    div()
                                        .text_xs()
                                        .text_color(cx.theme().muted_foreground)
                                        .child("镜头与画面控制"),
                                )
                                .child(
                                    div()
                                        .h_flex()
                                        .w_full()
                                        .min_w_0()
                                        .gap_2()
                                        .child(mirror_button)
                                        .child(
                                            Button::new("switch-camera-card")
                                                .outline()
                                                .small()
                                                .flex_1()
                                                .min_w_0()
                                                .tooltip("镜头切换")
                                                .accessibility_label("镜头切换")
                                                .child(reicon_button_content(
                                                    "切换",
                                                    "camera-rotate",
                                                    cx.theme().primary,
                                                ))
                                                .on_click(cx.listener(|this, _, _, cx| {
                                                    this.send_live_camera_command(CameraCommand {
                                                        command: camera_command::Command::SwitchBack
                                                            as i32,
                                                        resolution: None,
                                                        mirrored: false,
                                                    });
                                                    cx.notify();
                                                })),
                                        )
                                        .child(
                                            Button::new("request-idr-card")
                                                .outline()
                                                .small()
                                                .flex_1()
                                                .min_w_0()
                                                .tooltip("画面修复")
                                                .accessibility_label("画面修复")
                                                .child(reicon_button_content(
                                                    "修复",
                                                    "refresh",
                                                    cx.theme().primary,
                                                ))
                                                .on_click(cx.listener(|this, _, _, cx| {
                                                    if let Err(err) =
                                                        this.runtime.request_keyframe()
                                                    {
                                                        this.diagnostics_error =
                                                            Some(format!("请求关键帧失败：{err}"));
                                                    }
                                                    cx.notify();
                                                })),
                                        ),
                                ),
                        ),
                )
            })
            .when(!streaming, |this| {
                let now_ms = current_unix_time_ms();
                this.child(
                    div()
                        .v_flex()
                        .flex_1()
                        .min_h_0()
                        .gap_2()
                        .overflow_y_scrollbar()
                        .when(snapshot.trusted_devices.is_empty(), |this| {
                            this.child(
                                div()
                                    .v_flex()
                                    .items_center()
                                    .gap_2()
                                    .p_6()
                                    .text_center()
                                    .text_color(cx.theme().muted_foreground)
                                    .child(reicon_named("camera", cx.theme().muted_foreground))
                                    .child(
                                        div()
                                            .text_sm()
                                            .font_weight(FontWeight::MEDIUM)
                                            .child("还没有已信任设备"),
                                    )
                                    .child(div().text_xs().child("手机完成首次连接后会显示在这里")),
                            )
                        })
                        .children(snapshot.trusted_devices.iter().map(|device| {
                            let device_id = device.device_id.clone();
                            let device_name = device.device_name.clone();
                            let identity_prefix = device.identity_prefix.clone();
                            let identity_label =
                                format!("删除 {}（身份 {}）", device.device_name, identity_prefix);
                            let last_connected =
                                crate::receiver_runtime::format_last_connected_relative_ms(
                                    device.last_connected_at_ms,
                                    now_ms,
                                );
                            div()
                                .h_flex()
                                .justify_between()
                                .items_center()
                                .gap_3()
                                .p_3()
                                .rounded(cx.theme().radius)
                                .border_1()
                                .border_color(cx.theme().border)
                                .bg(cx.theme().secondary.opacity(0.45))
                                .child(
                                    div()
                                        .h_flex()
                                        .items_center()
                                        .min_w_0()
                                        .flex_1()
                                        .gap_2()
                                        .child(
                                            img("device-frames/generic-phone.svg")
                                                .w_8()
                                                .h_16()
                                                .flex_none()
                                                .object_fit(ObjectFit::Contain),
                                        )
                                        .child(
                                            div()
                                                .v_flex()
                                                .min_w_0()
                                                .flex_1()
                                                .gap_1()
                                                .child(
                                                    div()
                                                        .text_sm()
                                                        .font_weight(FontWeight::SEMIBOLD)
                                                        .truncate()
                                                        .child(device.device_name.clone()),
                                                )
                                                .child(
                                                    div()
                                                        .h_flex()
                                                        .min_w_0()
                                                        .items_center()
                                                        .gap_1()
                                                        .text_xs()
                                                        .text_color(cx.theme().muted_foreground)
                                                        .whitespace_nowrap()
                                                        .child(
                                                            reicon_named(
                                                                "clock",
                                                                cx.theme().muted_foreground,
                                                            )
                                                            .size(rems(0.75))
                                                            .flex_none(),
                                                        )
                                                        .child(div().min_w_0().truncate().child(
                                                            format!("最近连接 {last_connected}"),
                                                        )),
                                                )
                                                .child(
                                                    div()
                                                        .h_flex()
                                                        .min_w_0()
                                                        .items_center()
                                                        .gap_1()
                                                        .text_xs()
                                                        .text_color(cx.theme().muted_foreground)
                                                        .whitespace_nowrap()
                                                        .child(
                                                            reicon_named(
                                                                "key",
                                                                cx.theme().muted_foreground,
                                                            )
                                                            .size(rems(0.75))
                                                            .flex_none(),
                                                        )
                                                        .child(div().min_w_0().truncate().child(
                                                            format!("身份 {identity_prefix}"),
                                                        )),
                                                ),
                                        ),
                                )
                                .child(
                                    div()
                                        .h_flex()
                                        .flex_none()
                                        .items_center()
                                        .gap_1()
                                        .child(status_badge("等待接入", false, cx))
                                        .child(
                                            Button::new(format!(
                                                "remove-trusted-{}",
                                                device.device_id
                                            ))
                                            .ghost()
                                            .tooltip(identity_label.clone())
                                            .accessibility_label(identity_label)
                                            .child(
                                                reicon_named("xmark", cx.theme().danger)
                                                    .size(rems(0.875)),
                                            )
                                            .on_click(cx.listener(move |_, _, window, cx| {
                                                PicooDesktopApp::open_remove_trusted_dialog(
                                                    cx.entity().downgrade(),
                                                    device_id.clone(),
                                                    device_name.clone(),
                                                    identity_prefix.clone(),
                                                    window,
                                                    cx,
                                                );
                                            })),
                                        ),
                                )
                                .into_any_element()
                        })),
                )
            })
            .child(
                div()
                    .h_flex()
                    .items_center()
                    .justify_between()
                    .gap_3()
                    .pt_3()
                    .border_t_1()
                    .border_color(cx.theme().border)
                    .child(
                        Switch::new("auto-accept-trusted-device")
                            .small()
                            .checked(self.prefs.auto_accept_paired)
                            .label("自动接受可信设备")
                            .on_click(cx.listener(|this, checked, _, cx| {
                                this.prefs.auto_accept_paired = *checked;
                                this.runtime.set_auto_accept_paired(*checked);
                                let _ = this.persist_prefs();
                                cx.notify();
                            })),
                    )
                    .child(
                        div()
                            .text_xs()
                            .font_family(cx.theme().mono_font_family.clone())
                            .text_color(cx.theme().muted_foreground)
                            .child(format!("{} 台已信任", snapshot.trusted_device_count)),
                    ),
            )
    }

    pub(super) fn open_disconnect_dialog(
        app: WeakEntity<Self>,
        sender_name: String,
        window: &mut Window,
        cx: &mut App,
    ) {
        window.open_alert_dialog(cx, move |alert, _, _| {
            let app = app.clone();
            alert
                .title(format!("断开“{sender_name}”？"))
                .description("视频推流和虚拟摄像头画面会立即停止，设备之后仍可重新连接。")
                .button_props(
                    DialogButtonProps::default()
                        .ok_text("断开")
                        .ok_variant(ButtonVariant::Danger)
                        .cancel_text("取消")
                        .show_cancel(true),
                )
                .on_ok(move |_, _, cx| {
                    let _ = app.update(cx, |this, cx| {
                        this.runtime.disconnect();
                        this.page = DesktopPage::Waiting;
                        cx.notify();
                    });
                    true
                })
        });
    }

    pub(super) fn open_remove_trusted_dialog(
        app: WeakEntity<Self>,
        device_id: String,
        device_name: String,
        identity_prefix: String,
        window: &mut Window,
        cx: &mut App,
    ) {
        window.open_alert_dialog(cx, move |alert, _, _| {
            let app = app.clone();
            let device_id = device_id.clone();
            alert
                .title(format!("删除“{device_name}”（身份 {identity_prefix}）？"))
                .description("此设备下次连接时必须重新核对配对短码。")
                .button_props(
                    DialogButtonProps::default()
                        .ok_text("删除")
                        .ok_variant(ButtonVariant::Danger)
                        .cancel_text("取消")
                        .show_cancel(true),
                )
                .on_ok(move |_, window, cx| {
                    let outcome = app.update(cx, |this, cx| {
                        let result = match this.runtime.remove_trusted_device(&device_id) {
                            Ok(true) => {
                                this.diagnostics_error = None;
                                this.diagnostics_message = Some(format!("已删除配对：{device_id}"));
                                Ok(())
                            }
                            Ok(false) => {
                                let message = format!("未找到配对设备：{device_id}");
                                this.diagnostics_error = Some(message.clone());
                                Err(message)
                            }
                            Err(err) => {
                                let message = format!("删除配对失败：{err}");
                                this.diagnostics_error = Some(message.clone());
                                Err(message)
                            }
                        };
                        cx.notify();
                        result
                    });
                    match outcome {
                        Ok(Ok(())) => true,
                        Ok(Err(message)) => {
                            window.push_notification((NotificationType::Error, message), cx);
                            false
                        }
                        Err(_) => false,
                    }
                })
        });
    }
}

fn current_unix_time_ms() -> u64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|duration| duration.as_millis().min(u128::from(u64::MAX)) as u64)
        .unwrap_or(0)
}
