use gpui_kit::component::button::*;
use gpui_kit::component::dialog::DialogButtonProps;
use gpui_kit::component::notification::NotificationType;
use gpui_kit::component::scroll::ScrollableElement;
use gpui_kit::component::switch::*;
use gpui_kit::component::*;
use gpui_kit::prelude::FluentBuilder;
use gpui_kit::*;

use crate::receiver_runtime::ReceiverSnapshot;

use super::icons::reicon_named;
use super::widgets::status_badge;
use super::{DesktopPage, PicooDesktopApp};

impl PicooDesktopApp {
    pub(super) fn render_device_connection_card(
        &self,
        snapshot: &ReceiverSnapshot,
        cx: &Context<Self>,
    ) -> impl IntoElement {
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
                            .border_color(cx.theme().border)
                            .bg(cx.theme().secondary)
                            .font_family(cx.theme().mono_font_family.clone())
                            .text_xs()
                            .text_color(cx.theme().muted_foreground)
                            .child(if snapshot.trusted_device_count == 0 {
                                "等待设备"
                            } else {
                                "已信任设备"
                            }),
                    ),
            )
            .child({
                let now_ms = current_unix_time_ms();
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
                                                    .child(
                                                        div().min_w_0().truncate().child(format!(
                                                            "身份 {identity_prefix}"
                                                        )),
                                                    ),
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
                                        Button::new(format!("remove-trusted-{}", device.device_id))
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
                    }))
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
                            .h_flex()
                            .items_center()
                            .gap_2()
                            .child(
                                Button::new("reset-all-pairings")
                                    .small()
                                    .danger()
                                    .label("重置全部配对…")
                                    .disabled(snapshot.trusted_device_count == 0)
                                    .on_click(cx.listener(|_, _, window, cx| {
                                        PicooDesktopApp::open_reset_trusted_dialog(
                                            cx.entity().downgrade(),
                                            window,
                                            cx,
                                        );
                                    })),
                            )
                            .child(
                                div()
                                    .text_xs()
                                    .font_family(cx.theme().mono_font_family.clone())
                                    .text_color(cx.theme().muted_foreground)
                                    .child(format!("{} 台已信任", snapshot.trusted_device_count)),
                            ),
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

    pub(super) fn open_reset_trusted_dialog(
        app: WeakEntity<Self>,
        window: &mut Window,
        cx: &mut App,
    ) {
        window.open_alert_dialog(cx, move |alert, _, _| {
            let app = app.clone();
            alert
                .title("重置全部配对？")
                .description(
                    "所有手机都会失去信任，当前连接会断开。再次连接时，必须在两端重新核对配对短码。",
                )
                .button_props(
                    DialogButtonProps::default()
                        .ok_text("重置配对")
                        .ok_variant(ButtonVariant::Danger)
                        .cancel_text("取消")
                        .show_cancel(true),
                )
                .on_ok(move |_, window, cx| {
                    let outcome = app.update(cx, |this, cx| {
                        match this.runtime.clear_trusted_devices() {
                            Ok(removed) => {
                                this.runtime.disconnect();
                                this.page = DesktopPage::Waiting;
                                this.diagnostics_error = None;
                                this.diagnostics_message = Some(format!(
                                    "已重置 {removed} 台设备的配对"
                                ));
                                cx.notify();
                                Ok(removed)
                            }
                            Err(error) => {
                                let message = format!("重置配对失败：{error}");
                                this.diagnostics_error = Some(message.clone());
                                cx.notify();
                                Err(message)
                            }
                        }
                    });
                    match outcome {
                        Ok(Ok(removed)) => {
                            window.push_notification(
                                (
                                    NotificationType::Success,
                                    format!("已重置 {removed} 台设备的配对"),
                                ),
                                cx,
                            );
                            true
                        }
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
