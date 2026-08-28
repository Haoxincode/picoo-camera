//! GPUI desktop shell — ARCH-PICOO-UI-001.
//!
//! First launch / Waiting / Live pages + Settings modal driven by [`ReceiverRuntime`] snapshots.

use std::path::PathBuf;
use std::sync::Arc;
use std::time::Duration;

use gpui::prelude::FluentBuilder;
use gpui::*;
use gpui_component::button::*;
use gpui_component::input::{Input, InputState};
use gpui_component::scroll::ScrollableElement;
use gpui_component::switch::*;
use gpui_component::*;
use gpui_component_assets::Assets;
use image::{Frame, ImageBuffer, Rgba};
use picoo_discovery::DEFAULT_QUIC_PORT;
use picoo_protocol::control::{camera_command, CameraCommand, Resolution};
use picoo_receiver::ReceiverError;
use picoo_session::ReceiverStatus;
use smallvec::smallvec;

use crate::diagnostics_export::export_diagnostics_to_file_with_hosts;
use crate::model::VirtualCameraStatus;
use crate::prefs::{load_prefs, save_prefs, DesktopPreferences, LogLevel};
use crate::preview_page::{
    preview_page_from_env, resolve_initial_shell, InitialDesktopPage,
};
use crate::qr_display;
use crate::receiver_runtime::{ReceiverRuntime, ReceiverSnapshot, TrustedDeviceSummary};
use crate::vcam_status::{detect_vcam_status, vcam_repair_hint};
use crate::video_surface::VideoSurface;

#[derive(Clone, Copy, PartialEq, Eq)]
enum DesktopPage {
    FirstLaunch,
    Waiting,
    Live,
}

pub struct PicooDesktopApp {
    runtime: ReceiverRuntime,
    prefs: DesktopPreferences,
    tray_policy: crate::tray::TrayPolicy,
    page: DesktopPage,
    /// AC-D-SET-01: settings as overlay modal (not a full page).
    settings_open: bool,
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
        let shell = resolve_initial_shell(
            preview_page_from_env(),
            prefs.first_launch_completed,
            vcam_status == VirtualCameraStatus::Unsupported,
        );
        let page = match shell.page {
            InitialDesktopPage::FirstLaunch => DesktopPage::FirstLaunch,
            InitialDesktopPage::Waiting => DesktopPage::Waiting,
            InitialDesktopPage::Live => DesktopPage::Live,
        };
        let settings_open = shell.settings_open;
        Self {
            runtime,
            prefs: prefs.clone(),
            tray_policy: crate::tray::TrayPolicy::from_pref(prefs.minimize_to_tray),
            page,
            settings_open,
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
        let filter = self.prefs.log_level.env_filter();
        let _ = std::env::set_var("RUST_LOG", filter);
        if let Err(err) = crate::logging::reload_filter(filter) {
            tracing::warn!("log level reload failed: {err}");
        }
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
                    self.diagnostics_error = None;
                    self.diagnostics_message =
                        Some("虚拟摄像头已激活（Picoo Camera / MF Session）".into());
                }
                Err(err) => {
                    tracing::warn!("Install Virtual Camera failed: {err}");
                    self.diagnostics_message = None;
                    self.diagnostics_error = Some(format!(
                        "虚拟摄像头激活失败：{err}。请以管理员运行，或重装 MSI 后重试。"
                    ));
                    self.refresh_vcam_status();
                }
            }
        }
        #[cfg(not(all(windows, feature = "windows-vcam")))]
        {
            self.refresh_vcam_status();
            self.diagnostics_message = Some(
                "Linux 用于验证桌面功能与 UI，不注册虚拟摄像头。会议软件接入只在 Windows / macOS。"
                    .into(),
            );
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
                self.diagnostics_message = Some(format!(
                    "已导出至 {}（已脱敏，不含视频）",
                    result.path.as_deref().unwrap_or("(未知路径)")
                ));
            }
            Err(err) => {
                self.diagnostics_message = None;
                self.diagnostics_error = Some(format!("导出失败：{err}"));
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
                    crate::tray::pump_win32_tray_messages();
                    crate::tray::sync_tray_tip(snapshot.status);
                    if let Some(action) = crate::tray::take_pending_menu_action() {
                        let outcome = action.apply();
                        if outcome.quit {
                            crate::tray::note_tray_cleared();
                            cx.quit();
                        } else if outcome.restore_window {
                            // Defer activate out of this entity update to avoid
                            // nested App/Entity RefCell borrows during pump.
                            cx.spawn(async move |_, cx| {
                                cx.background_executor()
                                    .timer(Duration::from_millis(0))
                                    .await;
                                let _ = cx.update(|cx| cx.activate(true));
                            })
                            .detach();
                        }
                    }
                    if matches!(snapshot.status, ReceiverStatus::Streaming) {
                        if this.page != DesktopPage::FirstLaunch {
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
                        // Product VCam only exists on Windows. Linux must stay Unsupported.
                        #[cfg(all(windows, feature = "windows-vcam"))]
                        {
                            this.vcam_status = VirtualCameraStatus::Active;
                            this.runtime
                                .set_virtual_camera_status(VirtualCameraStatus::Active);
                        }
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
        // Pump loop is started once after window open (not here) to avoid
        // re-entrant Entity updates / RefCell panics during render.
        let snapshot = self.snapshot();
        if self.page == DesktopPage::Waiting && self.show_qr {
            self.ensure_qr_image(&snapshot);
        }

        div()
            .v_flex()
            .size_full()
            .bg(cx.theme().background)
            .child(self.render_header(cx))
            .child(
                div()
                    .flex_1()
                    .relative()
                    .child(match self.page {
                        DesktopPage::FirstLaunch => self.render_first_launch(cx).into_any_element(),
                        DesktopPage::Live => self.render_live(&snapshot, cx).into_any_element(),
                        DesktopPage::Waiting => {
                            self.render_waiting(&snapshot, cx).into_any_element()
                        }
                    })
                    .when(self.settings_open, |this| {
                        this.child(self.render_settings_modal(&snapshot, cx))
                    })
                    .when(
                        matches!(snapshot.status, ReceiverStatus::Pairing)
                            && snapshot.pairing_short_code.is_some(),
                        |this| this.child(self.render_pairing_modal(&snapshot, cx)),
                    ),
            )
    }
}

impl PicooDesktopApp {
    fn render_header(&self, cx: &Context<Self>) -> impl IntoElement {
        // HTML `.desktop-titlebar`: title + GPUI badge + settings gear. No page nav.
        div()
            .h_flex()
            .w_full()
            .px_4()
            .py_3()
            .gap_3()
            .items_center()
            .bg(cx.theme().title_bar)
            .border_b_1()
            .border_color(cx.theme().title_bar_border)
            .child(
                div()
                    .h_flex()
                    .items_center()
                    .gap_2()
                    .child(
                        div()
                            .text_sm()
                            .font_weight(FontWeight::BOLD)
                            .text_color(cx.theme().foreground)
                            .child("Picoo Camera Receiver"),
                    )
                    .child(
                        div()
                            .px_1p5()
                            .py_0p5()
                            .rounded(px(4.))
                            .bg(cx.theme().primary.opacity(0.15))
                            .text_color(cx.theme().primary)
                            .text_xs()
                            .font_family("monospace")
                            .child("GPUI Native"),
                    ),
            )
            .child(div().flex_1())
            .child(
                Button::new("nav-settings")
                    .ghost()
                    .icon(IconName::Settings)
                    .tooltip("设置中心")
                    .on_click(cx.listener(|this, _, _, cx| {
                        this.settings_open = !this.settings_open;
                        cx.notify();
                    })),
            )
    }

    fn render_first_launch(&self, cx: &Context<Self>) -> impl IntoElement {
        div()
            .v_flex()
            .size_full()
            .items_center()
            .justify_center()
            .gap_4()
            .p_8()
            .bg(cx.theme().background)
            .text_color(cx.theme().foreground)
            .child(
                div()
                    .text_2xl()
                    .font_weight(FontWeight::BOLD)
                    .child("Picoo Camera"),
            )
            .child(
                div()
                    .text_color(cx.theme().muted_foreground)
                    .child("把手机变成电脑无线摄像头"),
            )
            .child(format!(
                "虚拟摄像头状态：{}",
                vcam_label_zh(self.vcam_status)
            ))
            .child(
                div()
                    .text_sm()
                    .text_color(cx.theme().muted_foreground)
                    .max_w_96()
                    .text_center()
                    .child(vcam_repair_hint(self.vcam_status)),
            )
            .when(
                self.vcam_status != VirtualCameraStatus::Unsupported,
                |this| {
                    this.child(
                        Button::new("refresh-vcam")
                            .label("重新检测虚拟摄像头")
                            .on_click(cx.listener(|this, _, _, cx| {
                                this.refresh_vcam_status();
                                cx.notify();
                            })),
                    )
                    .child(
                        Button::new("install-vcam")
                            .label("安装 / 激活虚拟摄像头")
                            .on_click(cx.listener(|this, _, _, cx| {
                                this.try_register_vcam();
                                cx.notify();
                            })),
                    )
                },
            )
            .children(
                self.diagnostics_message
                    .as_ref()
                    .map(|msg| {
                        div()
                            .text_sm()
                            .text_color(cx.theme().success)
                            .child(msg.clone())
                            .into_any_element()
                    })
                    .into_iter(),
            )
            .children(
                self.diagnostics_error
                    .as_ref()
                    .map(|err| {
                        div()
                            .text_sm()
                            .text_color(cx.theme().danger_foreground)
                            .max_w_96()
                            .text_center()
                            .child(err.clone())
                            .into_any_element()
                    })
                    .into_iter(),
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

    fn render_idle_brand_logo(&self, cx: &Context<Self>) -> impl IntoElement {
        div()
            .size(px(72.))
            .rounded_full()
            .flex()
            .items_center()
            .justify_center()
            .bg(cx.theme().primary)
            .shadow_lg()
            .child(
                div()
                    .h_flex()
                    .items_center()
                    .gap_1()
                    .child(
                        div()
                            .w(px(28.))
                            .h(px(20.))
                            .rounded(px(4.))
                            .border_2()
                            .border_color(gpui::white()),
                    )
                    .child(
                        div()
                            .w(px(10.))
                            .h(px(12.))
                            .rounded(px(2.))
                            .bg(gpui::white()),
                    ),
            )
    }

    fn render_waiting(&self, snapshot: &ReceiverSnapshot, cx: &Context<Self>) -> impl IntoElement {
        let endpoint = endpoint_label(snapshot);
        let vcam = vcam_label_zh(snapshot.virtual_camera);
        let vcam_ready = matches!(
            snapshot.virtual_camera,
            VirtualCameraStatus::Installed | VirtualCameraStatus::Active
        );

        div()
            .v_flex()
            .size_full()
            .items_center()
            .justify_center()
            .bg(cx.theme().background)
            .child(
                div()
                    .v_flex()
                    .items_center()
                    .gap_5()
                    .px_8()
                    .child(self.render_idle_brand_logo(cx))
                    .child(
                        div()
                            .text_2xl()
                            .font_weight(FontWeight::BOLD)
                            .text_color(cx.theme().foreground)
                            .child("等待手机连接…"),
                    )
                    .child(
                        div()
                            .max_w_96()
                            .text_center()
                            .text_sm()
                            .text_color(cx.theme().muted_foreground)
                            .child("在同一 Wi‑Fi 下打开手机端 Picoo Camera，将自动发现本电脑。也可通过手机直接扫描下方二维码建立直连。"),
                    )
                    .child(
                        div()
                            .px_4()
                            .py_2()
                            .rounded_full()
                            .when(vcam_ready, |this| {
                                this.bg(cx.theme().success.opacity(0.1))
                                    .border_1()
                                    .border_color(cx.theme().success.opacity(0.25))
                            })
                            .when(!vcam_ready, |this| {
                                this.bg(cx.theme().danger.opacity(0.1))
                                    .border_1()
                                    .border_color(cx.theme().danger.opacity(0.25))
                            })
                            .text_sm()
                            .text_color(if vcam_ready {
                                cx.theme().success
                            } else {
                                cx.theme().danger_foreground
                            })
                            .child(format!("● 虚拟摄像头驱动：{vcam}")),
                    )
                    .when(!vcam_ready, |this| {
                        this.child(
                            div()
                                .v_flex()
                                .items_center()
                                .gap_2()
                                .child(
                                    div()
                                        .text_xs()
                                        .text_color(cx.theme().muted_foreground)
                                        .max_w_96()
                                        .text_center()
                                        .child(vcam_repair_hint(self.vcam_status)),
                                )
                                .when(
                                    self.vcam_status != VirtualCameraStatus::Unsupported,
                                    |this| {
                                        this.child(
                                            Button::new("waiting-vcam-repair")
                                                .label("修复虚拟摄像头注册")
                                                .on_click(cx.listener(|this, _, _, cx| {
                                                    this.try_register_vcam();
                                                    cx.notify();
                                                })),
                                        )
                                    },
                                )
                                .children(
                                    self.diagnostics_message
                                        .as_ref()
                                        .map(|msg| {
                                            div()
                                                .text_xs()
                                                .text_color(cx.theme().success)
                                                .child(msg.clone())
                                                .into_any_element()
                                        })
                                        .into_iter(),
                                )
                                .children(
                                    self.diagnostics_error
                                        .as_ref()
                                        .map(|err| {
                                            div()
                                                .text_xs()
                                                .text_color(cx.theme().danger_foreground)
                                                .max_w_96()
                                                .text_center()
                                                .child(err.clone())
                                                .into_any_element()
                                        })
                                        .into_iter(),
                                ),
                        )
                    })
                    .child(self.render_qr_card(snapshot, cx))
                    .child(
                        div()
                            .text_xs()
                            .text_color(cx.theme().muted_foreground)
                            .child(format!(
                                "QUIC 监听 {endpoint} · 已配对 {} 台",
                                snapshot.trusted_device_count
                            )),
                    ),
            )
    }

    fn render_qr_card(&self, snapshot: &ReceiverSnapshot, cx: &Context<Self>) -> impl IntoElement {
        let endpoint = endpoint_label(snapshot);
        div()
            .h_flex()
            .gap_6()
            .p_5()
            .rounded_lg()
            .border_1()
            .border_color(cx.theme().border)
            .bg(cx.theme().secondary)
            .max_w(px(480.))
            .child(
                div()
                    .w(px(120.))
                    .h(px(120.))
                    .rounded_md()
                    .overflow_hidden()
                    .bg(gpui::white())
                    .p_2()
                    .flex()
                    .items_center()
                    .justify_center()
                    .child(if self.show_qr {
                        if let Some(image) = &self.qr_image {
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
                        }
                    } else {
                        Button::new("show-qr")
                            .label("Show QR Code")
                            .on_click(cx.listener(|this, _, _, cx| {
                                this.show_qr = true;
                                cx.notify();
                            }))
                            .into_any_element()
                    }),
            )
            .child(
                div()
                    .v_flex()
                    .gap_2()
                    .child(
                        div()
                            .font_weight(FontWeight::SEMIBOLD)
                            .text_color(cx.theme().foreground)
                            .child("Show QR Code (扫码直连)"),
                    )
                    .child(
                        div()
                            .text_sm()
                            .text_color(cx.theme().muted_foreground)
                            .child("企业网络 mDNS 受限时扫码直连 QUIC 端口"),
                    )
                    .child(
                        div()
                            .px_2()
                            .py_0p5()
                            .rounded_md()
                            .bg(cx.theme().warning.opacity(0.12))
                            .text_sm()
                            .font_family("monospace")
                            .text_color(cx.theme().warning)
                            .child(endpoint),
                    )
                    .when_some(snapshot.qr_nonce.clone(), |this, nonce| {
                        this.child(
                            div()
                                .text_xs()
                                .text_color(cx.theme().muted_foreground)
                                .child(format!("Nonce · {nonce}")),
                        )
                    })
                    .when(self.show_qr, |this| {
                        this.child(Button::new("toggle-qr").label("隐藏二维码").on_click(
                            cx.listener(|this, _, _, cx| {
                                this.show_qr = false;
                                cx.notify();
                            }),
                        ))
                    }),
            )
    }

    fn render_pairing_modal(
        &self,
        snapshot: &ReceiverSnapshot,
        cx: &Context<Self>,
    ) -> impl IntoElement {
        let pairing = snapshot
            .pairing_short_code
            .clone()
            .unwrap_or_else(|| "······".into());
        let pairing_digits: Vec<char> = pairing.chars().filter(|c| c.is_ascii_digit()).collect();
        let first_time = snapshot.trusted_device_count == 0
            || snapshot
                .active_sender
                .as_ref()
                .map(|s| {
                    !snapshot
                        .trusted_devices
                        .iter()
                        .any(|d| d.device_id == s.sender_id)
                })
                .unwrap_or(true);
        let sender_name = snapshot
            .active_sender
            .as_ref()
            .map(|s| s.device_name.clone())
            .unwrap_or_else(|| "手机".into());
        let pairing_ttl_label = {
            let ttl = self
                .runtime
                .receiver()
                .pairing_ttl_remaining()
                .map(|d| d.as_secs())
                .unwrap_or(0);
            if ttl > 0 {
                format!("握手上下文派生短码 · {ttl}s 内有效")
            } else {
                "短码已过期 · 请让手机重新发起配对".into()
            }
        };

        div()
            .absolute()
            .inset_0()
            .bg(cx.theme().overlay)
            .flex()
            .items_center()
            .justify_center()
            .child(
                div()
                    .v_flex()
                    .gap_4()
                    .p_6()
                    .w_96()
                    .rounded_lg()
                    .border_1()
                    .border_color(cx.theme().border)
                    .bg(cx.theme().popover)
                    .shadow_lg()
                    .child(
                        div()
                            .text_lg()
                            .font_weight(FontWeight::BOLD)
                            .text_color(cx.theme().foreground)
                            .child("核对配对短码"),
                    )
                    .child(
                        div()
                            .text_sm()
                            .text_color(cx.theme().muted_foreground)
                            .child(format!(
                        "来自 {sender_name} 的{}连接请求。请确认手机上显示相同的 6 位数字：",
                        if first_time { "首次" } else { "" }
                    )),
                    )
                    .child(
                        div()
                            .v_flex()
                            .items_center()
                            .p_4()
                            .rounded_md()
                            .border_1()
                            .border_color(cx.theme().border)
                            .bg(cx.theme().background)
                            .child(
                                div()
                                    .h_flex()
                                    .gap_2()
                                    .children(if pairing_digits.len() == 6 {
                                        pairing_digits
                                            .into_iter()
                                            .map(|digit| pairing_code_box(digit, cx))
                                            .collect::<Vec<_>>()
                                    } else {
                                        vec![div()
                                            .text_3xl()
                                            .font_weight(FontWeight::BOLD)
                                            .font_family("monospace")
                                            .text_color(cx.theme().foreground)
                                            .child(format_pairing_code(&pairing))
                                            .into_any_element()]
                                    }),
                            )
                            .child(
                                div()
                                    .mt_1()
                                    .text_xs()
                                    .text_color(cx.theme().muted_foreground)
                                    .child(pairing_ttl_label),
                            ),
                    )
                    .child(
                        div()
                            .h_flex()
                            .gap_2()
                            .justify_end()
                            .child(Button::new("cancel-pairing").label("拒绝").on_click(
                                cx.listener(|this, _, _, cx| {
                                    this.runtime.disconnect();
                                    cx.notify();
                                }),
                            ))
                            .child(
                                Button::new("confirm-pairing")
                                    .primary()
                                    .label("两端一致，确认配对")
                                    .on_click(cx.listener(|this, _, _, cx| {
                                        this.runtime.confirm_pairing();
                                        cx.notify();
                                    })),
                            ),
                    ),
            )
    }

    fn send_live_camera_command(&mut self, command: CameraCommand) {
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

    fn render_live(&self, snapshot: &ReceiverSnapshot, cx: &Context<Self>) -> impl IntoElement {
        let streaming = matches!(snapshot.status, ReceiverStatus::Streaming);
        let res_label = snapshot
            .stream_config
            .as_ref()
            .map(|c| {
                if c.height >= 1080 {
                    "1080p30".into()
                } else if c.height >= 720 {
                    "720p30".into()
                } else if c.height >= 480 {
                    "480p30".into()
                } else {
                    format!("{}p{}", c.height, c.fps)
                }
            })
            .unwrap_or_else(|| "—".into());
        let fps = snapshot
            .stream_config
            .as_ref()
            .map(|c| format!("{:.1} FPS", c.fps as f64))
            .unwrap_or_else(|| "—".into());
        let sender_name = snapshot
            .active_sender
            .as_ref()
            .map(|s| s.device_name.clone())
            .unwrap_or_else(|| "等待 Sender".into());
        let quality = crate::network_quality::network_quality_label(
            snapshot.stream_metrics.packet_loss,
            snapshot.stream_metrics.latency_ms,
        );
        let bitrate = snapshot.stream_metrics.bitrate_bps as f64 / 1_000_000.0;
        let rtt = snapshot.stream_metrics.latency_ms;
        let loss_pct = snapshot.stream_metrics.packet_loss * 100.0;
        let jitter = snapshot.link_jitter_ms;
        let remote_mirrored = snapshot
            .stream_config
            .as_ref()
            .map(|c| c.mirrored)
            .unwrap_or(false);

        div()
            .v_flex()
            .size_full()
            .bg(cx.theme().sidebar)
            .child(
                div()
                    .flex_1()
                    .relative()
                    .bg(cx.theme().tiles)
                    .overflow_hidden()
                    .child(self.video_surface.render_preview())
                    .child(
                        div()
                            .absolute()
                            .top_3()
                            .left_4()
                            .right_4()
                            .h_flex()
                            .justify_between()
                            .child(live_hud_pill(format!("● {sender_name}"), cx))
                            .child(live_hud_pill(
                                format!(
                                    "Virtual Camera: {}",
                                    vcam_label(snapshot.virtual_camera).to_uppercase()
                                ),
                                cx,
                            )),
                    ),
            )
            .child(
                div()
                    .v_flex()
                    .gap_3()
                    .p_4()
                    .bg(cx.theme().secondary)
                    .border_t_1()
                    .border_color(cx.theme().border)
                    .child(
                        div()
                            .h_flex()
                            .gap_2()
                            .flex_wrap()
                            .child(telemetry_cell("画质规格", res_label, cx))
                            .child(telemetry_cell("实时帧率", fps, cx))
                            .child(telemetry_cell("接收码率", format!("{bitrate:.1} Mbps"), cx))
                            .child(telemetry_cell("RTT 延迟", format!("{rtt:.0} ms"), cx))
                            .child(telemetry_cell(
                                "丢包 / 抖动",
                                format!("{loss_pct:.1}% · {jitter:.0} ms"),
                                cx,
                            ))
                            .child(telemetry_cell("网络质量", quality.into(), cx)),
                    )
                    .when(streaming, |this| {
                        this.child(
                            div()
                                .h_flex()
                                .gap_2()
                                .flex_wrap()
                                .items_center()
                                .child(Button::new("cam-front").label("前置").on_click(
                                    cx.listener(|this, _, _, cx| {
                                        this.send_live_camera_command(CameraCommand {
                                            command: camera_command::Command::SwitchFront as i32,
                                            resolution: None,
                                            mirrored: false,
                                        });
                                        cx.notify();
                                    }),
                                ))
                                .child(Button::new("cam-back").label("后置").on_click(cx.listener(
                                    |this, _, _, cx| {
                                        this.send_live_camera_command(CameraCommand {
                                            command: camera_command::Command::SwitchBack as i32,
                                            resolution: None,
                                            mirrored: false,
                                        });
                                        cx.notify();
                                    },
                                )))
                                .child(Button::new("res-480").label("480p").on_click(cx.listener(
                                    |this, _, _, cx| {
                                        this.send_live_camera_command(CameraCommand {
                                            command: camera_command::Command::SetResolution as i32,
                                            resolution: Some(Resolution {
                                                width: 854,
                                                height: 480,
                                            }),
                                            mirrored: false,
                                        });
                                        cx.notify();
                                    },
                                )))
                                .child(Button::new("res-720").label("720p").on_click(cx.listener(
                                    |this, _, _, cx| {
                                        this.send_live_camera_command(CameraCommand {
                                            command: camera_command::Command::SetResolution as i32,
                                            resolution: Some(Resolution {
                                                width: 1280,
                                                height: 720,
                                            }),
                                            mirrored: false,
                                        });
                                        cx.notify();
                                    },
                                )))
                                .child(Button::new("res-1080").label("1080p").on_click(
                                    cx.listener(|this, _, _, cx| {
                                        this.send_live_camera_command(CameraCommand {
                                            command: camera_command::Command::SetResolution as i32,
                                            resolution: Some(Resolution {
                                                width: 1920,
                                                height: 1080,
                                            }),
                                            mirrored: false,
                                        });
                                        cx.notify();
                                    }),
                                ))
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
                                ),
                        )
                    })
                    .children(
                        self.diagnostics_error
                            .as_ref()
                            .map(|err| {
                                div()
                                    .text_xs()
                                    .text_color(cx.theme().danger_foreground)
                                    .child(err.clone())
                                    .into_any_element()
                            })
                            .into_iter(),
                    )
                    .child(
                        div()
                            .h_flex()
                            .gap_2()
                            .justify_end()
                            .child(Button::new("request-idr").label("请求关键帧").on_click(
                                cx.listener(|this, _, _, cx| {
                                    if let Err(err) = this.runtime.request_keyframe() {
                                        tracing::warn!("RequestKeyframe failed: {err}");
                                    }
                                    cx.notify();
                                }),
                            ))
                            .child(
                                div()
                                    .id("disconnect")
                                    .px_3()
                                    .py_1p5()
                                    .rounded_md()
                                    .bg(cx.theme().danger.opacity(0.2))
                                    .border_1()
                                    .border_color(cx.theme().danger.opacity(0.4))
                                    .text_sm()
                                    .font_weight(FontWeight::SEMIBOLD)
                                    .text_color(cx.theme().danger_foreground)
                                    .cursor_pointer()
                                    .on_mouse_down(
                                        gpui::MouseButton::Left,
                                        cx.listener(|this, _, _, cx| {
                                            this.runtime.disconnect();
                                            this.page = DesktopPage::Waiting;
                                            cx.notify();
                                        }),
                                    )
                                    .child("断开会话"),
                            ),
                    ),
            )
    }

    fn render_settings_modal(
        &self,
        snapshot: &ReceiverSnapshot,
        cx: &Context<Self>,
    ) -> impl IntoElement {
        // AC-D-SET-01: settings overlay (prototype #d-modal-settings).
        div()
            .absolute()
            .inset_0()
            .bg(cx.theme().overlay)
            .flex()
            .items_center()
            .justify_center()
            .child(
                div()
                    .v_flex()
                    .gap_3()
                    .p_6()
                    .w(px(620.))
                    .max_h(px(640.))
                    // ScrollableElement must be in scope — Windows CI failed without it.
                    .overflow_y_scrollbar()
                    .rounded_lg()
                    .border_1()
                    .border_color(cx.theme().border)
                    .bg(cx.theme().popover)
                    .shadow_lg()
                    .child(
                        div()
                            .v_flex()
                            .gap_1()
                            .child(
                                div()
                                    .text_lg()
                                    .font_weight(FontWeight::BOLD)
                                    .text_color(cx.theme().foreground)
                                    .child("桌面端设置中心"),
                            )
                            .child(
                                div()
                                    .text_sm()
                                    .text_color(cx.theme().muted_foreground)
                                    .child("Receiver 运行参数、虚拟摄像头状态与已配对设备管理"),
                            ),
                    )
                    .child(self.render_settings(snapshot, cx))
                    .child(
                        div()
                            .h_flex()
                            .justify_end()
                            .child(
                                Button::new("close-settings")
                                    .primary()
                                    .label("完成并保存")
                                    .on_click(cx.listener(|this, _, _, cx| {
                                        this.save_display_name(cx);
                                        this.settings_open = false;
                                        cx.notify();
                                    })),
                            ),
                    ),
            )
    }

    fn render_settings(&self, snapshot: &ReceiverSnapshot, cx: &Context<Self>) -> impl IntoElement {
        let ring_hint = match &snapshot.shared_ring_error {
            Some(err) => format!("Shared Frame Ring 附着失败 — {err}"),
            None if self.vcam_status == VirtualCameraStatus::Unsupported => {
                "本机预览可用；不向会议软件输出".into()
            }
            None => "Shared Frame Ring 已附着（VCam DLL 可读帧）".into(),
        };

        div()
            .v_flex()
            .gap_3()
            .child(settings_group("基础偏好设置", cx, |group| {
                group
                    .child(settings_row(
                        "电脑显示名称",
                        "在手机发现列表中显示的设备名",
                        cx,
                        div()
                            .h_flex()
                            .gap_2()
                            .items_center()
                            .child(div().w(px(160.)).child(Input::new(&self.display_name_input)))
                            .child(
                                Button::new("save-display-name")
                                    .ghost()
                                    .small()
                                    .label("保存")
                                    .on_click(cx.listener(|this, _, _, cx| {
                                        this.save_display_name(cx);
                                    })),
                            )
                            .into_any_element(),
                    ))
                    .child(settings_row(
                        "自动接受已配对设备",
                        "已验证固定公钥的手机连接时直接开始推流",
                        cx,
                        Switch::new("auto-accept-paired")
                            .checked(self.prefs.auto_accept_paired)
                            .on_click(cx.listener(|this, checked, _, cx| {
                                this.prefs.auto_accept_paired = *checked;
                                this.runtime.set_auto_accept_paired(*checked);
                                let _ = this.persist_prefs();
                                cx.notify();
                            }))
                            .into_any_element(),
                    ))
                    .child(settings_row(
                        "开机启动",
                        "登录后自动打开 Receiver",
                        cx,
                        Switch::new("launch-at-startup")
                            .checked(self.prefs.launch_at_startup)
                            .on_click(cx.listener(|this, checked, _, cx| {
                                this.prefs.launch_at_startup = *checked;
                                if let Err(err) = crate::startup::sync_launch_at_startup(*checked) {
                                    tracing::warn!("launch-at-startup sync failed: {err}");
                                }
                                let _ = this.persist_prefs();
                                cx.notify();
                            }))
                            .into_any_element(),
                    ))
                    .child(settings_row(
                        "最小化到系统托盘",
                        "关闭窗口后保持后台运行",
                        cx,
                        Switch::new("minimize-to-tray")
                            .checked(self.prefs.minimize_to_tray)
                            .on_click(cx.listener(|this, checked, _, cx| {
                                this.prefs.minimize_to_tray = *checked;
                                this.tray_policy = crate::tray::TrayPolicy::from_pref(*checked);
                                let _ = this.persist_prefs();
                                cx.notify();
                            }))
                            .into_any_element(),
                    ))
            }))
            .child(settings_group("虚拟摄像头管理 (Virtual Camera)", cx, |group| {
                group
                    .child(settings_row(
                        "系统虚拟摄像头驱动状态",
                        vcam_repair_hint(self.vcam_status),
                        cx,
                        div()
                            .h_flex()
                            .gap_2()
                            .items_center()
                            .child(
                                div()
                                    .text_sm()
                                    .text_color(if self.vcam_status == VirtualCameraStatus::Unsupported {
                                        cx.theme().danger_foreground
                                    } else {
                                        cx.theme().success
                                    })
                                    .child(vcam_label_zh(self.vcam_status)),
                            )
                            .when(
                                self.vcam_status != VirtualCameraStatus::Unsupported,
                                |this| {
                                    this.child(
                                        Button::new("repair-vcam")
                                            .ghost()
                                            .small()
                                            .label("修复 / 重新激活")
                                            .on_click(cx.listener(|this, _, _, cx| {
                                                this.try_register_vcam();
                                                cx.notify();
                                            })),
                                    )
                                },
                            )
                            .into_any_element(),
                    ))
                    .child(settings_row(
                        "未推流时默认占位画面",
                        ring_hint.as_str(),
                        cx,
                        div()
                            .h_flex()
                            .gap_1()
                            .children(crate::prefs::PlaceholderModePref::ALL.iter().map(|mode| {
                                let selected = self.prefs.placeholder_mode == *mode;
                                let mut button = Button::new(format!("placeholder-{mode:?}"))
                                    .small()
                                    .ghost()
                                    .label(mode.label());
                                if selected {
                                    button = button.primary();
                                }
                                let mode = *mode;
                                button
                                    .on_click(cx.listener(move |this, _, _, cx| {
                                        this.prefs.placeholder_mode = mode;
                                        this.runtime.set_placeholder_mode(mode.to_frame_hub());
                                        let _ = this.persist_prefs();
                                        cx.notify();
                                    }))
                                    .into_any_element()
                            }))
                            .into_any_element(),
                    ))
            }))
            .child(settings_group("信任设备管理 (PUC-007)", cx, |group| {
                group
                    .child(
                        div()
                            .h_flex()
                            .items_center()
                            .justify_between()
                            .child(
                                div()
                                    .text_sm()
                                    .text_color(cx.theme().muted_foreground)
                                    .child(format!("共 {} 台设备", snapshot.trusted_device_count)),
                            )
                            .when(snapshot.trusted_device_count > 0, |this| {
                                this.child(
                                    Button::new("clear-all-trusted")
                                        .ghost()
                                        .small()
                                        .label("清除全部配对")
                                        .on_click(cx.listener(|this, _, _, cx| {
                                            match this.runtime.clear_trusted_devices() {
                                                Ok(n) => {
                                                    this.diagnostics_error = None;
                                                    this.diagnostics_message =
                                                        Some(format!("已清除 {n} 个配对设备"));
                                                }
                                                Err(err) => {
                                                    this.diagnostics_error =
                                                        Some(format!("清除配对失败：{err}"));
                                                }
                                            }
                                            cx.notify();
                                        })),
                                )
                            }),
                    )
                    .children(snapshot.trusted_devices.iter().map(|device| {
                        self.render_trusted_device_row(device, cx)
                            .into_any_element()
                    }))
            }))
            .child(settings_group("诊断与隐私", cx, |group| {
                group
                    .child(settings_row(
                        "日志级别",
                        "运行时 reload EnvFilter，不重启进程",
                        cx,
                        div()
                            .h_flex()
                            .gap_1()
                            .children(LogLevel::ALL.iter().map(|level| {
                                let selected = self.prefs.log_level == *level;
                                let mut button = Button::new(format!("log-{level:?}"))
                                    .small()
                                    .ghost()
                                    .label(level.label());
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
                            }))
                            .into_any_element(),
                    ))
                    .child(settings_row(
                        "日志脱敏导出",
                        "默认脱敏 IP、设备名与公钥指纹，不包含视频帧数据",
                        cx,
                        Button::new("export-diagnostics")
                            .ghost()
                            .small()
                            .label("导出诊断日志 (.json)")
                            .on_click(cx.listener(|this, _, _, cx| {
                                this.export_diagnostics(cx);
                            }))
                            .into_any_element(),
                    ))
                    .children(
                        self.diagnostics_message
                            .as_ref()
                            .map(|msg| {
                                div()
                                    .text_sm()
                                    .text_color(cx.theme().success)
                                    .child(msg.clone())
                                    .into_any_element()
                            })
                            .into_iter(),
                    )
                    .children(
                        self.diagnostics_error
                            .as_ref()
                            .map(|err| {
                                div()
                                    .text_sm()
                                    .text_color(cx.theme().danger_foreground)
                                    .child(err.clone())
                                    .into_any_element()
                            })
                            .into_iter(),
                    )
            }))
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
                    .child(format!(
                        "{} · 上次 {} · 公钥指纹 {}",
                        device.platform,
                        crate::receiver_runtime::format_last_connected_ms(
                            device.last_connected_at_ms,
                        ),
                        // Fingerprint remains abbreviated in UI; full redaction is for export only.
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
                            match this.runtime.remove_trusted_device(&device_id) {
                                Ok(true) => {
                                    this.diagnostics_error = None;
                                    this.diagnostics_message =
                                        Some(format!("已删除配对：{device_id}"));
                                }
                                Ok(false) => {
                                    this.diagnostics_error =
                                        Some(format!("未找到配对设备：{device_id}"));
                                }
                                Err(err) => {
                                    this.diagnostics_error = Some(format!("删除配对失败：{err}"));
                                }
                            }
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
        VirtualCameraStatus::Unsupported => "Unsupported",
    }
}

fn vcam_label_zh(status: VirtualCameraStatus) -> &'static str {
    match status {
        VirtualCameraStatus::Unknown => "检测中",
        VirtualCameraStatus::Installed => "就绪 (Ready)",
        VirtualCameraStatus::NotInstalled => "未安装 (Not Installed)",
        VirtualCameraStatus::Active => "就绪 (Active)",
        VirtualCameraStatus::Unsupported => "本平台不适用",
    }
}

fn settings_group(
    title: &'static str,
    cx: &Context<PicooDesktopApp>,
    build: impl FnOnce(Div) -> Div,
) -> impl IntoElement {
    build(
        div()
            .v_flex()
            .gap_1()
            .p_4()
            .rounded_lg()
            .border_1()
            .border_color(cx.theme().border)
            .bg(cx.theme().background)
            .child(
                div()
                    .text_xs()
                    .font_weight(FontWeight::BOLD)
                    .text_color(cx.theme().muted_foreground)
                    .child(title.to_uppercase()),
            ),
    )
}

fn settings_row(
    title: &'static str,
    hint: &str,
    cx: &Context<PicooDesktopApp>,
    control: impl IntoElement,
) -> impl IntoElement {
    div()
        .h_flex()
        .w_full()
        .items_center()
        .justify_between()
        .gap_3()
        .py_2()
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
                        .text_color(cx.theme().foreground)
                        .child(title),
                )
                .child(
                    div()
                        .text_xs()
                        .text_color(cx.theme().muted_foreground)
                        .child(hint.to_string()),
                ),
        )
        .child(control)
}

fn endpoint_label(snapshot: &ReceiverSnapshot) -> String {
    if snapshot.advertise_host.is_empty() {
        return "—".into();
    }
    format!("{}:{DEFAULT_QUIC_PORT}", snapshot.advertise_host)
}

fn live_hud_pill(label: String, cx: &Context<PicooDesktopApp>) -> impl IntoElement {
    div()
        .px_3()
        .py_1p5()
        .rounded(px(8.))
        .bg(cx.theme().sidebar.opacity(0.7))
        .border_1()
        .border_color(cx.theme().border)
        .text_xs()
        .font_weight(FontWeight::SEMIBOLD)
        .text_color(cx.theme().foreground)
        .child(label)
}

fn format_pairing_code(code: &str) -> String {
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

fn pairing_code_box(digit: char, cx: &Context<PicooDesktopApp>) -> gpui::AnyElement {
    div()
        .w(px(40.))
        .h(px(48.))
        .flex()
        .items_center()
        .justify_center()
        .rounded_md()
        .border_1()
        .border_color(cx.theme().border)
        .bg(cx.theme().muted)
        .text_xl()
        .font_family("monospace")
        .font_weight(FontWeight::BOLD)
        .text_color(cx.theme().foreground)
        .child(digit.to_string())
        .into_any_element()
}

fn telemetry_cell(
    label: &'static str,
    value: String,
    cx: &Context<PicooDesktopApp>,
) -> impl IntoElement {
    div()
        .v_flex()
        .gap_0p5()
        .child(
            div()
                .text_xs()
                .font_weight(FontWeight::BOLD)
                .text_color(cx.theme().muted_foreground)
                .child(label.to_uppercase()),
        )
        .child(
            div()
                .text_sm()
                .font_family("monospace")
                .font_weight(FontWeight::BOLD)
                .text_color(cx.theme().foreground)
                .child(value),
        )
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
    // Ensure subscriber exists even if main skipped prefs-aware init paths.
    crate::logging::init_logging(prefs.log_level.env_filter());
    let _ = std::env::set_var("RUST_LOG", prefs.log_level.env_filter());
    let _ = crate::logging::reload_filter(prefs.log_level.env_filter());
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
    let _vcam_registration =
        match crate::vcam_register::VirtualCameraRegistration::register_and_start() {
            Ok(reg) => {
                tracing::info!("Picoo Camera virtual camera started for this session");
                runtime.set_virtual_camera_status(VirtualCameraStatus::Active);
                Some(reg)
            }
            Err(err) => {
                tracing::warn!(
                    "MF virtual camera start deferred: {err} (try Settings → 安装/激活虚拟摄像头, or run as Administrator)"
                );
                None
            }
        };

    let app = gpui_platform::application().with_assets(Assets);
    let prefs_for_window = prefs.clone();
    app.run(move |cx| {
        gpui_component::init(cx);
        crate::picoo_theme::apply_picoo_theme(cx);
        cx.set_app_identity("com.picoo.camera", "Picoo Camera");
        cx.activate(true);

        cx.open_window(
            WindowOptions {
                titlebar: Some(TitlebarOptions {
                    title: Some("Picoo Camera Receiver".into()),
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
                // Start frame/tray pump after the view exists — not inside Render.
                view.update(cx, |this, cx| {
                    this.ensure_pump_loop(cx);
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
                // Tray uses a Win32 message-only host (FindWindowW fallback remains).
                let _ = crate::tray::pump_win32_tray_messages();
                cx.new(|cx| Root::new(view, window, cx).bg(cx.theme().background))
            },
        )
        .expect("open window");
    });

    Ok(())
}
