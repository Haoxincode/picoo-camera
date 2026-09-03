use gpui::*;
use gpui_component::button::*;
use gpui_component::dialog::DialogButtonProps;
use gpui_component::notification::NotificationType;
use gpui_component::scroll::ScrollableElement;
use gpui_component::*;
use picoo_receiver::{TrustedIdentityCandidate, TrustedIdentityReplacement};

use super::PicooDesktopApp;

impl PicooDesktopApp {
    pub(super) fn confirm_pairing_request(&mut self, cx: &mut Context<Self>) -> Result<(), String> {
        self.runtime
            .confirm_pairing()
            .map_err(|error| format!("配对确认失败：{error}"))?;
        self.pairing_locally_confirmed = true;
        cx.notify();
        Ok(())
    }

    pub(super) fn reject_pairing_request(&mut self, cx: &mut Context<Self>) -> Result<(), String> {
        self.runtime
            .reject_pairing()
            .map_err(|error| format!("拒绝配对失败：{error}"))?;
        self.pairing_locally_confirmed = false;
        cx.notify();
        Ok(())
    }

    pub(super) fn open_pairing_dialog(
        app: WeakEntity<Self>,
        code: String,
        sender_name: String,
        first_time: bool,
        ttl: u64,
        window: &mut Window,
        cx: &mut App,
    ) {
        let description = format!(
            "来自 {sender_name} 的{}连接请求。请确认手机上显示相同的 6 位数字。",
            if first_time { "首次" } else { "" }
        );
        let ttl_label = if ttl > 0 {
            format!("握手上下文派生短码 · {ttl}s 内有效")
        } else {
            "短码已过期 · 请让手机重新发起配对".into()
        };
        let confirm_app = app.clone();
        let cancel_app = app.clone();
        let close_app = app;

        window.open_alert_dialog(cx, move |alert, _, cx| {
            alert
                .title("核对配对短码")
                .description(description.clone())
                .child(pairing_code_panel(&code, ttl_label.clone(), cx))
                .button_props(
                    DialogButtonProps::default()
                        .ok_text("两端一致，确认配对")
                        .ok_variant(ButtonVariant::Primary)
                        .cancel_text("拒绝")
                        .show_cancel(true),
                )
                .on_ok({
                    let confirm_app = confirm_app.clone();
                    move |_, window, cx| {
                        let outcome =
                            confirm_app.update(cx, |this, cx| this.confirm_pairing_request(cx));
                        match outcome {
                            Ok(Ok(())) => {
                                window.push_notification(
                                    (
                                        NotificationType::Success,
                                        "电脑端已确认，正在等待手机完成配对",
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
                    }
                })
                .on_cancel({
                    let cancel_app = cancel_app.clone();
                    move |_, window, cx| {
                        let outcome =
                            cancel_app.update(cx, |this, cx| this.reject_pairing_request(cx));
                        match outcome {
                            Ok(Ok(())) => true,
                            Ok(Err(message)) => {
                                window.push_notification((NotificationType::Error, message), cx);
                                false
                            }
                            Err(_) => false,
                        }
                    }
                })
                .on_close({
                    let close_app = close_app.clone();
                    move |_, _, cx| {
                        let _ = close_app.update(cx, |this, cx| {
                            this.pairing_dialog.mark_closed();
                            cx.notify();
                        });
                    }
                })
        });
    }

    pub(super) fn open_identity_replacement_dialog(
        app: WeakEntity<Self>,
        replacement: TrustedIdentityReplacement,
        window: &mut Window,
        cx: &mut App,
    ) {
        let count = replacement.previous_identities.len();
        let revision = replacement.revision;
        let device_name = replacement.device_name.clone();
        let previous_identities = replacement.previous_identities.clone();
        let ok_app = app.clone();
        let cancel_app = app.clone();
        let close_app = app;

        window.open_alert_dialog(cx, move |alert, _, cx| {
            let ok_app = ok_app.clone();
            let cancel_app = cancel_app.clone();
            let close_app = close_app.clone();
            alert
                .title(format!("清理“{device_name}”的同名配对记录？"))
                .description(format!(
                    "当前身份已完成安全配对。以下 {count} 个记录是独立的加密身份；请核对指纹，仅当确认它们属于当前手机的历史安装时再清理。"
                ))
                .child(identity_candidate_list(&previous_identities, cx))
                .button_props(
                    DialogButtonProps::default()
                        .ok_text("仅保留当前身份")
                        .ok_variant(ButtonVariant::Danger)
                        .cancel_text("保留全部")
                        .show_cancel(true),
                )
                .on_ok({
                    move |_, window, cx| {
                        let result = ok_app.update(cx, |this, cx| {
                            let result = this.runtime.replace_trusted_identity_history(revision);
                            cx.notify();
                            result
                        });
                        match result {
                            Ok(Ok(removed)) => {
                                window.push_notification(
                                    (
                                        NotificationType::Success,
                                        format!("已撤销 {removed} 个同名旧配对"),
                                    ),
                                    cx,
                                );
                                true
                            }
                            Ok(Err(error)) => {
                                window.push_notification(
                                    (
                                        NotificationType::Error,
                                        format!("替换旧配对失败：{error}"),
                                    ),
                                    cx,
                                );
                                false
                            }
                            Err(_) => false,
                        }
                    }
                })
                .on_cancel({
                    move |_, _, cx| {
                        let _ = cancel_app.update(cx, |this, cx| {
                            this.runtime
                                .dismiss_trusted_identity_replacement(revision);
                            cx.notify();
                        });
                        true
                    }
                })
                .on_close({
                    move |_, _, cx| {
                        let _ = close_app.update(cx, |this, cx| {
                            if this.identity_replacement_dialog_revision == Some(revision) {
                                this.identity_replacement_dialog_revision = None;
                            }
                            cx.notify();
                        });
                    }
                })
        });
    }
}

fn identity_candidate_list(
    candidates: &[TrustedIdentityCandidate],
    cx: &mut App,
) -> impl IntoElement {
    div()
        .v_flex()
        .max_h(rems(10.))
        .gap_1()
        .overflow_y_scrollbar()
        .children(candidates.iter().map(|candidate| {
            let prefix = distinguishable_candidate_prefix(candidate, candidates);
            div()
                .h_flex()
                .justify_between()
                .gap_3()
                .px_2()
                .py_1()
                .rounded(cx.theme().radius)
                .bg(cx.theme().secondary.opacity(0.45))
                .child(
                    div()
                        .text_sm()
                        .font_weight(FontWeight::MEDIUM)
                        .child(format!("身份 {prefix}")),
                )
                .child(
                    div()
                        .text_xs()
                        .text_color(cx.theme().muted_foreground)
                        .child(format!(
                            "最近连接 {}",
                            crate::receiver_runtime::format_last_connected_ms(
                                candidate.last_connected_at_ms.unwrap_or(0),
                            )
                        )),
                )
        }))
}

fn distinguishable_candidate_prefix(
    candidate: &TrustedIdentityCandidate,
    candidates: &[TrustedIdentityCandidate],
) -> String {
    let fingerprint = candidate.certificate_fingerprint.as_str();
    let mut length = fingerprint.len().min(8);
    while length < fingerprint.len()
        && candidates.iter().any(|other| {
            other.device_id != candidate.device_id
                && other
                    .certificate_fingerprint
                    .get(..length)
                    .is_some_and(|prefix| fingerprint.get(..length) == Some(prefix))
        })
    {
        length = (length + 4).min(fingerprint.len());
    }
    fingerprint.get(..length).unwrap_or(fingerprint).to_string()
}

pub(super) fn format_pairing_code(code: &str) -> String {
    let digits: String = code
        .chars()
        .filter(|c| c.is_ascii_digit())
        .take(6)
        .collect();
    if digits.len() <= 3 {
        return digits;
    }
    format!("{} {}", &digits[..3], &digits[3..])
}

pub(super) fn connection_code_hero(code: &str, cx: &Context<PicooDesktopApp>) -> gpui::AnyElement {
    let digits = code
        .chars()
        .filter(|character| character.is_ascii_digit())
        .take(6)
        .collect::<Vec<_>>();
    if digits.len() != 6 {
        return div()
            .text_3xl()
            .font_family(cx.theme().mono_font_family.clone())
            .font_weight(FontWeight::BOLD)
            .text_color(cx.theme().foreground)
            .child(code.to_string())
            .into_any_element();
    }

    div()
        .h_flex()
        .gap_6()
        .children(digits.chunks(3).map(|group| {
            div().h_flex().gap_3().children(group.iter().map(|digit| {
                div()
                    .font_family(cx.theme().mono_font_family.clone())
                    .text_size(rems(3.875))
                    .line_height(relative(1.))
                    .font_weight(FontWeight::EXTRA_BOLD)
                    .text_color(cx.theme().foreground)
                    .child(digit.to_string())
            }))
        }))
        .into_any_element()
}

pub(super) fn pairing_code_panel(code: &str, ttl_label: String, cx: &App) -> gpui::AnyElement {
    let digits = code
        .chars()
        .filter(|character| character.is_ascii_digit())
        .take(6)
        .collect::<Vec<_>>();
    div()
        .v_flex()
        .items_center()
        .gap_2()
        .p_4()
        .rounded(cx.theme().radius)
        .border_1()
        .border_color(cx.theme().border)
        .bg(cx.theme().background)
        .child(
            div()
                .h_flex()
                .gap_2()
                .children(digits.into_iter().map(|digit| pairing_code_box(digit, cx))),
        )
        .child(
            div()
                .text_xs()
                .text_color(cx.theme().muted_foreground)
                .child(ttl_label),
        )
        .into_any_element()
}

pub(super) fn pairing_code_box(digit: char, cx: &App) -> gpui::AnyElement {
    div()
        .w_10()
        .h_12()
        .flex()
        .items_center()
        .justify_center()
        .rounded(cx.theme().radius)
        .border_1()
        .border_color(cx.theme().border)
        .bg(cx.theme().secondary)
        .text_xl()
        .font_family(cx.theme().mono_font_family.clone())
        .font_weight(FontWeight::BOLD)
        .text_color(cx.theme().foreground)
        .child(digit.to_string())
        .into_any_element()
}

#[cfg(test)]
mod tests {
    use super::distinguishable_candidate_prefix;
    use picoo_receiver::TrustedIdentityCandidate;

    fn candidate(device_id: &str, fingerprint: &str) -> TrustedIdentityCandidate {
        TrustedIdentityCandidate {
            device_id: device_id.into(),
            certificate_fingerprint: fingerprint.into(),
            last_connected_at_ms: None,
        }
    }

    #[test]
    fn cleanup_dialog_fingerprint_prefix_expands_past_collision() {
        let candidates = vec![
            candidate("a", "12345678aaaabbbb"),
            candidate("b", "12345678ccccdddd"),
        ];
        assert_eq!(
            distinguishable_candidate_prefix(&candidates[0], &candidates),
            "12345678aaaa"
        );
    }
}
