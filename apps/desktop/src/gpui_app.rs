//! GPUI desktop shell — ARCH-PICOO-UI-001.
//!
//! Waiting / Live / Settings pages driven by [`ReceiverRuntime`] snapshots.

use std::time::Duration;

use gpui::*;
use gpui_component::{button::*, group_box::*, *};
use gpui_component_assets::Assets;
use picoo_receiver::ReceiverError;
use picoo_session::ReceiverStatus;

use crate::model::VirtualCameraStatus;
use crate::receiver_runtime::{
    ReceiverRuntime, ReceiverRuntimeConfig, ReceiverSnapshot, TrustedDeviceSummary,
};

#[derive(Clone, Copy, PartialEq, Eq)]
enum DesktopPage {
    Waiting,
    Live,
    Settings,
}

pub struct PicooDesktopApp {
    runtime: ReceiverRuntime,
    page: DesktopPage,
    show_qr: bool,
    pump_started: bool,
}

impl PicooDesktopApp {
    fn new(runtime: ReceiverRuntime) -> Self {
        Self {
            runtime,
            page: DesktopPage::Waiting,
            show_qr: true,
            pump_started: false,
        }
    }

    fn snapshot(&self) -> ReceiverSnapshot {
        self.runtime.snapshot()
    }

    fn ensure_pump_loop(&mut self, cx: &mut Context<Self>) {
        if self.pump_started {
            return;
        }
        self.pump_started = true;
        cx.spawn(async move |this, cx| loop {
            cx.background_executor()
                .timer(Duration::from_millis(16))
                .await;
            if this
                .update(cx, |this, cx| {
                    let _ = this.runtime.pump();
                    let snapshot = this.runtime.snapshot();
                    if matches!(snapshot.status, ReceiverStatus::Streaming) {
                        this.page = DesktopPage::Live;
                    } else if matches!(
                        snapshot.status,
                        ReceiverStatus::Disconnected | ReceiverStatus::Discovering
                    ) && this.page == DesktopPage::Live
                    {
                        this.page = DesktopPage::Waiting;
                    }
                    cx.notify();
                })
                .is_err()
            {
                break;
            }
        })
        .detach();
    }

    fn status_label(status: ReceiverStatus) -> &'static str {
        match status {
            ReceiverStatus::Discovering => "等待连接",
            ReceiverStatus::Pairing => "配对中",
            ReceiverStatus::Connecting => "连接中",
            ReceiverStatus::Negotiating => "协商中",
            ReceiverStatus::Streaming => "直播中",
            ReceiverStatus::Reconnecting => "重连中",
            ReceiverStatus::Disconnected => "未连接",
            ReceiverStatus::PermissionRequired => "需要权限",
            ReceiverStatus::VirtualCameraUnavailable => "虚拟摄像头不可用",
            ReceiverStatus::NetworkUnstable => "网络不稳定",
        }
    }
}

impl Render for PicooDesktopApp {
    fn render(&mut self, _: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        self.ensure_pump_loop(cx);
        let snapshot = self.snapshot();

        div()
            .v_flex()
            .size_full()
            .bg(cx.theme().background)
            .child(self.render_header(cx))
            .child(div().flex_1().child(match self.page {
                DesktopPage::Waiting => self.render_waiting(&snapshot, cx).into_any_element(),
                DesktopPage::Live => self.render_live(&snapshot, cx).into_any_element(),
                DesktopPage::Settings => self.render_settings(&snapshot, cx).into_any_element(),
            }))
    }
}

impl PicooDesktopApp {
    fn render_header(&self, cx: &Context<Self>) -> impl IntoElement {
        div()
            .h_flex()
            .w_full()
            .p_4()
            .gap_2()
            .items_center()
            .border_b_1()
            .border_color(cx.theme().border)
            .child(
                div()
                    .text_lg()
                    .font_weight(FontWeight::SEMIBOLD)
                    .child(format!("Picoo Camera — {}", self.snapshot().display_name)),
            )
            .child(div().flex_1())
            .child(self.nav_button("等待连接", DesktopPage::Waiting, cx))
            .child(self.nav_button("直播", DesktopPage::Live, cx))
            .child(self.nav_button("设置", DesktopPage::Settings, cx))
    }

    fn nav_button(
        &self,
        label: &'static str,
        page: DesktopPage,
        cx: &Context<Self>,
    ) -> impl IntoElement {
        let active = self.page == page;
        let mut button = Button::new(format!("nav-{label}")).label(label);
        if active {
            button = button.primary();
        }
        button.on_click(cx.listener(move |this, _, _, cx| {
            this.page = page;
            cx.notify();
        }))
    }

    fn render_waiting(&self, snapshot: &ReceiverSnapshot, cx: &Context<Self>) -> impl IntoElement {
        let bind = snapshot
            .bind_addr
            .map(|a| a.to_string())
            .unwrap_or_else(|| "—".into());
        let status = Self::status_label(snapshot.status);
        let pairing = snapshot
            .pairing_short_code
            .clone()
            .unwrap_or_else(|| "—".into());

        div()
            .v_flex()
            .gap_4()
            .p_6()
            .child(
                GroupBox::new()
                    .outline()
                    .title("等待手机连接")
                    .child(format!("状态：{status}"))
                    .child(format!("监听地址：{bind}"))
                    .child(format!("已配对设备：{}", snapshot.trusted_device_count))
                    .child(format!(
                        "虚拟摄像头：{}",
                        vcam_label(VirtualCameraStatus::Unknown)
                    )),
            )
            .child(
                GroupBox::new().outline().title("配对码").child(
                    div()
                        .v_flex()
                        .gap_2()
                        .child(
                            div()
                                .text_2xl()
                                .font_weight(FontWeight::BOLD)
                                .child(pairing),
                        )
                        .child(
                            Button::new("confirm-pairing")
                                .primary()
                                .label("确认配对")
                                .on_click(cx.listener(|this, _, _, cx| {
                                    this.runtime.confirm_pairing();
                                    cx.notify();
                                })),
                        ),
                ),
            )
            .children(if self.show_qr {
                vec![GroupBox::new()
                    .outline()
                    .title("二维码连接 (PUC-003)")
                    .child(
                        Button::new("toggle-qr")
                            .label("隐藏二维码")
                            .on_click(cx.listener(|this, _, _, cx| {
                                this.show_qr = false;
                                cx.notify();
                            })),
                    )
                    .child(
                        div().font_family("monospace").text_sm().child(
                            snapshot
                                .qr_ascii
                                .clone()
                                .unwrap_or_else(|| "QR 不可用".into()),
                        ),
                    )
                    .into_any_element()]
            } else {
                vec![Button::new("show-qr")
                    .label("显示二维码")
                    .on_click(cx.listener(|this, _, _, cx| {
                        this.show_qr = true;
                        cx.notify();
                    }))
                    .into_any_element()]
            })
    }

    fn render_live(&self, snapshot: &ReceiverSnapshot, cx: &Context<Self>) -> impl IntoElement {
        let status = Self::status_label(snapshot.status);
        let resolution = snapshot
            .stream_config
            .as_ref()
            .map(|c| format!("{}×{}", c.width, c.height))
            .unwrap_or_else(|| "—".into());
        let fps = snapshot
            .stream_config
            .as_ref()
            .map(|c| c.fps.to_string())
            .unwrap_or_else(|| "—".into());

        div().v_flex().gap_4().p_6().child(
            GroupBox::new().outline().title("直播预览").child(
                div()
                    .v_flex()
                    .gap_3()
                    .child(
                        div()
                            .w_full()
                            .h_64()
                            .bg(cx.theme().muted)
                            .flex()
                            .items_center()
                            .justify_center()
                            .rounded_md()
                            .child("VideoSurface 占位 — MF 解码接入中"),
                    )
                    .child(format!("状态：{status}"))
                    .child(format!("分辨率：{resolution} @ {fps} fps"))
                    .child(format!(
                        "接收 AU：{} / 包：{}",
                        snapshot.ingress.access_units, snapshot.ingress.packets_received
                    )),
            ),
        )
    }

    fn render_settings(&self, snapshot: &ReceiverSnapshot, cx: &Context<Self>) -> impl IntoElement {
        div()
            .v_flex()
            .gap_4()
            .p_6()
            .child(
                GroupBox::new()
                    .outline()
                    .title("已配对设备")
                    .child(format!("共 {} 台设备", snapshot.trusted_device_count))
                    .children(snapshot.trusted_devices.iter().map(|device| {
                        self.render_trusted_device_row(device, cx)
                            .into_any_element()
                    })),
            )
            .child(
                GroupBox::new()
                    .outline()
                    .title("诊断")
                    .child("CLI: picoo-desktop --export-diagnostics"),
            )
    }

    fn render_trusted_device_row(
        &self,
        device: &TrustedDeviceSummary,
        cx: &Context<Self>,
    ) -> impl IntoElement {
        div()
            .h_flex()
            .w_full()
            .gap_2()
            .items_center()
            .child(
                div()
                    .v_flex()
                    .child(format!("{} ({})", device.device_name, device.device_id))
                    .child(format!("fp={}", device.certificate_fingerprint)),
            )
            .child(div().flex_1())
            .child(
                Button::new(format!("remove-{}", device.device_id))
                    .label("删除")
                    .on_click({
                        let device_id = device.device_id.clone();
                        cx.listener(move |this, _, _, cx| {
                            let _ = this.runtime.remove_trusted_device(&device_id);
                            cx.notify();
                        })
                    }),
            )
    }
}

fn vcam_label(status: VirtualCameraStatus) -> &'static str {
    match status {
        VirtualCameraStatus::Unknown => "检测中",
        VirtualCameraStatus::Installed => "已安装",
        VirtualCameraStatus::NotInstalled => "未安装",
        VirtualCameraStatus::Active => "运行中",
    }
}

pub fn run_gpui_app() -> Result<(), ReceiverError> {
    let runtime = ReceiverRuntime::start(ReceiverRuntimeConfig::default())?;

    let app = gpui_platform::application().with_assets(Assets);
    app.run(move |cx| {
        gpui_component::init(cx);
        cx.set_app_identity("com.picoo.camera", "Picoo Camera");
        cx.activate(true);

        cx.open_window(
            WindowOptions {
                titlebar: Some(TitlebarOptions {
                    title: Some("Picoo Camera".into()),
                    ..Default::default()
                }),
                window_bounds: Some(WindowBounds::Windowed(Bounds {
                    origin: point(px(100.), px(100.)),
                    size: size(px(960.), px(720.)),
                })),
                ..Default::default()
            },
            |window, cx| {
                let view = cx.new(|_| PicooDesktopApp::new(runtime));
                cx.new(|cx| Root::new(view, window, cx).bg(cx.theme().background))
            },
        )
        .expect("open window");
    });

    Ok(())
}
