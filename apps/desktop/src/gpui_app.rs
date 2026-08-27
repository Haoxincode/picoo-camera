//! GPUI desktop shell — ARCH-PICOO-UI-001.
//!
//! First launch / Waiting / Live / Settings pages driven by [`ReceiverRuntime`] snapshots.

use std::path::PathBuf;
use std::sync::Arc;
use std::time::Duration;

use gpui::prelude::FluentBuilder;
use gpui::*;
use gpui_component::button::*;
use gpui_component::group_box::*;
use gpui_component::input::{Input, InputState};
use gpui_component::switch::*;
use gpui_component::*;
use gpui_component_assets::Assets;
use image::{Frame, ImageBuffer, Rgba};
use picoo_protocol::control::{camera_command, CameraCommand};
use picoo_receiver::ReceiverError;
use picoo_session::ReceiverStatus;
use smallvec::smallvec;

use crate::diagnostics_export::export_diagnostics_to_file_with_hosts;
use crate::model::VirtualCameraStatus;
use crate::prefs::{load_prefs, save_prefs, DesktopPreferences, LogLevel};
use crate::qr_display;
use crate::receiver_runtime::{ReceiverRuntime, ReceiverSnapshot, TrustedDeviceSummary};
use crate::vcam_status::{detect_vcam_status, vcam_repair_hint};
use crate::video_surface::VideoSurface;

#[derive(Clone, Copy, PartialEq, Eq)]
enum DesktopPage {
    FirstLaunch,
    Waiting,
    Live,
    Settings,
}

pub struct PicooDesktopApp {
    runtime: ReceiverRuntime,
    prefs: DesktopPreferences,
    tray_policy: crate::tray::TrayPolicy,
    page: DesktopPage,
    show_qr: bool,
    pump_started: bool,
    video_surface: VideoSurface,
    display_name_input: Entity<InputState>,
    vcam_status: VirtualCameraStatus,
    diagnostics_message: Option<String>,
    diagnostics_error: Option<String>,
    /// Cached QR bitmap for waiting page (PUC-003).
    qr_image: Option<Arc<RenderImage>>,
    qr_payload_key: Option<String>,
    /// Holds Session-lifetime MF virtual camera while the UI is open.
    #[cfg(all(windows, feature = "windows-vcam"))]
    vcam_registration: Option<crate::vcam_register::VirtualCameraRegistration>,
}

impl PicooDesktopApp {
    fn new(
        runtime: ReceiverRuntime,
        prefs: DesktopPreferences,
        display_name_input: Entity<InputState>,
        vcam_status: VirtualCameraStatus,
    ) -> Self {
        let page = if prefs.first_launch_completed {
            DesktopPage::Waiting
        } else {
            DesktopPage::FirstLaunch
        };
        Self {
            runtime,
            prefs: prefs.clone(),
            tray_policy: crate::tray::TrayPolicy::from_pref(prefs.minimize_to_tray),
            page,
            show_qr: true,
            pump_started: false,
            video_surface: VideoSurface::default(),
            display_name_input,
            vcam_status,
            diagnostics_message: None,
            diagnostics_error: None,
            qr_image: None,
            qr_payload_key: None,
            #[cfg(all(windows, feature = "windows-vcam"))]
            vcam_registration: None,
        }
    }

    fn snapshot(&self) -> ReceiverSnapshot {
        self.runtime.snapshot()
    }

    /// Close-button policy from settings (REQ-PICOO-UI-008).
    fn close_outcome(&self) -> crate::tray::CloseOutcome {
        self.tray_policy.close_outcome()
    }

    fn ensure_qr_image(&mut self, snapshot: &ReceiverSnapshot) {
        let Some(json) = snapshot.qr_json.as_ref() else {
            self.qr_image = None;
            self.qr_payload_key = None;
            return;
        };
        if self.qr_payload_key.as_ref() == Some(json) && self.qr_image.is_some() {
            return;
        }
        match qr_display::render_qr_rgba(json, 6) {
            Ok((width, height, rgba)) => {
                if let Some(buffer) =
                    ImageBuffer::<Rgba<u8>, Vec<u8>>::from_raw(width, height, rgba)
                {
                    let frame = Frame::new(buffer);
                    self.qr_image = Some(Arc::new(RenderImage::new(smallvec![frame])));
                    self.qr_payload_key = Some(json.clone());
                }
            }
            Err(_) => {
                self.qr_image = None;
                self.qr_payload_key = None;
            }
        }
    }

    fn persist_prefs(&mut self) -> Result<(), String> {
        save_prefs(&self.prefs)
    }

    fn apply_log_level(&self) {
        let _ = std::env::set_var("RUST_LOG", self.prefs.log_level.env_filter());
    }

    fn complete_first_launch(&mut self, cx: &mut Context<Self>) {
        self.prefs.first_launch_completed = true;
        let _ = self.persist_prefs();
        self.page = DesktopPage::Waiting;
        cx.notify();
    }

    fn refresh_vcam_status(&mut self) {
        let status = detect_vcam_status();
        self.vcam_status = status;
        self.runtime.set_virtual_camera_status(status);
    }

    fn try_register_vcam(&mut self) {
        #[cfg(all(windows, feature = "windows-vcam"))]
        {
            match crate::vcam_register::VirtualCameraRegistration::register_and_start() {
                Ok(reg) => {
                    self.vcam_registration = Some(reg);
                    self.vcam_status = VirtualCameraStatus::Active;
                    self.runtime
                        .set_virtual_camera_status(VirtualCameraStatus::Active);
                }
                Err(err) => {
                    tracing::warn!("Install Virtual Camera failed: {err}");
                    self.refresh_vcam_status();
                }
            }
        }
        #[cfg(not(all(windows, feature = "windows-vcam")))]
        {
            self.refresh_vcam_status();
        }
    }

    fn save_display_name(&mut self, cx: &mut Context<Self>) {
        let name = self.display_name_input.read(cx).value().trim().to_string();
        let name = if name.is_empty() {
            "Picoo Camera".into()
        } else {
            name
        };
        self.prefs.display_name = name.clone();
        self.runtime.set_display_name(name);
        let _ = self.persist_prefs();
        cx.notify();
    }

    fn export_diagnostics(&mut self, cx: &mut Context<Self>) {
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
                self.diagnostics_error = None;
                self.diagnostics_message = result.path;
            }
            Err(err) => {
                self.diagnostics_message = None;
                self.diagnostics_error = Some(err);
            }
        }
        cx.notify();
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
                    if let Some(slot) = this.runtime.receiver().latest_frame() {
                        this.video_surface.update_from_slot(slot);
                    }
                    let snapshot = this.runtime.snapshot();
                    // REQ-PICOO-UI-008: live tip while hidden to tray.
                    crate::tray::sync_tray_tip(snapshot.status);
                    if let Some(action) = crate::tray::take_pending_menu_action() {
                        let outcome = action.apply();
                        if outcome.quit {
                            crate::tray::note_tray_cleared();
                            cx.quit();
                        } else if outcome.restore_window {
                            cx.activate(true);
                        }
                    }
                    if matches!(snapshot.status, ReceiverStatus::Streaming) {
                        if this.page != DesktopPage::Settings
                            && this.page != DesktopPage::FirstLaunch
                        {
                            this.page = DesktopPage::Live;
                        }
                    } else if matches!(
                        snapshot.status,
                        ReceiverStatus::Disconnected | ReceiverStatus::Discovering
                    ) && this.page == DesktopPage::Live
                    {
                        this.page = DesktopPage::Waiting;
                    }
                    if matches!(snapshot.status, ReceiverStatus::Streaming) {
                        this.vcam_status = VirtualCameraStatus::Active;
                        this.runtime
                            .set_virtual_camera_status(VirtualCameraStatus::Active);
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
}

impl Render for PicooDesktopApp {
    fn render(&mut self, _: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        self.ensure_pump_loop(cx);
        let snapshot = self.snapshot();
        if self.page == DesktopPage::Waiting && self.show_qr {
            self.ensure_qr_image(&snapshot);
        }

        div()
            .v_flex()
            .size_full()
            .bg(cx.theme().background)
            .child(self.render_header(cx))
            .child(div().flex_1().child(match self.page {
                DesktopPage::FirstLaunch => self.render_first_launch(cx).into_any_element(),
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
            .when(self.prefs.first_launch_completed, |this| {
                this.child(self.nav_button("等待连接", DesktopPage::Waiting, cx))
                    .child(self.nav_button("直播", DesktopPage::Live, cx))
                    .child(self.nav_button("设置", DesktopPage::Settings, cx))
            })
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

    fn render_first_launch(&self, cx: &Context<Self>) -> impl IntoElement {
        div()
            .v_flex()
            .gap_4()
            .p_6()
            .child(
                GroupBox::new()
                    .outline()
                    .title("Picoo Camera")
                    .child("Use your phone as a wireless camera")
                    .child(format!(
                        "Virtual Camera [ {} ]",
                        vcam_label(self.vcam_status)
                    ))
                    .child(vcam_repair_hint(self.vcam_status)),
            )
            .child(
                Button::new("refresh-vcam")
                    .label("重新检测虚拟摄像头")
                    .on_click(cx.listener(|this, _, _, cx| {
                        this.refresh_vcam_status();
                        cx.notify();
                    })),
            )
            .child(
                Button::new("install-vcam")
                    .label("Install Virtual Camera")
                    .on_click(cx.listener(|this, _, _, cx| {
                        this.try_register_vcam();
                        cx.notify();
                    })),
            )
            .child(
                Button::new("continue-first-launch")
                    .primary()
                    .label("继续")
                    .on_click(cx.listener(|this, _, _, cx| {
                        this.complete_first_launch(cx);
                    })),
            )
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
        let vcam = vcam_label(snapshot.virtual_camera);

        div()
            .v_flex()
            .gap_4()
            .p_6()
            .child(
                GroupBox::new()
                    .outline()
                    .title("等待手机连接")
                    .child("Open Picoo Camera on your phone and connect to this computer.")
                    .child(format!("状态：{status}"))
                    .child(format!("监听地址：{bind}"))
                    .child(format!("已配对设备：{}", snapshot.trusted_device_count))
                    .child(format!("Virtual Camera: {vcam}")),
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
                        )
                        .child(
                            Button::new("cancel-pairing")
                                .label("取消")
                                .on_click(cx.listener(|this, _, _, cx| {
                                    // PUC-001 / PRD §17.2: Cancel aborts pending pairing.
                                    this.runtime.disconnect();
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
                        div()
                            .w_64()
                            .h_64()
                            .bg(cx.theme().background)
                            .rounded_md()
                            .overflow_hidden()
                            .flex()
                            .items_center()
                            .justify_center()
                            .child(if let Some(image) = &self.qr_image {
                                img(ImageSource::Render(image.clone()))
                                    .w_full()
                                    .h_full()
                                    .object_fit(ObjectFit::Contain)
                                    .into_any_element()
                            } else {
                                div()
                                    .font_family("monospace")
                                    .text_xs()
                                    .child(
                                        snapshot
                                            .qr_ascii
                                            .clone()
                                            .unwrap_or_else(|| "QR 不可用".into()),
                                    )
                                    .into_any_element()
                            }),
                    )
                    .into_any_element()]
            } else {
                vec![Button::new("show-qr")
                    .label("Show QR Code")
                    .on_click(cx.listener(|this, _, _, cx| {
                        this.show_qr = true;
                        cx.notify();
                    }))
                    .into_any_element()]
            })
    }

    fn send_live_camera_command(&mut self, command: CameraCommand) {
        if let Err(err) = self.runtime.send_camera_command(command) {
            tracing::warn!("CameraCommand failed: {err}");
        }
    }

    fn render_live(&self, snapshot: &ReceiverSnapshot, cx: &Context<Self>) -> impl IntoElement {
        let status = Self::status_label(snapshot.status);
        let streaming = matches!(snapshot.status, ReceiverStatus::Streaming);
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
        let sender_name = snapshot
            .active_sender
            .as_ref()
            .map(|s| s.device_name.clone())
            .unwrap_or_else(|| "—".into());
        let remote_mirrored = snapshot
            .stream_config
            .as_ref()
            .map(|c| c.mirrored)
            .unwrap_or(false);

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
                            .rounded_md()
                            .overflow_hidden()
                            .child(self.video_surface.render_preview()),
                    )
                    .child(format!("{sender_name} · {status}"))
                    .child(format!("{resolution} · {fps} FPS"))
                    .child(format!(
                        "码率 {:.1} Mbps · 延迟 {:.0} ms · 丢包 {:.1}%",
                        snapshot.stream_metrics.bitrate_bps as f64 / 1_000_000.0,
                        snapshot.stream_metrics.latency_ms,
                        snapshot.stream_metrics.packet_loss * 100.0
                    ))
                    .child(format!(
                        "接收 AU：{} / 包：{}",
                        snapshot.ingress.access_units, snapshot.ingress.packets_received
                    ))
                    .child(format!(
                        "Virtual Camera: {}",
                        vcam_label(snapshot.virtual_camera)
                    ))
                    .when(streaming, |el| {
                        el.child(
                            div()
                                .h_flex()
                                .gap_2()
                                .child(
                                    Button::new("cam-front")
                                        .label("前置摄像头")
                                        .on_click(cx.listener(|this, _, _, cx| {
                                            this.send_live_camera_command(CameraCommand {
                                                command: camera_command::Command::SwitchFront
                                                    as i32,
                                                resolution: None,
                                                mirrored: false,
                                            });
                                            cx.notify();
                                        })),
                                )
                                .child(
                                    Button::new("cam-back")
                                        .label("后置摄像头")
                                        .on_click(cx.listener(|this, _, _, cx| {
                                            this.send_live_camera_command(CameraCommand {
                                                command: camera_command::Command::SwitchBack
                                                    as i32,
                                                resolution: None,
                                                mirrored: false,
                                            });
                                            cx.notify();
                                        })),
                                ),
                        )
                        .child(
                            Switch::new("remote-mirror")
                                .checked(remote_mirrored)
                                .label("远端镜像")
                                .on_click(cx.listener(|this, checked, _, cx| {
                                    this.send_live_camera_command(CameraCommand {
                                        command: camera_command::Command::SetMirror as i32,
                                        resolution: None,
                                        mirrored: *checked,
                                    });
                                    cx.notify();
                                })),
                        )
                    })
                    .child(
                        Button::new("disconnect")
                            .label("断开连接")
                            .on_click(cx.listener(|this, _, _, cx| {
                                this.runtime.disconnect();
                                this.page = DesktopPage::Waiting;
                                cx.notify();
                            })),
                    ),
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
                    .title("常规")
                    .child(
                        div()
                            .v_flex()
                            .gap_2()
                            .child("桌面显示名称")
                            .child(Input::new(&self.display_name_input))
                            .child(
                                Button::new("save-display-name")
                                    .label("保存显示名称")
                                    .on_click(cx.listener(|this, _, _, cx| {
                                        this.save_display_name(cx);
                                    })),
                            ),
                    )
                    .child(
                        Switch::new("auto-accept-paired")
                            .checked(self.prefs.auto_accept_paired)
                            .label("自动接受已配对设备")
                            .on_click(cx.listener(|this, checked, _, cx| {
                                this.prefs.auto_accept_paired = *checked;
                                this.runtime.set_auto_accept_paired(*checked);
                                let _ = this.persist_prefs();
                                cx.notify();
                            })),
                    )
                    .child(
                        Switch::new("launch-at-startup")
                            .checked(self.prefs.launch_at_startup)
                            .label("开机启动")
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
                    )
                    .child(
                        Switch::new("minimize-to-tray")
                            .checked(self.prefs.minimize_to_tray)
                            .label("最小化到托盘")
                            .on_click(cx.listener(|this, checked, _, cx| {
                                this.prefs.minimize_to_tray = *checked;
                                this.tray_policy = crate::tray::TrayPolicy::from_pref(*checked);
                                let _ = this.persist_prefs();
                                cx.notify();
                            })),
                    )
                    .child(
                        Switch::new("default-placeholder")
                            .checked(self.prefs.use_default_placeholder)
                            .label("默认占位画面")
                            .on_click(cx.listener(|this, checked, _, cx| {
                                this.prefs.use_default_placeholder = *checked;
                                this.runtime.set_use_default_placeholder(*checked);
                                let _ = this.persist_prefs();
                                cx.notify();
                            })),
                    ),
            )
            .child(
                GroupBox::new()
                    .outline()
                    .title("日志级别")
                    .children(LogLevel::ALL.iter().map(|level| {
                        let selected = self.prefs.log_level == *level;
                        let mut button = Button::new(format!("log-{level:?}")).label(level.label());
                        if selected {
                            button = button.primary();
                        }
                        let level = *level;
                        button
                            .on_click(cx.listener(move |this, _, _, cx| {
                                this.prefs.log_level = level;
                                this.apply_log_level();
                                let _ = this.persist_prefs();
                                cx.notify();
                            }))
                            .into_any_element()
                    })),
            )
            .child(
                GroupBox::new()
                    .outline()
                    .title("已配对设备")
                    .child(format!("共 {} 台设备", snapshot.trusted_device_count))
                    .when(snapshot.trusted_device_count > 0, |box_| {
                        box_.child(
                            Button::new("clear-all-trusted")
                                .label("清除全部配对")
                                .on_click(cx.listener(|this, _, _, cx| {
                                    let _ = this.runtime.clear_trusted_devices();
                                    cx.notify();
                                })),
                        )
                    })
                    .children(snapshot.trusted_devices.iter().map(|device| {
                        self.render_trusted_device_row(device, cx)
                            .into_any_element()
                    })),
            )
            .child(
                GroupBox::new()
                    .outline()
                    .title("虚拟摄像头修复")
                    .child(format!("状态：{}", vcam_label(self.vcam_status)))
                    .child(vcam_repair_hint(self.vcam_status))
                    .child(
                        Button::new("repair-vcam")
                            .label("重新检测 / 修复引导")
                            .on_click(cx.listener(|this, _, _, cx| {
                                this.try_register_vcam();
                                cx.notify();
                            })),
                    ),
            )
            .child(
                GroupBox::new()
                    .outline()
                    .title("诊断")
                    .child(
                        Button::new("export-diagnostics")
                            .label("导出诊断信息")
                            .on_click(cx.listener(|this, _, _, cx| {
                                this.export_diagnostics(cx);
                            })),
                    )
                    .children(
                        self.diagnostics_message
                            .as_ref()
                            .map(|path| {
                                vec![format!("已导出至 {path}（已脱敏，不含视频）")
                                    .into_any_element()]
                            })
                            .unwrap_or_default(),
                    )
                    .children(
                        self.diagnostics_error
                            .as_ref()
                            .map(|err| vec![format!("导出失败：{err}").into_any_element()])
                            .unwrap_or_default(),
                    ),
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
                    .child(format!(
                        "{} ({})",
                        picoo_diagnostics::redact_device_name(&device.device_name),
                        picoo_diagnostics::redact_device_id(&device.device_id)
                    ))
                    .child(format!(
                        "{} · last {} · fp={}",
                        device.platform,
                        crate::receiver_runtime::format_last_connected_ms(
                            device.last_connected_at_ms,
                        ),
                        picoo_diagnostics::redact_fingerprint(&device.certificate_fingerprint)
                    )),
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
        VirtualCameraStatus::Installed => "Installed",
        VirtualCameraStatus::NotInstalled => "Not Installed",
        VirtualCameraStatus::Active => "Active",
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

pub fn run_gpui_app() -> Result<(), ReceiverError> {
    let prefs = load_prefs();
    let _ = std::env::set_var("RUST_LOG", prefs.log_level.env_filter());
    // REQ-PICOO-UI-007: apply persisted startup preference at launch.
    if let Err(err) = crate::startup::sync_launch_at_startup(prefs.launch_at_startup) {
        tracing::warn!("startup sync on launch: {err}");
    }
    let vcam_status = detect_vcam_status();
    let runtime = ReceiverRuntime::from_prefs(&prefs)?;
    let mut runtime = runtime;
    runtime.set_virtual_camera_status(vcam_status);

    // REQ-PICOO-VCAM-002: keep Session-lifetime MF virtual camera for the desktop process.
    #[cfg(all(windows, feature = "windows-vcam"))]
    let _vcam_registration = match crate::vcam_register::VirtualCameraRegistration::register_and_start()
    {
        Ok(reg) => {
            tracing::info!("Picoo Camera virtual camera started for this session");
            runtime.set_virtual_camera_status(VirtualCameraStatus::Active);
            Some(reg)
        }
        Err(err) => {
            tracing::warn!("MF virtual camera start deferred: {err}");
            None
        }
    };

    let app = gpui_platform::application().with_assets(Assets);
    let prefs_for_window = prefs.clone();
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
            move |window, cx| {
                let display_name_input = cx.new(|cx| {
                    InputState::new(window, cx)
                        .default_value(prefs_for_window.display_name.clone())
                        .placeholder("桌面显示名称")
                });
                let view = cx.new(|_| {
                    PicooDesktopApp::new(runtime, prefs_for_window, display_name_input, vcam_status)
                });
                // REQ-PICOO-UI-008: close → tray hide (or quit) from Settings preference.
                let tray_view = view.clone();
                window.on_window_should_close(cx, move |window, cx| {
                    let outcome = tray_view.read(cx).close_outcome();
                    if outcome.hide_to_tray {
                        let status = tray_view.read(cx).runtime.snapshot().status;
                        let tip = crate::tray::tip_for_status(status);
                        crate::tray::note_hidden_to_tray_with_tip(&tip);
                        // App-level hide keeps the process; minimize covers hosts
                        // where hide() is a no-op until Shell_NotifyIcon lands.
                        cx.hide();
                        window.minimize_window();
                        return false;
                    }
                    crate::tray::note_tray_cleared();
                    outcome.allow_close
                });
                // HWND injection hook for Shell_NotifyIconW (Windows FindWindowW fallback).
                crate::tray::set_notify_icon_hwnd(None);
                cx.new(|cx| Root::new(view, window, cx).bg(cx.theme().background))
            },
        )
        .expect("open window");
    });

    Ok(())
}
