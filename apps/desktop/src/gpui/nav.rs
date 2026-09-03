use std::time::Duration;

use gpui::prelude::FluentBuilder;
use gpui::*;
use gpui_component::animation::{ease_in_out_cubic, EffectTransition};
use gpui_component::button::*;
use gpui_component::scroll::ScrollableElement;
use gpui_component::*;

use crate::receiver_runtime::ReceiverSnapshot;

use super::icons::reicon_named;
use super::{DesktopSection, PicooDesktopApp};

const SIDEBAR_EXPANDED_WIDTH: Pixels = px(204.);
const SIDEBAR_COLLAPSED_WIDTH: Pixels = px(48.);
const SIDEBAR_TRANSITION_DURATION: Duration = Duration::from_millis(200);

#[derive(Clone, Copy, Debug, PartialEq)]
struct SidebarWidthTransition {
    from_width: Pixels,
    target_width: Pixels,
}

impl SidebarWidthTransition {
    fn new(target_width: Pixels) -> Self {
        Self {
            from_width: target_width,
            target_width,
        }
    }

    fn update_target(&mut self, target_width: Pixels) {
        if self.target_width != target_width {
            self.from_width = self.target_width;
            self.target_width = target_width;
        }
    }
}

fn sidebar_width_animation_id(from: Pixels, to: Pixels) -> ElementId {
    ElementId::NamedInteger(
        "picoo-sidebar-width".into(),
        (from.as_f32().to_bits() as u64) << 32 | to.as_f32().to_bits() as u64,
    )
}

impl PicooDesktopApp {
    pub(super) fn render_window_title_bar(&self, cx: &Context<Self>) -> impl IntoElement {
        TitleBar::new().border_b_0().bg(cx.theme().background)
    }

    pub(super) fn render_sidebar(
        &self,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> gpui::AnyElement {
        let collapsed = self.sidebar_collapsed;
        let target_width = if collapsed {
            SIDEBAR_COLLAPSED_WIDTH
        } else {
            SIDEBAR_EXPANDED_WIDTH
        };
        let navigation = div()
            .v_flex()
            .flex_1()
            .min_h_0()
            .justify_between()
            .when(collapsed, |this| this.px_2())
            .when(!collapsed, |this| this.px_3())
            .pt_2()
            .pb_3()
            .child(
                div()
                    .v_flex()
                    .gap_2()
                    .child(self.nav_button("连接", DesktopSection::Connect, "monitor-phone", cx))
                    .child(self.nav_button(
                        "虚拟摄像头",
                        DesktopSection::VirtualCamera,
                        "monitor-camera",
                        cx,
                    ))
                    .child(self.nav_button("网络", DesktopSection::Network, "wifi", cx))
                    .child(self.nav_button("通用", DesktopSection::General, "settings", cx)),
            )
            .child(
                div()
                    .v_flex()
                    .gap_2()
                    .pt_3()
                    .border_t_1()
                    .border_color(cx.theme().border)
                    .child(self.nav_button("帮助", DesktopSection::Help, "help-circle", cx))
                    .child(self.nav_button("关于", DesktopSection::About, "info", cx))
                    .child(self.theme_button(cx)),
            );
        let sidebar = div()
            .v_flex()
            .w(target_width)
            .h_full()
            .flex_shrink_0()
            .border_r_1()
            .border_color(cx.theme().border)
            .bg(cx.theme().sidebar)
            .child(navigation);

        let transition = self.sidebar_width_transition(
            "picoo-sidebar-width-transition",
            target_width,
            window,
            cx,
        );
        let wrapper = div()
            .id("picoo-sidebar-clip")
            .flex()
            .h_full()
            .flex_shrink_0()
            .overflow_hidden()
            .child(sidebar);

        EffectTransition::new(SIDEBAR_TRANSITION_DURATION)
            .ease(ease_in_out_cubic)
            .width(transition.from_width, transition.target_width)
            .apply(
                wrapper,
                sidebar_width_animation_id(transition.from_width, transition.target_width),
            )
            .into_any_element()
    }

    fn sidebar_width_transition(
        &self,
        key: &'static str,
        target_width: Pixels,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> SidebarWidthTransition {
        let transition =
            window.use_keyed_state(key, cx, |_, _| SidebarWidthTransition::new(target_width));
        if transition.read(cx).target_width != target_width {
            transition.update(cx, |transition, _| {
                transition.update_target(target_width);
            });
        }
        *transition.read(cx)
    }

    pub(super) fn sidebar_toggle_button(&self, cx: &Context<Self>) -> impl IntoElement {
        let collapsed = self.sidebar_collapsed;
        let label = if collapsed {
            "展开侧边栏"
        } else {
            "折叠侧边栏"
        };

        Button::new("toggle-sidebar")
            .ghost()
            .small()
            .h_8()
            .tooltip(label)
            .accessibility_label(label)
            .child(reicon_named(
                if collapsed {
                    "sidebar-right"
                } else {
                    "sidebar-left"
                },
                cx.theme().foreground,
            ))
            .on_click(cx.listener(|this, _, _, cx| {
                this.sidebar_collapsed = !this.sidebar_collapsed;
                cx.notify();
            }))
    }

    pub(super) fn theme_button(&self, cx: &Context<Self>) -> impl IntoElement {
        let is_dark = cx.theme().is_dark();
        Button::new("toggle-theme")
            .ghost()
            .w_full()
            .h_8()
            .when(self.sidebar_collapsed, |this| this.px_0())
            .when(!self.sidebar_collapsed, |this| this.px_2())
            .when(self.sidebar_collapsed, |this| {
                this.tooltip(if is_dark {
                    "浅色模式"
                } else {
                    "深色模式"
                })
            })
            .accessibility_label(if is_dark {
                "浅色模式"
            } else {
                "深色模式"
            })
            .child(
                div()
                    .h_flex()
                    .w_full()
                    .when(self.sidebar_collapsed, |this| this.justify_center())
                    .when(!self.sidebar_collapsed, |this| this.gap_2())
                    .child(
                        reicon_named(
                            if is_dark { "sun" } else { "moon" },
                            cx.theme().muted_foreground,
                        )
                        .size_4(),
                    )
                    .when(!self.sidebar_collapsed, |this| {
                        this.child(if is_dark {
                            "浅色模式"
                        } else {
                            "深色模式"
                        })
                    }),
            )
            .on_click(cx.listener(move |_, _, window, cx| {
                let mode = if is_dark {
                    ThemeMode::Light
                } else {
                    ThemeMode::Dark
                };
                Theme::change(mode, Some(window), cx);
                cx.notify();
            }))
    }

    pub(super) fn nav_button(
        &self,
        label: &'static str,
        section: DesktopSection,
        icon: &'static str,
        cx: &Context<Self>,
    ) -> impl IntoElement {
        let active = self.section == section;
        let icon_color = if active {
            cx.theme().primary
        } else {
            cx.theme().muted_foreground
        };
        Button::new(format!("nav-{icon}"))
            .ghost()
            .w_full()
            .h_8()
            .when(self.sidebar_collapsed, |this| this.px_0().tooltip(label))
            .when(!self.sidebar_collapsed, |this| this.px_2())
            .selected(active)
            .accessibility_label(label)
            .child(
                div()
                    .h_flex()
                    .w_full()
                    .when(self.sidebar_collapsed, |this| this.justify_center())
                    .when(!self.sidebar_collapsed, |this| this.gap_2())
                    .child(reicon_named(icon, icon_color).size_4())
                    .when(!self.sidebar_collapsed, |this| this.child(label)),
            )
            .on_click(cx.listener(move |this, _, _, cx| {
                this.section = section;
                cx.notify();
            }))
    }

    pub(super) fn render_section(
        &self,
        snapshot: &ReceiverSnapshot,
        cx: &Context<Self>,
    ) -> gpui::AnyElement {
        let page = match self.section {
            DesktopSection::Connect => self.render_connect(snapshot, cx).into_any_element(),
            DesktopSection::VirtualCamera => self
                .render_virtual_camera_page(snapshot, cx)
                .into_any_element(),
            DesktopSection::Network => self.render_network_page(snapshot, cx).into_any_element(),
            DesktopSection::General => self.render_general_page(snapshot, cx).into_any_element(),
            DesktopSection::Help => self.render_help_page(cx).into_any_element(),
            DesktopSection::About => self.render_about_page(cx).into_any_element(),
        };
        if self.section == DesktopSection::Connect {
            // REQ-PICOO-UI-0001 / AC-D-LAYOUT-01: the connection workspace is
            // stable at the supported minimum; its trusted-device list owns
            // ordinary overflow instead of nesting it under a page scrollbar.
            div()
                .size_full()
                .flex_1()
                .min_w_0()
                .min_h_0()
                .overflow_hidden()
                .child(
                    div()
                        .v_flex()
                        .size_full()
                        .min_w_0()
                        .min_h_0()
                        .gap_4()
                        .p_4()
                        .child(page),
                )
                .into_any_element()
        } else {
            div()
                .w_full()
                .h_full()
                .flex_1()
                .min_w_0()
                .min_h_0()
                .overflow_y_scrollbar()
                .child(
                    div()
                        .v_flex()
                        .w_full()
                        .min_h_full()
                        .gap_4()
                        .p_4()
                        .child(page),
                )
                .into_any_element()
        }
    }

    pub(super) fn render_workspace_toolbar(&self, cx: &Context<Self>) -> gpui::AnyElement {
        let content = div()
            .h_flex()
            .h_full()
            .w_full()
            .min_w_0()
            .items_end()
            .pl_3()
            .child(
                div()
                    .h_flex()
                    .h_8()
                    .items_center()
                    .gap_3()
                    .child(
                        div()
                            .on_mouse_down(MouseButton::Left, |_, _, cx| {
                                cx.stop_propagation();
                            })
                            .child(self.sidebar_toggle_button(cx)),
                    )
                    .child(div().h_4().border_l_1().border_color(cx.theme().border))
                    .child(
                        div()
                            .min_w_0()
                            .text_sm()
                            .font_weight(FontWeight::SEMIBOLD)
                            .text_color(cx.theme().muted_foreground)
                            .child(self.section.label()),
                    ),
            );

        if cfg!(target_os = "macos") {
            div()
                .h_flex()
                .h_10()
                .flex_shrink_0()
                .border_b_1()
                .border_color(cx.theme().border)
                .bg(cx.theme().background)
                .child(content)
                .into_any_element()
        } else {
            TitleBar::new()
                .h_10()
                .pl_0()
                .border_b_1()
                .border_color(cx.theme().border)
                .bg(cx.theme().background)
                .child(content)
                .into_any_element()
        }
    }
}

#[cfg(test)]
mod tests {
    use super::{SidebarWidthTransition, SIDEBAR_COLLAPSED_WIDTH, SIDEBAR_EXPANDED_WIDTH};

    #[test]
    fn sidebar_width_transition_tracks_the_previous_target() {
        let mut transition = SidebarWidthTransition::new(SIDEBAR_EXPANDED_WIDTH);

        transition.update_target(SIDEBAR_COLLAPSED_WIDTH);
        assert_eq!(transition.from_width, SIDEBAR_EXPANDED_WIDTH);
        assert_eq!(transition.target_width, SIDEBAR_COLLAPSED_WIDTH);

        transition.update_target(SIDEBAR_EXPANDED_WIDTH);
        assert_eq!(transition.from_width, SIDEBAR_COLLAPSED_WIDTH);
        assert_eq!(transition.target_width, SIDEBAR_EXPANDED_WIDTH);
    }
}
