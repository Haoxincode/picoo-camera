use gpui::prelude::FluentBuilder;
use gpui::*;
use gpui_component::*;
use picoo_session::ReceiverStatus;

use super::icons::reicon_named;
use super::PicooDesktopApp;

pub(super) fn hardware_topology(cx: &Context<PicooDesktopApp>) -> impl IntoElement {
    let macbook_frame = if cx.theme().is_dark() {
        "device-frames/macbook-pro-dark.svg"
    } else {
        "device-frames/macbook-pro-light.svg"
    };
    let connection_dots = [0.4, 0.6, 1.0, 1.0, 0.6, 0.4]
        .into_iter()
        .map(|opacity| {
            div()
                .size_1p5()
                .flex_none()
                .rounded_full()
                .bg(cx.theme().primary.opacity(opacity))
                .into_any_element()
        })
        .collect::<Vec<_>>();

    div()
        .h_flex()
        .w_full()
        .min_h_20()
        .items_center()
        .justify_between()
        .pb_1()
        .child(
            div()
                .relative()
                .w(rems(2.5))
                .h(rems(5.08))
                .flex_none()
                // The HTML prototype wraps the real-device frame in `drop-shadow-md`.
                // Keep that separation from the light card so the sub-pixel metal rails
                // remain visible at the prototype's 40 px display width.
                .rounded(rems(0.38))
                .shadow_md()
                .child(
                    div()
                        .absolute()
                        .top(rems(0.067))
                        .right(rems(0.085))
                        .bottom(rems(0.067))
                        .left(rems(0.085))
                        .overflow_hidden()
                        .rounded(rems(0.36))
                        .bg(cx.theme().group_box),
                )
                .child(
                    img("device-frames/iphone-16-max.svg")
                        .w_full()
                        .h_full()
                        .object_fit(ObjectFit::Contain),
                )
                .child(
                    div()
                        .absolute()
                        .top(rems(0.))
                        .right(rems(0.))
                        .bottom(rems(0.))
                        .left(rems(0.))
                        .rounded(rems(0.38))
                        .border_1()
                        .border_color(cx.theme().foreground.opacity(0.24)),
                ),
        )
        .child(
            div()
                .h_flex()
                .items_center()
                .gap_1p5()
                .px_2()
                .children(connection_dots),
        )
        .child(
            div()
                .relative()
                .w(rems(7.))
                .h(rems(4.2))
                .flex_none()
                .child(
                    img(macbook_frame)
                        .w_full()
                        .h_full()
                        .object_fit(ObjectFit::Contain),
                )
                .child(
                    div()
                        // Mirrors the HTML prototype's MacBook screen slot.
                        .absolute()
                        .top(rems(0.31))
                        .right(rems(0.63))
                        .bottom(rems(0.49))
                        .left(rems(0.63))
                        .flex()
                        .items_center()
                        .justify_center()
                        .overflow_hidden()
                        .rounded(rems(0.06))
                        .bg(cx.theme().primary.opacity(if cx.theme().is_dark() {
                            0.16
                        } else {
                            0.08
                        }))
                        .child(
                            div()
                                .size_5()
                                .flex()
                                .items_center()
                                .justify_center()
                                .rounded_full()
                                .border_1()
                                .border_color(cx.theme().primary.opacity(0.4))
                                .bg(cx.theme().primary.opacity(0.16))
                                .child(reicon_named("camera", cx.theme().primary)),
                        ),
                ),
        )
}

pub(super) fn live_hud_pill(label: String, cx: &Context<PicooDesktopApp>) -> impl IntoElement {
    div()
        .px_3()
        .py_1p5()
        .rounded(cx.theme().radius)
        .bg(cx.theme().popover.opacity(0.88))
        .border_1()
        .border_color(cx.theme().border)
        .text_xs()
        .font_weight(FontWeight::SEMIBOLD)
        .text_color(cx.theme().popover_foreground)
        .child(label)
}

pub(super) fn page_header(
    icon: &'static str,
    title: &'static str,
    description: &'static str,
    cx: &Context<PicooDesktopApp>,
) -> impl IntoElement {
    div()
        .v_flex()
        .gap_1()
        .child(
            div()
                .h_flex()
                .items_center()
                .gap_2p5()
                .child(reicon_named(icon, cx.theme().primary).size(rems(1.5)))
                .child(div().text_xl().font_weight(FontWeight::BOLD).child(title)),
        )
        .when(!description.is_empty(), |this| {
            this.child(
                div()
                    .text_sm()
                    .text_color(cx.theme().muted_foreground)
                    .child(description),
            )
        })
}

pub(super) fn section_header(
    icon: &'static str,
    title: &'static str,
    cx: &Context<PicooDesktopApp>,
) -> impl IntoElement {
    div()
        .h_flex()
        .items_center()
        .gap_2()
        .text_sm()
        .font_weight(FontWeight::SEMIBOLD)
        .child(reicon_named(icon, cx.theme().primary))
        .child(title)
}

pub(super) fn placeholder_title(mode: crate::prefs::PlaceholderModePref) -> &'static str {
    mode.label()
}

pub(super) fn placeholder_preview(
    mode: crate::prefs::PlaceholderModePref,
    cx: &Context<PicooDesktopApp>,
) -> AnyElement {
    match mode {
        crate::prefs::PlaceholderModePref::Logo => div()
            .w_full()
            .h(rems(7.5))
            .v_flex()
            .items_center()
            .justify_center()
            .gap_1()
            .rounded(cx.theme().radius)
            .border_1()
            .border_color(cx.theme().border)
            .bg(cx.theme().secondary)
            .child(
                div()
                    .size_9()
                    .flex()
                    .items_center()
                    .justify_center()
                    .rounded_full()
                    .border_1()
                    .border_color(cx.theme().primary.opacity(0.4))
                    .bg(cx.theme().group_box)
                    .child(reicon_named("camera", cx.theme().primary)),
            )
            .child(
                div()
                    .text_xs()
                    .font_weight(FontWeight::SEMIBOLD)
                    .child("Picoo Camera"),
            )
            .into_any_element(),
        crate::prefs::PlaceholderModePref::Black => div()
            .w_full()
            .h(rems(7.5))
            .rounded(cx.theme().radius)
            .border_1()
            .border_color(cx.theme().border)
            .bg(rgb(0x000000))
            .into_any_element(),
        crate::prefs::PlaceholderModePref::Bars => div()
            .w_full()
            .h(rems(7.5))
            .v_flex()
            .overflow_hidden()
            .rounded(cx.theme().radius)
            .border_1()
            .border_color(cx.theme().border)
            .child(
                div().h_flex().flex_1().children(
                    [
                        0xC0C0C0, 0xC0C000, 0x00C0C0, 0x00C000, 0xC000C0, 0xC00000, 0x0000C0,
                    ]
                    .map(|color| div().h_full().flex_1().bg(rgb(color))),
                ),
            )
            .child(
                div().h(rems(1.8)).h_flex().children(
                    [
                        0x0000C0, 0x131313, 0xC000C0, 0x131313, 0x00C0C0, 0x131313, 0xC0C0C0,
                    ]
                    .map(|color| div().h_full().flex_1().bg(rgb(color))),
                ),
            )
            .into_any_element(),
    }
}

pub(super) fn placeholder_choice_indicator(
    selected: bool,
    cx: &Context<PicooDesktopApp>,
) -> AnyElement {
    if selected {
        reicon_named("check-circle-filled", cx.theme().primary)
            .size_4()
            .into_any_element()
    } else {
        div()
            .size_4()
            .rounded_full()
            .border_1()
            .border_color(cx.theme().border)
            .into_any_element()
    }
}

pub(super) fn settings_toggle_row(
    icon: &'static str,
    title: &'static str,
    description: &'static str,
    toggle: impl IntoElement,
    divided: bool,
    cx: &Context<PicooDesktopApp>,
) -> impl IntoElement {
    div()
        .h_flex()
        .items_center()
        .justify_between()
        .gap_5()
        .p_5()
        .when(divided, |this| {
            this.border_t_1().border_color(cx.theme().border)
        })
        .child(
            div()
                .v_flex()
                .gap_1()
                .child(
                    div()
                        .h_flex()
                        .items_center()
                        .gap_2()
                        .text_sm()
                        .font_weight(FontWeight::MEDIUM)
                        .child(reicon_named(icon, cx.theme().primary))
                        .child(title),
                )
                .child(
                    div()
                        .text_xs()
                        .text_color(cx.theme().muted_foreground)
                        .child(description),
                ),
        )
        .child(toggle)
}

pub(super) fn network_detail_row(
    icon: &'static str,
    title: &'static str,
    description: &'static str,
    value: String,
    cx: &Context<PicooDesktopApp>,
) -> impl IntoElement {
    div()
        .h_flex()
        .items_center()
        .justify_between()
        .gap_5()
        .p_5()
        .border_b_1()
        .border_color(cx.theme().border)
        .child(
            div()
                .h_flex()
                .items_center()
                .gap_3()
                .min_w_0()
                .child(reicon_named(icon, cx.theme().primary))
                .child(
                    div()
                        .v_flex()
                        .gap_1()
                        .child(div().text_sm().font_weight(FontWeight::MEDIUM).child(title))
                        .child(
                            div()
                                .text_xs()
                                .text_color(cx.theme().muted_foreground)
                                .child(description),
                        ),
                ),
        )
        .child(
            div()
                .flex_none()
                .font_family(cx.theme().mono_font_family.clone())
                .text_xs()
                .font_weight(FontWeight::MEDIUM)
                .child(value),
        )
}

pub(super) fn status_badge(
    label: impl Into<SharedString>,
    healthy: bool,
    cx: &Context<PicooDesktopApp>,
) -> impl IntoElement {
    let color = if healthy {
        cx.theme().success
    } else {
        cx.theme().warning
    };
    div()
        .h_flex()
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
        .child(label.into())
}

pub(super) fn onboarding_step(
    number: &'static str,
    icon: &'static str,
    label: &'static str,
    cx: &Context<PicooDesktopApp>,
) -> impl IntoElement {
    div()
        .h_flex()
        .gap_3()
        .items_center()
        .child(
            div()
                .size_5()
                .flex()
                .items_center()
                .justify_center()
                .rounded(cx.theme().radius_full())
                .bg(cx.theme().primary)
                .text_xs()
                .font_weight(FontWeight::BOLD)
                .text_color(cx.theme().primary_foreground)
                .child(number),
        )
        .child(reicon_named(icon, cx.theme().primary))
        .child(div().text_sm().child(label))
}

pub(super) fn connection_security_status(status: ReceiverStatus) -> (&'static str, bool) {
    match status {
        ReceiverStatus::Pairing => ("已加密 · 待确认", false),
        ReceiverStatus::Connecting | ReceiverStatus::Negotiating => ("正在建立", false),
        ReceiverStatus::Streaming
        | ReceiverStatus::Reconnecting
        | ReceiverStatus::NetworkUnstable => ("已验证", true),
        ReceiverStatus::Disconnected
        | ReceiverStatus::Discovering
        | ReceiverStatus::PermissionRequired
        | ReceiverStatus::VirtualCameraUnavailable => ("等待连接", false),
    }
}

pub(super) fn network_status_row(
    icon: &'static str,
    label: &'static str,
    value: impl Into<SharedString>,
    healthy: bool,
    cx: &Context<PicooDesktopApp>,
) -> impl IntoElement {
    let state_color = if healthy {
        cx.theme().success
    } else {
        cx.theme().warning
    };
    let state_icon = if healthy {
        "check-circle-filled"
    } else {
        "xmark"
    };

    div()
        .h_flex()
        .items_center()
        .justify_between()
        .gap_3()
        .text_xs()
        .child(
            div()
                .h_flex()
                .items_center()
                .gap_2p5()
                .text_color(cx.theme().muted_foreground)
                .child(reicon_named(icon, cx.theme().muted_foreground))
                .child(label),
        )
        .child(
            div()
                .h_flex()
                .items_center()
                .gap_1p5()
                .child(value.into())
                .child(reicon_named(state_icon, state_color)),
        )
}

pub(super) fn metric_row(
    label: &'static str,
    value: String,
    cx: &Context<PicooDesktopApp>,
) -> impl IntoElement {
    div()
        .h_flex()
        .justify_between()
        .gap_4()
        .pb_2()
        .border_b_1()
        .border_color(cx.theme().border)
        .child(
            div()
                .text_xs()
                .text_color(cx.theme().muted_foreground)
                .child(label),
        )
        .child(
            div()
                .text_xs()
                .font_family(cx.theme().mono_font_family.clone())
                .font_weight(FontWeight::MEDIUM)
                .text_color(cx.theme().foreground)
                .child(value),
        )
}

pub(super) fn status_row(
    label: &'static str,
    value: impl Into<SharedString>,
    healthy: bool,
    cx: &Context<PicooDesktopApp>,
) -> impl IntoElement {
    let color = if healthy {
        cx.theme().success
    } else {
        cx.theme().warning
    };
    div()
        .h_flex()
        .justify_between()
        .gap_3()
        .text_xs()
        .child(div().text_color(cx.theme().muted_foreground).child(label))
        .child(
            div()
                .h_flex()
                .gap_2()
                .child(value.into())
                .child(div().size_1p5().rounded(cx.theme().radius_full()).bg(color)),
        )
}

#[cfg(test)]
mod tests {
    use super::connection_security_status;
    use picoo_session::ReceiverStatus;

    #[test]
    fn security_copy_distinguishes_encryption_from_completed_pairing() {
        assert_eq!(
            connection_security_status(ReceiverStatus::Discovering),
            ("等待连接", false)
        );
        assert_eq!(
            connection_security_status(ReceiverStatus::Pairing),
            ("已加密 · 待确认", false)
        );
        assert_eq!(
            connection_security_status(ReceiverStatus::Streaming),
            ("已验证", true)
        );
    }
}
