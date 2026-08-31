use std::path::PathBuf;

use gpui::prelude::FluentBuilder;
use gpui::*;
use gpui_component::button::*;
use gpui_component::input::Input;
use gpui_component::switch::*;
use gpui_component::*;
use picoo_discovery::DEFAULT_QUIC_PORT;

use crate::diagnostics_export::export_diagnostics_to_file_with_hosts;
use crate::prefs::LogLevel;
use crate::receiver_runtime::ReceiverSnapshot;

use super::connect::endpoint_label;
use super::icons::{reicon_named, reicon_svg};
use super::widgets::{
    network_detail_row, onboarding_step, page_header, section_header, settings_toggle_row,
    status_badge,
};
use super::PicooDesktopApp;

#[derive(Default)]
pub(super) struct DiagnosticsExportState {
    path: Option<PathBuf>,
}

impl DiagnosticsExportState {
    fn succeeded(&mut self, path: PathBuf) {
        self.path = Some(path);
    }

    fn failed(&mut self) {
        self.path = None;
    }

    fn can_reveal(&self) -> bool {
        self.path.is_some()
    }

    fn path(&self) -> Option<&std::path::Path> {
        self.path.as_deref()
    }
}

impl PicooDesktopApp {
    pub(super) fn export_diagnostics(&mut self, cx: &mut Context<Self>) {
        let snapshot = self.snapshot();
        let out_path = default_diagnostics_path();
        let mut hosts = Vec::new();
        if let Some(addr) = snapshot.bind_addr {
            hosts.push(addr.to_string());
        }
        match export_diagnostics_to_file_with_hosts(
            &out_path.to_string_lossy(),
            snapshot.status,
            snapshot.ingress,
            &hosts,
        ) {
            Ok(result) => {
                let exported_path = result
                    .path
                    .as_deref()
                    .map(PathBuf::from)
                    .unwrap_or(out_path);
                self.diagnostics_error = None;
                self.diagnostics_message = Some(format!(
                    "已导出至 {}（已脱敏，不含视频）",
                    exported_path.display()
                ));
                self.diagnostics_export.succeeded(exported_path);
            }
            Err(err) => {
                self.diagnostics_message = None;
                self.diagnostics_error = Some(format!("导出失败：{err}"));
                self.diagnostics_export.failed();
            }
        }
        cx.notify();
    }

    pub(super) fn render_network_page(
        &self,
        snapshot: &ReceiverSnapshot,
        cx: &Context<Self>,
    ) -> impl IntoElement {
        div()
            .v_flex()
            .max_w(rems(55.))
            .mx_auto()
            .gap_5()
            .child(page_header("wifi", "网络", "配置局域网发现与连接服务", cx))
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
            .child(section_header("tuning", "高级设置", cx))
            .child(
                div()
                    .v_flex()
                    .overflow_hidden()
                    .rounded(cx.theme().radius_lg)
                    .border_1()
                    .border_color(cx.theme().border)
                    .bg(cx.theme().group_box)
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
                    ))
                    .child(network_detail_row(
                        "activity",
                        "传输质量",
                        "RTT 延迟 · 丢包率 · 抖动",
                        format!(
                            "{:.0} ms · {:.1}% · {:.1} ms",
                            snapshot.stream_metrics.latency_ms,
                            snapshot.stream_metrics.packet_loss * 100.0,
                            snapshot.link_jitter_ms
                        ),
                        cx,
                    )),
            )
    }

    pub(super) fn render_general_page(
        &self,
        _snapshot: &ReceiverSnapshot,
        cx: &Context<Self>,
    ) -> impl IntoElement {
        div()
            .v_flex()
            .max_w(rems(55.))
            .mx_auto()
            .gap_6()
            .child(page_header("settings", "通用设置", "", cx))
            .child(self.render_settings(cx))
    }

    pub(super) fn render_help_page(&self, cx: &Context<Self>) -> impl IntoElement {
        div()
            .v_flex()
            .max_w(rems(55.))
            .mx_auto()
            .gap_5()
            .child(page_header(
                "help-circle",
                "帮助",
                "连接手机前可依次检查以下项目",
                cx,
            ))
            .child(
                div()
                    .v_flex()
                    .gap_4()
                    .p_5()
                    .rounded(cx.theme().radius_lg)
                    .border_1()
                    .border_color(cx.theme().border)
                    .bg(cx.theme().group_box)
                    .child(onboarding_step(
                        "1",
                        "wifi",
                        "确认手机与电脑连接到同一 Wi‑Fi",
                        cx,
                    ))
                    .child(onboarding_step(
                        "2",
                        "radio",
                        "确认路由器没有开启 AP 隔离",
                        cx,
                    ))
                    .child(onboarding_step(
                        "3",
                        "server",
                        "自动发现失败时，在手机端输入监听地址",
                        cx,
                    ))
                    .child(onboarding_step(
                        "4",
                        "shield-check",
                        "首次连接时核对两端显示的 6 位配对短码",
                        cx,
                    )),
            )
            .child(section_header("tuning", "诊断", cx))
            .child(
                div()
                    .v_flex()
                    .gap_4()
                    .p_5()
                    .rounded(cx.theme().radius_lg)
                    .border_1()
                    .border_color(cx.theme().border)
                    .bg(cx.theme().group_box)
                    .child(
                        div()
                            .v_flex()
                            .gap_2()
                            .child(
                                div()
                                    .text_sm()
                                    .font_weight(FontWeight::MEDIUM)
                                    .child("日志级别"),
                            )
                            .child(div().h_flex().gap_2().children(LogLevel::ALL.iter().map(
                                |level| {
                                    let selected = self.prefs.log_level == *level;
                                    let level = *level;
                                    Button::new(format!("log-{level:?}"))
                                        .outline()
                                        .small()
                                        .selected(selected)
                                        .label(level.label())
                                        .on_click(cx.listener(move |this, _, _, cx| {
                                            this.prefs.log_level = level;
                                            this.apply_log_level();
                                            let _ = this.persist_prefs();
                                            cx.notify();
                                        }))
                                        .into_any_element()
                                },
                            ))),
                    )
                    .child(
                        div()
                            .h_flex()
                            .items_center()
                            .justify_between()
                            .gap_4()
                            .pt_4()
                            .border_t_1()
                            .border_color(cx.theme().border)
                            .child(
                                div()
                                    .v_flex()
                                    .gap_1()
                                    .child(
                                        div()
                                            .text_sm()
                                            .font_weight(FontWeight::MEDIUM)
                                            .child("导出诊断信息"),
                                    )
                                    .child(
                                        div()
                                            .text_xs()
                                            .text_color(cx.theme().muted_foreground)
                                            .child("导出脱敏运行状态，不包含视频内容。"),
                                    ),
                            )
                            .child(
                                div()
                                    .h_flex()
                                    .gap_2()
                                    .child(
                                        Button::new("export-diagnostics")
                                            .outline()
                                            .label("导出")
                                            .on_click(cx.listener(|this, _, _, cx| {
                                                this.export_diagnostics(cx);
                                            })),
                                    )
                                    .when(self.diagnostics_export.can_reveal(), |actions| {
                                        actions.child(
                                            Button::new("reveal-diagnostics")
                                                .outline()
                                                .label("打开所在文件夹")
                                                .on_click(cx.listener(|this, _, _, cx| {
                                                    if let Some(path) =
                                                        this.diagnostics_export.path()
                                                    {
                                                        cx.reveal_path(path);
                                                    }
                                                })),
                                        )
                                    }),
                            ),
                    )
                    .children(
                        self.diagnostics_message
                            .as_ref()
                            .map(|message| vec![message.clone().into_any_element()])
                            .unwrap_or_default(),
                    )
                    .children(
                        self.diagnostics_error
                            .as_ref()
                            .map(|error| vec![format!("错误：{error}").into_any_element()])
                            .unwrap_or_default(),
                    ),
            )
    }

    pub(super) fn render_about_page(&self, cx: &Context<Self>) -> impl IntoElement {
        div()
            .v_flex()
            .max_w(rems(55.))
            .mx_auto()
            .items_center()
            .text_center()
            .gap_4()
            .py_12()
            .child(
                div()
                    .size_16()
                    .flex()
                    .items_center()
                    .justify_center()
                    .rounded(cx.theme().radius_lg)
                    .bg(cx.theme().primary)
                    .child(reicon_svg(
                        include_bytes!("../../../../assets/icons/reicon/camera.svg"),
                        cx.theme().primary_foreground,
                    )),
            )
            .child(
                div()
                    .text_2xl()
                    .font_weight(FontWeight::BOLD)
                    .child("Picoo Camera"),
            )
            .child(
                div()
                    .max_w_96()
                    .text_sm()
                    .text_color(cx.theme().muted_foreground)
                    .child("将手机摄像头通过局域网安全地连接到电脑。视频点对点传输，不经过云端。"),
            )
            .child(
                div()
                    .text_xs()
                    .font_family(cx.theme().mono_font_family.clone())
                    .text_color(cx.theme().muted_foreground)
                    .child("GPUI · Rustls · Quinn · Reicon"),
            )
    }

    pub(super) fn render_settings(&self, cx: &Context<Self>) -> impl IntoElement {
        let background_description = if cfg!(target_os = "macos") {
            "关闭窗口后继续在 Dock 与后台接收连接。"
        } else {
            "保持手机连接和虚拟摄像头可用。"
        };
        let startup_label = if cfg!(target_os = "macos") {
            "登录时启动"
        } else {
            "开机启动"
        };

        div()
            .v_flex()
            .gap_6()
            .child(
                div()
                    .v_flex()
                    .gap_2p5()
                    .child(section_header("desktop", "电脑名称", cx))
                    .child(
                        div()
                            .h_flex()
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
                                    .gap_1()
                                    .child(
                                        div()
                                            .text_sm()
                                            .font_weight(FontWeight::MEDIUM)
                                            .child("电脑名称"),
                                    )
                                    .child(
                                        div()
                                            .text_xs()
                                            .text_color(cx.theme().muted_foreground)
                                            .child("手机上看到的这台电脑名称。"),
                                    ),
                            )
                            .child(Input::new(&self.display_name_input).w(rems(20.))),
                    ),
            )
            .child(
                div()
                    .v_flex()
                    .gap_2p5()
                    .child(section_header("play-filled", "后台运行", cx))
                    .child(
                        div()
                            .v_flex()
                            .overflow_hidden()
                            .rounded(cx.theme().radius_lg)
                            .border_1()
                            .border_color(cx.theme().border)
                            .bg(cx.theme().group_box)
                            .child(settings_toggle_row(
                                "play-filled",
                                "关闭窗口后继续在后台运行",
                                background_description,
                                Switch::new("continue-in-background")
                                    .checked(self.tray_policy.minimize_to_tray)
                                    .disabled(cfg!(target_os = "macos"))
                                    .on_click(cx.listener(|this, checked, _, cx| {
                                        this.prefs.minimize_to_tray = *checked;
                                        this.tray_policy =
                                            crate::tray::TrayPolicy::for_current_platform(*checked);
                                        let _ = this.persist_prefs();
                                        cx.notify();
                                    })),
                                false,
                                cx,
                            ))
                            .child(settings_toggle_row(
                                "refresh",
                                startup_label,
                                "打开电脑并进入桌面后自动启动 Picoo Camera。",
                                Switch::new("launch-at-startup")
                                    .checked(self.prefs.launch_at_startup)
                                    .on_click(cx.listener(|this, checked, _, cx| {
                                        this.prefs.launch_at_startup = *checked;
                                        if let Err(err) =
                                            crate::startup::sync_launch_at_startup(*checked)
                                        {
                                            tracing::warn!("launch-at-startup sync failed: {err}");
                                        }
                                        let _ = this.persist_prefs();
                                        cx.notify();
                                    })),
                                true,
                                cx,
                            )),
                    ),
            )
            .child(
                div()
                    .h_flex()
                    .items_center()
                    .gap_2()
                    .pt_2()
                    .border_t_1()
                    .border_color(cx.theme().border)
                    .text_xs()
                    .text_color(cx.theme().muted_foreground)
                    .child(reicon_named("check-circle-filled", cx.theme().success))
                    .child("更改会自动保存"),
            )
    }
}

fn default_diagnostics_path() -> PathBuf {
    if cfg!(target_os = "windows") {
        std::env::var("TEMP")
            .map(|t| PathBuf::from(t).join("picoo-diagnostics.json"))
            .unwrap_or_else(|_| PathBuf::from("picoo-diagnostics.json"))
    } else {
        std::env::var("HOME")
            .map(|h| PathBuf::from(h).join("picoo-diagnostics.json"))
            .unwrap_or_else(|_| PathBuf::from("picoo-diagnostics.json"))
    }
}

#[cfg(test)]
mod tests {
    use super::DiagnosticsExportState;
    use std::path::PathBuf;

    #[test]
    fn diagnostics_reveal_action_tracks_last_successful_export() {
        let mut state = DiagnosticsExportState::default();
        assert!(!state.can_reveal());
        assert!(state.path().is_none());

        let path = PathBuf::from("diagnostics").join("picoo-diagnostics.json");
        state.succeeded(path.clone());
        assert!(state.can_reveal());
        assert_eq!(state.path(), Some(path.as_path()));

        state.failed();
        assert!(!state.can_reveal());
        assert!(state.path().is_none());
    }
}
