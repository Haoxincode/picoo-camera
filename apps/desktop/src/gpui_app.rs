//! GPUI desktop shell — ARCH-PICOO-UI-001.
//!
//! Desktop navigation and receiver states driven by [`ReceiverRuntime`] snapshots.

use std::borrow::Cow;
use std::path::PathBuf;
use std::time::Duration;

use gpui::prelude::FluentBuilder;
use gpui::*;
use gpui_component::animation::{ease_in_out_cubic, EffectTransition};
use gpui_component::button::*;
use gpui_component::dialog::DialogButtonProps;
use gpui_component::input::{Input, InputEvent, InputState};
use gpui_component::scroll::ScrollableElement;
use gpui_component::switch::*;
use gpui_component::*;
use gpui_component_assets::Assets;
use picoo_discovery::DEFAULT_QUIC_PORT;
use picoo_protocol::control::{camera_command, CameraCommand, Resolution};
use picoo_receiver::ReceiverError;
use picoo_session::ReceiverStatus;

use crate::diagnostics_export::export_diagnostics_to_file_with_hosts;
use crate::model::VirtualCameraStatus;
use crate::prefs::{load_prefs, save_prefs, DesktopPreferences, LogLevel};
use crate::receiver_runtime::{ReceiverRuntime, ReceiverSnapshot};
use crate::vcam_status::{
    detect_vcam_status, vcam_repair_hint, vcam_setup_action_label, vcam_setup_unavailable_message,
};
use crate::video_surface::VideoSurface;

const DEVICE_FRAME_ASSETS: [(&str, &[u8]); 3] = [
    (
        "device-frames/iphone-16-max.svg",
        include_bytes!("../../../assets/device-frames/iphone-16-max.svg"),
    ),
    (
        "device-frames/macbook-pro-light.svg",
        include_bytes!("../../../assets/device-frames/macbook-pro-light.svg"),
    ),
    (
        "device-frames/macbook-pro-dark.svg",
        include_bytes!("../../../assets/device-frames/macbook-pro-dark.svg"),
    ),
];

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

struct PicooAssets;

impl AssetSource for PicooAssets {
    fn load(&self, path: &str) -> Result<Option<Cow<'static, [u8]>>> {
        if let Some((_, bytes)) = DEVICE_FRAME_ASSETS
            .iter()
            .find(|(asset_path, _)| *asset_path == path)
        {
            return Ok(Some(Cow::Borrowed(bytes)));
        }
        Assets.load(path)
    }

    fn list(&self, path: &str) -> Result<Vec<SharedString>> {
        let mut assets = Assets.list(path)?;
        assets.extend(
            DEVICE_FRAME_ASSETS
                .iter()
                .filter(|(asset_path, _)| asset_path.starts_with(path))
                .map(|(asset_path, _)| SharedString::from(*asset_path)),
        );
        Ok(assets)
    }
}

#[derive(Clone, Copy, PartialEq, Eq)]
enum DesktopPage {
    FirstLaunch,
    Waiting,
    Live,
}

#[derive(Clone, Copy, PartialEq, Eq)]
enum DesktopSection {
    Connect,
    VirtualCamera,
    Network,
    General,
    Help,
    About,
}

impl DesktopSection {
    fn label(self) -> &'static str {
        match self {
            Self::Connect => "连接",
            Self::VirtualCamera => "虚拟摄像头",
            Self::Network => "网络",
            Self::General => "通用",
            Self::Help => "帮助",
            Self::About => "关于",
        }
    }
}

#[derive(Clone, PartialEq, Eq)]
enum VcamSetupState {
    Idle,
    #[cfg(all(windows, feature = "windows-vcam"))]
    Running,
    #[cfg(all(windows, feature = "windows-vcam"))]
    Succeeded(String),
    Failed(String),
}

impl VcamSetupState {
    fn is_running(&self) -> bool {
        #[cfg(all(windows, feature = "windows-vcam"))]
        {
            matches!(self, Self::Running)
        }
        #[cfg(not(all(windows, feature = "windows-vcam")))]
        {
            false
        }
    }
}

pub struct PicooDesktopApp {
    runtime: ReceiverRuntime,
    prefs: DesktopPreferences,
    tray_policy: crate::tray::TrayPolicy,
    page: DesktopPage,
    section: DesktopSection,
    sidebar_collapsed: bool,
    pump_started: bool,
    last_presented_snapshot: ReceiverSnapshot,
    video_surface: VideoSurface,
    display_name_input: Entity<InputState>,
    _subscriptions: Vec<Subscription>,
    vcam_status: VirtualCameraStatus,
    vcam_setup_state: VcamSetupState,
    diagnostics_message: Option<String>,
    diagnostics_error: Option<String>,
    window_handle: AnyWindowHandle,
    pairing_dialog_code: Option<String>,
    pairing_dialog_visible: bool,
    /// Holds Session-lifetime MF virtual camera while the UI is open.
    #[cfg(all(windows, feature = "windows-vcam"))]
    vcam_registration: Option<crate::vcam_register::VirtualCameraSessionHost>,
}

impl PicooDesktopApp {
    fn new(
        runtime: ReceiverRuntime,
        prefs: DesktopPreferences,
        vcam_status: VirtualCameraStatus,
        window_handle: AnyWindowHandle,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> Self {
        let display_name_input = cx.new(|cx| {
            InputState::new(window, cx)
                .default_value(prefs.display_name.clone())
                .placeholder("桌面显示名称")
        });
        let _subscriptions = vec![cx.subscribe_in(
            &display_name_input,
            window,
            |this, _, event: &InputEvent, _, cx| {
                if matches!(event, InputEvent::Change) {
                    this.save_display_name(cx);
                }
            },
        )];
        let page = if prefs.first_launch_completed {
            DesktopPage::Waiting
        } else {
            DesktopPage::FirstLaunch
        };
        let last_presented_snapshot = runtime.snapshot();
        Self {
            runtime,
            prefs: prefs.clone(),
            tray_policy: crate::tray::TrayPolicy::for_current_platform(prefs.minimize_to_tray),
            page,
            section: DesktopSection::Connect,
            sidebar_collapsed: false,
            pump_started: false,
            last_presented_snapshot,
            video_surface: VideoSurface::default(),
            display_name_input,
            _subscriptions,
            vcam_status,
            vcam_setup_state: VcamSetupState::Idle,
            diagnostics_message: None,
            diagnostics_error: None,
            window_handle,
            pairing_dialog_code: None,
            pairing_dialog_visible: false,
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

    fn persist_prefs(&mut self) -> Result<(), String> {
        save_prefs(&self.prefs)
    }

    fn apply_log_level(&self) {
        let filter = self.prefs.log_level.env_filter();
        std::env::set_var("RUST_LOG", filter);
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
        self.vcam_setup_state = VcamSetupState::Idle;
    }

    fn vcam_setup_button_label(&self) -> &'static str {
        if self.vcam_setup_state.is_running() {
            "正在等待管理员授权…"
        } else {
            vcam_setup_action_label()
        }
    }

    fn render_vcam_setup_feedback(&self, cx: &Context<Self>) -> Option<AnyElement> {
        match &self.vcam_setup_state {
            VcamSetupState::Idle => None,
            #[cfg(all(windows, feature = "windows-vcam"))]
            VcamSetupState::Running => Some(
                div()
                    .text_sm()
                    .text_color(cx.theme().muted_foreground)
                    .child(
                        "请在 Windows 用户账户控制中允许 Picoo Camera 修改设备，然后返回此窗口。",
                    )
                    .into_any_element(),
            ),
            #[cfg(all(windows, feature = "windows-vcam"))]
            VcamSetupState::Succeeded(message) => Some(
                div()
                    .text_sm()
                    .text_color(cx.theme().success)
                    .child(message.clone())
                    .into_any_element(),
            ),
            VcamSetupState::Failed(message) => Some(
                div()
                    .text_sm()
                    .text_color(cx.theme().danger)
                    .child(message.clone())
                    .into_any_element(),
            ),
        }
    }

    fn try_register_vcam(&mut self, cx: &mut Context<Self>) {
        if self.vcam_setup_state.is_running() {
            return;
        }

        #[cfg(all(windows, feature = "windows-vcam"))]
        {
            self.vcam_setup_state = VcamSetupState::Running;
            cx.notify();

            let repair = cx.background_executor().spawn_dedicated(|_| async move {
                crate::vcam_register::repair_and_start_elevated()
            });
            cx.spawn(async move |this, cx| {
                let repair_result = repair.await;
                let _ = this.update(cx, |this, cx| {
                    match repair_result {
                        Ok(registration) => {
                            this.vcam_registration = Some(registration);
                            this.vcam_status = VirtualCameraStatus::Active;
                            this.runtime
                                .set_virtual_camera_status(VirtualCameraStatus::Active);
                            this.vcam_setup_state =
                                VcamSetupState::Succeeded("虚拟摄像头已修复并激活。".into());
                        }
                        Err(err) => {
                            tracing::warn!("Install or repair Virtual Camera failed: {err}");
                            this.refresh_vcam_status();
                            this.vcam_setup_state = VcamSetupState::Failed(err);
                        }
                    }
                    cx.notify();
                });
            })
            .detach();
        }
        #[cfg(target_os = "macos")]
        {
            self.refresh_vcam_status();
            self.vcam_setup_state = VcamSetupState::Failed(vcam_setup_unavailable_message().into());
            cx.notify();
        }
        #[cfg(not(any(target_os = "macos", all(windows, feature = "windows-vcam"))))]
        {
            self.refresh_vcam_status();
            self.vcam_setup_state = VcamSetupState::Failed(vcam_setup_unavailable_message().into());
            cx.notify();
        }
    }

    fn save_display_name(&mut self, cx: &mut Context<Self>) {
        let name = self
            .display_name_input
            .read(cx)
            .value()
            .trim()
            .chars()
            .take(32)
            .collect::<String>();
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
                    let video_changed = this
                        .runtime
                        .receiver()
                        .latest_frame()
                        .is_some_and(|slot| this.video_surface.update_from_slot(slot));
                    let mut snapshot = this.runtime.snapshot();
                    let previous_page = this.page;
                    // REQ-PICOO-UI-008: Windows tray message/tip pump.
                    #[cfg(all(windows, feature = "windows-vcam"))]
                    {
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
                        this.vcam_status = VirtualCameraStatus::Active;
                        this.runtime
                            .set_virtual_camera_status(VirtualCameraStatus::Active);
                        snapshot.virtual_camera = VirtualCameraStatus::Active;
                    }

                    let pairing_request = snapshot.pairing_short_code.as_ref().and_then(|code| {
                        if !matches!(snapshot.status, ReceiverStatus::Pairing)
                            || this.pairing_dialog_code.as_ref() == Some(code)
                        {
                            return None;
                        }

                        let first_time = snapshot.trusted_device_count == 0
                            || snapshot
                                .active_sender
                                .as_ref()
                                .map(|sender| {
                                    !snapshot
                                        .trusted_devices
                                        .iter()
                                        .any(|device| device.device_id == sender.sender_id)
                                })
                                .unwrap_or(true);
                        let sender_name = snapshot
                            .active_sender
                            .as_ref()
                            .map(|sender| sender.device_name.clone())
                            .unwrap_or_else(|| "手机".into());
                        let ttl = this
                            .runtime
                            .receiver()
                            .pairing_ttl_remaining()
                            .map(|duration| duration.as_secs())
                            .unwrap_or(0);

                        this.pairing_dialog_code = Some(code.clone());
                        this.pairing_dialog_visible = true;
                        Some((code.clone(), sender_name, first_time, ttl))
                    });

                    if let Some((code, sender_name, first_time, ttl)) = pairing_request {
                        let app = cx.entity().downgrade();
                        let window_handle = this.window_handle;
                        cx.spawn(async move |_, cx| {
                            cx.background_executor()
                                .timer(Duration::from_millis(0))
                                .await;
                            let _ = window_handle.update(cx, move |_, window, cx| {
                                PicooDesktopApp::open_pairing_dialog(
                                    app,
                                    code,
                                    sender_name,
                                    first_time,
                                    ttl,
                                    window,
                                    cx,
                                );
                            });
                        })
                        .detach();
                    }

                    if !matches!(snapshot.status, ReceiverStatus::Pairing) {
                        this.pairing_dialog_code = None;
                        if this.pairing_dialog_visible {
                            this.pairing_dialog_visible = false;
                            let window_handle = this.window_handle;
                            cx.spawn(async move |_, cx| {
                                cx.background_executor()
                                    .timer(Duration::from_millis(0))
                                    .await;
                                let _ = window_handle
                                    .update(cx, |_, window, cx| window.close_dialog(cx));
                            })
                            .detach();
                        }
                    }
                    let snapshot_changed = snapshot != this.last_presented_snapshot;
                    if snapshot_changed {
                        this.last_presented_snapshot = snapshot;
                    }
                    if snapshot_changed || video_changed || this.page != previous_page {
                        cx.notify();
                    }
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
    fn render(&mut self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        // Pump loop is started once after window open (not here) to avoid
        // re-entrant Entity updates / RefCell panics during render.
        let snapshot = self.snapshot();
        let content = if self.page == DesktopPage::FirstLaunch {
            self.render_first_launch(cx).into_any_element()
        } else {
            div()
                .h_flex()
                .size_full()
                .min_w_0()
                .min_h_0()
                .overflow_hidden()
                .rounded(cx.theme().radius_lg)
                .border_1()
                .border_color(cx.theme().border)
                .child(self.render_sidebar(window, cx))
                .child(self.render_section(&snapshot, cx))
                .into_any_element()
        };

        div()
            .v_flex()
            .size_full()
            .bg(cx.theme().background)
            .text_color(cx.theme().foreground)
            .child(self.render_header(cx))
            .child(
                div()
                    .v_flex()
                    .w_full()
                    .flex_1()
                    .min_w_0()
                    .min_h_0()
                    .relative()
                    .when(self.page != DesktopPage::FirstLaunch, |this| this.p_5())
                    .child(content),
            )
    }
}

impl PicooDesktopApp {
    fn render_header(&self, cx: &Context<Self>) -> impl IntoElement {
        let is_macos = cfg!(target_os = "macos");

        TitleBar::new()
            .h_12()
            .border_b_1()
            .border_color(cx.theme().border)
            .bg(cx.theme().sidebar)
            .child(
                div()
                    .h_flex()
                    .h_full()
                    .w_full()
                    .items_center()
                    .when(!is_macos, |this| {
                        this.child(self.render_header_brand(true, cx))
                    })
                    .child(div().flex_1()),
            )
    }

    fn render_header_brand(&self, show_title: bool, cx: &Context<Self>) -> impl IntoElement {
        div()
            .h_flex()
            .items_center()
            .gap_3()
            .child(
                div().size_6().flex().items_center().justify_center().child(
                    reicon_svg(
                        include_bytes!("../../../assets/icons/reicon/camera.svg"),
                        cx.theme().primary,
                    )
                    .size_5(),
                ),
            )
            .when(show_title, |this| {
                this.child(
                    div()
                        .text_sm()
                        .font_weight(FontWeight::SEMIBOLD)
                        .text_color(cx.theme().sidebar_foreground)
                        .child("Picoo Camera"),
                )
            })
    }

    fn render_sidebar(&self, window: &mut Window, cx: &mut Context<Self>) -> gpui::AnyElement {
        let collapsed = self.sidebar_collapsed;
        let target_width = if collapsed {
            SIDEBAR_COLLAPSED_WIDTH
        } else {
            SIDEBAR_EXPANDED_WIDTH
        };
        let sidebar = div()
            .v_flex()
            .w(target_width)
            .h_full()
            .flex_shrink_0()
            .justify_between()
            .when(collapsed, |this| this.px_2())
            .when(!collapsed, |this| this.px_3())
            .py_3()
            .border_r_1()
            .border_color(cx.theme().border)
            .bg(cx.theme().sidebar)
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

        let transition = window.use_keyed_state("picoo-sidebar-width-transition", cx, |_, _| {
            SidebarWidthTransition::new(target_width)
        });
        if transition.read(cx).target_width != target_width {
            transition.update(cx, |transition, _| {
                transition.update_target(target_width);
            });
        }
        let transition = *transition.read(cx);
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

    fn sidebar_toggle_button(&self, cx: &Context<Self>) -> impl IntoElement {
        let collapsed = self.sidebar_collapsed;
        let label = if collapsed {
            "展开侧边栏"
        } else {
            "折叠侧边栏"
        };

        Button::new("toggle-sidebar")
            .ghost()
            .small()
            .tooltip(label)
            .accessibility_label(label)
            .icon(
                Icon::new(if collapsed {
                    IconName::PanelLeftOpen
                } else {
                    IconName::PanelLeftClose
                })
                .size_4(),
            )
            .on_click(cx.listener(|this, _, _, cx| {
                this.sidebar_collapsed = !this.sidebar_collapsed;
                cx.notify();
            }))
    }

    fn theme_button(&self, cx: &Context<Self>) -> impl IntoElement {
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

    fn nav_button(
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

    fn render_section(&self, snapshot: &ReceiverSnapshot, cx: &Context<Self>) -> gpui::AnyElement {
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
                    .child(self.render_workspace_toolbar(cx))
                    .child(page),
            )
            .into_any_element()
    }

    fn render_workspace_toolbar(&self, cx: &Context<Self>) -> impl IntoElement {
        div()
            .h_flex()
            .items_center()
            .gap_3()
            .flex_shrink_0()
            .child(self.sidebar_toggle_button(cx))
            .child(div().h_4().border_l_1().border_color(cx.theme().border))
            .child(
                div()
                    .min_w_0()
                    .text_sm()
                    .font_weight(FontWeight::SEMIBOLD)
                    .text_color(cx.theme().muted_foreground)
                    .child(self.section.label()),
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
            .child(
                div()
                    .text_2xl()
                    .font_weight(FontWeight::BOLD)
                    .child("Picoo Camera"),
            )
            .child("把手机变成电脑无线摄像头")
            .child(format!(
                "虚拟摄像头状态：{}",
                vcam_label_zh(self.vcam_status)
            ))
            .child(vcam_repair_hint(self.vcam_status))
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
                    .label(self.vcam_setup_button_label())
                    .loading(self.vcam_setup_state.is_running())
                    .on_click(cx.listener(|this, _, _, cx| {
                        this.try_register_vcam(cx);
                    })),
            )
            .children(self.render_vcam_setup_feedback(cx))
            .children(self.diagnostics_message.as_ref().map(|msg| {
                div()
                    .text_sm()
                    .text_color(cx.theme().success)
                    .child(msg.clone())
                    .into_any_element()
            }))
            .children(self.diagnostics_error.as_ref().map(|err| {
                div()
                    .text_sm()
                    .text_color(cx.theme().danger)
                    .max_w_96()
                    .text_center()
                    .child(err.clone())
                    .into_any_element()
            }))
            .child(
                Button::new("continue-first-launch")
                    .primary()
                    .label("继续")
                    .on_click(cx.listener(|this, _, _, cx| {
                        this.complete_first_launch(cx);
                    })),
            )
    }

    fn render_connect(&self, snapshot: &ReceiverSnapshot, cx: &Context<Self>) -> impl IntoElement {
        // REQ-PICOO-UI-0001 / AC-D-LAYOUT-01: distribute all available
        // workspace width after the gap, instead of capping the page at 1160 px.
        div()
            .h_flex()
            .size_full()
            .min_h_0()
            .items_stretch()
            .gap_5()
            .child(
                div()
                    .v_flex()
                    .h_full()
                    .min_w_0()
                    .flex_basis(rems(0.))
                    .flex_grow(58.)
                    .flex_shrink_1()
                    .child(if self.page == DesktopPage::Live {
                        self.render_live(snapshot, cx).into_any_element()
                    } else {
                        self.render_waiting(snapshot, cx).into_any_element()
                    }),
            )
            .child(
                div()
                    .v_flex()
                    .h_full()
                    .min_w_0()
                    .flex_basis(rems(0.))
                    .flex_grow(42.)
                    .flex_shrink_1()
                    .justify_between()
                    .gap_4()
                    .child(self.render_device_connection_card(snapshot, cx))
                    .child(self.render_network_status_card(snapshot, cx)),
            )
    }

    fn render_waiting(&self, snapshot: &ReceiverSnapshot, cx: &Context<Self>) -> impl IntoElement {
        let vcam_ready = matches!(
            snapshot.virtual_camera,
            VirtualCameraStatus::Installed | VirtualCameraStatus::Active
        );
        let pairing_code = snapshot
            .pairing_short_code
            .as_deref()
            .map(format_pairing_code)
            .unwrap_or_else(|| "等待请求".into());

        div()
            .v_flex()
            .flex_1()
            .h_full()
            .min_h(rems(35.))
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
                            .child(status_badge(
                                if vcam_ready {
                                    "接收端已就绪"
                                } else {
                                    "需要修复"
                                },
                                vcam_ready,
                                cx,
                            )),
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
            .child(
                div()
                    .h_flex()
                    .items_center()
                    .justify_between()
                    .gap_3()
                    .pt_3()
                    .border_t_1()
                    .border_color(cx.theme().border)
                    .text_xs()
                    .text_color(cx.theme().muted_foreground)
                    .child(
                        div()
                            .h_flex()
                            .items_center()
                            .gap_2()
                            .child(
                                div()
                                    .size_2()
                                    .flex_none()
                                    .rounded_full()
                                    .bg(cx.theme().success),
                            )
                            .child(format!("局域网监听中 · {DEFAULT_QUIC_PORT}")),
                    )
                    .child(
                        div()
                            .h_flex()
                            .items_center()
                            .gap_1p5()
                            .flex_none()
                            .child(
                                reicon_named("shield-check", cx.theme().primary).size(rems(0.875)),
                            )
                            .child("安全直连已启用"),
                    ),
            )
    }

    fn render_manual_endpoint_card(
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

    fn render_device_connection_card(
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
                            .border_color(cx.theme().border)
                            .bg(cx.theme().secondary)
                            .font_family(cx.theme().mono_font_family.clone())
                            .text_xs()
                            .text_color(cx.theme().muted_foreground)
                            .child(if streaming {
                                "实时推流"
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
                    .selected(remote_mirrored)
                    .accessibility_label("镜像翻转")
                    .child(reicon_button_content(
                        "镜像翻转",
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
                        .gap_3()
                        .child(
                            div()
                                .h_flex()
                                .justify_between()
                                .items_center()
                                .p_3()
                                .rounded(cx.theme().radius)
                                .border_1()
                                .border_color(cx.theme().primary.opacity(0.45))
                                .bg(cx.theme().secondary)
                                .child(
                                    div()
                                        .v_flex()
                                        .gap_0p5()
                                        .child(
                                            div()
                                                .text_sm()
                                                .font_weight(FontWeight::SEMIBOLD)
                                                .child(sender_name),
                                        )
                                        .child(
                                            div()
                                                .text_xs()
                                                .text_color(cx.theme().success)
                                                .child("● 实时推流中"),
                                        ),
                                )
                                .child(
                                    Button::new("disconnect-active-device")
                                        .danger()
                                        .small()
                                        .accessibility_label("断开")
                                        .child(reicon_button_content(
                                            "断开",
                                            "xmark",
                                            cx.theme().danger_foreground,
                                        ))
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
                                    "视频规格",
                                    snapshot
                                        .stream_config
                                        .as_ref()
                                        .map(|config| {
                                            format!("{}p · {} FPS", config.height, config.fps)
                                        })
                                        .unwrap_or_else(|| "—".into()),
                                    cx,
                                ))
                                .child(metric_row("接收码率", format!("{bitrate:.1} Mbps"), cx))
                                .child(metric_row(
                                    "RTT / 抖动",
                                    format!(
                                        "{:.0} ms · {:.1} ms",
                                        snapshot.stream_metrics.latency_ms, snapshot.link_jitter_ms
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
                                        .gap_2()
                                        .child(mirror_button)
                                        .child(
                                            Button::new("switch-camera-card")
                                                .outline()
                                                .small()
                                                .flex_1()
                                                .accessibility_label("镜头切换")
                                                .child(reicon_button_content(
                                                    "镜头切换",
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
                                                .accessibility_label("画面修复")
                                                .child(reicon_button_content(
                                                    "画面修复",
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
                                        .gap_3()
                                        .child(
                                            div()
                                                .size_7()
                                                .flex()
                                                .flex_none()
                                                .items_center()
                                                .justify_center()
                                                .rounded(cx.theme().radius)
                                                .border_1()
                                                .border_color(cx.theme().border)
                                                .bg(cx.theme().group_box)
                                                .child(
                                                    reicon_named("iphone", cx.theme().primary)
                                                        .size(rems(1.)),
                                                ),
                                        )
                                        .child(
                                            div()
                                                .v_flex()
                                                .min_w_0()
                                                .gap_0p5()
                                                .child(
                                                    div()
                                                        .text_sm()
                                                        .font_weight(FontWeight::SEMIBOLD)
                                                        .truncate()
                                                        .child(device.device_name.clone()),
                                                )
                                                .child(
                                                    div()
                                                        .text_xs()
                                                        .text_color(cx.theme().muted_foreground)
                                                        .child(format!(
                                                            "{} · 最近连接 {} · 等待手机接入",
                                                            device.platform,
                                                            crate::receiver_runtime::format_last_connected_ms(
                                                                device.last_connected_at_ms,
                                                            )
                                                        )),
                                                ),
                                        ),
                                )
                                .child(
                                    div()
                                        .h_flex()
                                        .items_center()
                                        .gap_1()
                                        .child(status_badge("已信任", true, cx))
                                        .child(
                                            Button::new(format!(
                                                "manage-trusted-{}",
                                                device.device_id
                                            ))
                                            .ghost()
                                            .xsmall()
                                            .accessibility_label("管理可信设备")
                                            .child(
                                                reicon_named(
                                                    "more-horizontal",
                                                    cx.theme().muted_foreground,
                                                )
                                                .size(rems(0.875)),
                                            )
                                            .on_click(cx.listener(move |_, _, window, cx| {
                                                PicooDesktopApp::open_remove_trusted_dialog(
                                                    cx.entity().downgrade(),
                                                    device_id.clone(),
                                                    device_name.clone(),
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

    fn render_network_status_card(
        &self,
        snapshot: &ReceiverSnapshot,
        cx: &Context<Self>,
    ) -> impl IntoElement {
        let network_ready = snapshot.bind_addr.is_some()
            && !snapshot.advertise_host.is_empty()
            && snapshot.advertise_host != "127.0.0.1";
        let connected = !matches!(
            snapshot.status,
            ReceiverStatus::Disconnected | ReceiverStatus::Discovering
        );
        let latency = if snapshot.stream_metrics.latency_ms <= 30.0 {
            "低"
        } else if snapshot.stream_metrics.latency_ms <= 80.0 {
            "一般"
        } else {
            "较高"
        };
        div()
            .v_flex()
            .gap_3()
            .p_5()
            .rounded(cx.theme().radius_lg)
            .border_1()
            .border_color(cx.theme().border)
            .bg(cx.theme().group_box)
            .when(cx.theme().shadow, |this| this.shadow_lg())
            .child(
                div()
                    .h_flex()
                    .gap_2()
                    .child(reicon_named("activity", cx.theme().primary))
                    .child(
                        div()
                            .text_sm()
                            .font_weight(FontWeight::SEMIBOLD)
                            .child("网络状态"),
                    ),
            )
            .child(network_status_row(
                "wifi",
                "网络",
                if network_ready {
                    "局域网可用"
                } else {
                    "未检测到局域网"
                },
                network_ready,
                cx,
            ))
            .child(network_status_row(
                "server",
                "发现服务",
                if snapshot.discovery_available {
                    "在线"
                } else {
                    "不可用"
                },
                snapshot.discovery_available,
                cx,
            ))
            .child(network_status_row(
                "activity",
                "延迟",
                latency,
                latency != "较高",
                cx,
            ))
            .child(network_status_row(
                "shield",
                "安全",
                if connected {
                    "已保护"
                } else {
                    "等待连接"
                },
                connected,
                cx,
            ))
    }

    fn open_pairing_dialog(
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
                    move |_, _, cx| {
                        let _ = confirm_app.update(cx, |this, cx| {
                            this.runtime.confirm_pairing();
                            cx.notify();
                        });
                        true
                    }
                })
                .on_cancel({
                    let cancel_app = cancel_app.clone();
                    move |_, _, cx| {
                        let _ = cancel_app.update(cx, |this, cx| {
                            this.runtime.disconnect();
                            cx.notify();
                        });
                        true
                    }
                })
                .on_close({
                    let close_app = close_app.clone();
                    move |_, _, cx| {
                        let _ = close_app.update(cx, |this, cx| {
                            this.pairing_dialog_visible = false;
                            cx.notify();
                        });
                    }
                })
        });
    }

    fn open_disconnect_dialog(
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

    fn open_remove_trusted_dialog(
        app: WeakEntity<Self>,
        device_id: String,
        device_name: String,
        window: &mut Window,
        cx: &mut App,
    ) {
        window.open_alert_dialog(cx, move |alert, _, _| {
            let app = app.clone();
            let device_id = device_id.clone();
            alert
                .title(format!("删除“{device_name}”？"))
                .description("此设备下次连接时必须重新核对配对短码。")
                .button_props(
                    DialogButtonProps::default()
                        .ok_text("删除")
                        .ok_variant(ButtonVariant::Danger)
                        .cancel_text("取消")
                        .show_cancel(true),
                )
                .on_ok(move |_, _, cx| {
                    let _ = app.update(cx, |this, cx| {
                        match this.runtime.remove_trusted_device(&device_id) {
                            Ok(true) => {
                                this.diagnostics_error = None;
                                this.diagnostics_message = Some(format!("已删除配对：{device_id}"));
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
                    });
                    true
                })
        });
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
        let res_label = snapshot
            .stream_config
            .as_ref()
            .map(|config| format!("{}p · {} FPS", config.height, config.fps))
            .unwrap_or_else(|| "—".into());
        let sender_name = snapshot
            .active_sender
            .as_ref()
            .map(|sender| sender.device_name.clone())
            .unwrap_or_else(|| "手机摄像头".into());
        let quality = crate::network_quality::network_quality_label(
            snapshot.stream_metrics.packet_loss,
            snapshot.stream_metrics.latency_ms,
        );

        div()
            .v_flex()
            .w_full()
            .overflow_hidden()
            .rounded(cx.theme().radius_lg)
            .border_1()
            .border_color(cx.theme().border)
            .bg(cx.theme().group_box)
            .when(cx.theme().shadow, |this| this.shadow_lg())
            .child(
                div().p_4().child(
                    div()
                        // REQ-PICOO-UI-0001 / AC-D-TECH-02: the preview surface,
                        // including its empty state, always occupies a 16:9 viewport.
                        .w_full()
                        .aspect_ratio(16. / 9.)
                        .flex_none()
                        .relative()
                        .rounded(cx.theme().radius)
                        .bg(cx.theme().muted)
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
                                .child(live_hud_pill(format!("● {sender_name} · {res_label}"), cx))
                                .child(live_hud_pill(
                                    format!(
                                        "虚拟摄像头 · {}",
                                        vcam_label_zh(snapshot.virtual_camera)
                                    ),
                                    cx,
                                )),
                        ),
                ),
            )
            .child(
                div()
                    .h_flex()
                    .justify_between()
                    .items_center()
                    .p_4()
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
                                    .child("实时视频监视器"),
                            )
                            .child(
                                div()
                                    .text_xs()
                                    .text_color(cx.theme().muted_foreground)
                                    .child(format!("{res_label} · 网络{quality}")),
                            ),
                    )
                    .child(
                        div()
                            .font_family(cx.theme().mono_font_family.clone())
                            .text_xs()
                            .text_color(cx.theme().success)
                            .child("● LIVE"),
                    ),
            )
    }

    fn render_virtual_camera_page(
        &self,
        snapshot: &ReceiverSnapshot,
        cx: &Context<Self>,
    ) -> impl IntoElement {
        div()
            .v_flex()
            .max_w(rems(55.))
            .mx_auto()
            .gap_5()
            .child(page_header(
                "monitor",
                "虚拟摄像头",
                "管理系统虚拟摄像头和无视频流时的输出画面",
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
                    .child(
                        div()
                            .h_flex()
                            .items_center()
                            .justify_between()
                            .child(
                                div()
                                    .h_flex()
                                    .gap_3()
                                    .child(reicon_named("monitor", cx.theme().primary))
                                    .child(
                                        div()
                                            .v_flex()
                                            .gap_0p5()
                                            .child(
                                                div()
                                                    .font_weight(FontWeight::SEMIBOLD)
                                                    .child("Picoo Camera"),
                                            )
                                            .child(
                                                div()
                                                    .text_xs()
                                                    .text_color(cx.theme().muted_foreground)
                                                    .child("系统虚拟摄像头设备"),
                                            ),
                                    ),
                            )
                            .child(status_badge(
                                vcam_label_zh(snapshot.virtual_camera),
                                matches!(
                                    snapshot.virtual_camera,
                                    VirtualCameraStatus::Installed | VirtualCameraStatus::Active
                                ),
                                cx,
                            )),
                    )
                    .child(
                        div()
                            .p_4()
                            .rounded(cx.theme().radius)
                            .bg(cx.theme().secondary.opacity(0.45))
                            .text_sm()
                            .text_color(cx.theme().muted_foreground)
                            .child(vcam_repair_hint(snapshot.virtual_camera)),
                    )
                    .child(match &snapshot.shared_ring_error {
                        Some(err) => {
                            status_row("Shared Frame Ring", format!("附着失败：{err}"), false, cx)
                        }
                        None => status_row("Shared Frame Ring", "已附着".to_string(), true, cx),
                    })
                    .child(
                        div()
                            .h_flex()
                            .gap_2()
                            .child(
                                Button::new("refresh-vcam-page")
                                    .outline()
                                    .label("重新检测")
                                    .on_click(cx.listener(|this, _, _, cx| {
                                        this.refresh_vcam_status();
                                        cx.notify();
                                    })),
                            )
                            .child(
                                Button::new("repair-vcam-page")
                                    .label(self.vcam_setup_button_label())
                                    .loading(self.vcam_setup_state.is_running())
                                    .on_click(cx.listener(|this, _, _, cx| {
                                        this.try_register_vcam(cx);
                                    })),
                            ),
                    )
                    .children(self.render_vcam_setup_feedback(cx)),
            )
            .child(section_header("camera", "无视频流画面", cx))
            .child(
                div()
                    .v_flex()
                    .gap_3()
                    .child(
                        div()
                            .text_xs()
                            .text_color(cx.theme().muted_foreground)
                            .child("手机未连接或视频中断时显示的内容"),
                    )
                    .child(div().h_flex().gap_4().children(
                        crate::prefs::PlaceholderModePref::ALL.iter().map(|mode| {
                            let selected = self.prefs.placeholder_mode == *mode;
                            let mode = *mode;
                            div()
                                .id(format!("placeholder-{mode:?}"))
                                .flex_1()
                                .min_w_0()
                                .v_flex()
                                .gap_2()
                                .p_3()
                                .rounded(cx.theme().radius)
                                .border_1()
                                .border_color(if selected {
                                    cx.theme().primary
                                } else {
                                    cx.theme().border
                                })
                                .bg(cx.theme().group_box)
                                .cursor_pointer()
                                .child(
                                    div()
                                        .v_flex()
                                        .w_full()
                                        .gap_2()
                                        .child(placeholder_preview(mode, cx))
                                        .child(
                                            div()
                                                .h_flex()
                                                .w_full()
                                                .justify_between()
                                                .text_xs()
                                                .font_weight(FontWeight::MEDIUM)
                                                .child(placeholder_title(mode))
                                                .child(placeholder_choice_indicator(selected, cx)),
                                        ),
                                )
                                .on_click(cx.listener(move |this, _, _, cx| {
                                    this.prefs.placeholder_mode = mode;
                                    this.runtime.set_placeholder_mode(mode.to_frame_hub());
                                    let _ = this.persist_prefs();
                                    cx.notify();
                                }))
                                .into_any_element()
                        }),
                    )),
            )
    }

    fn render_network_page(
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

    fn render_general_page(
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

    fn render_help_page(&self, cx: &Context<Self>) -> impl IntoElement {
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
                                Button::new("export-diagnostics")
                                    .outline()
                                    .label("导出")
                                    .on_click(cx.listener(|this, _, _, cx| {
                                        this.export_diagnostics(cx);
                                    })),
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

    fn render_about_page(&self, cx: &Context<Self>) -> impl IntoElement {
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
                        include_bytes!("../../../assets/icons/reicon/camera.svg"),
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

    fn render_settings(&self, cx: &Context<Self>) -> impl IntoElement {
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

fn vcam_label_zh(status: VirtualCameraStatus) -> &'static str {
    match status {
        VirtualCameraStatus::Unknown => "检测中",
        VirtualCameraStatus::Bundled => "已随附 · 待激活",
        VirtualCameraStatus::Installed => "就绪 (Ready)",
        VirtualCameraStatus::NotInstalled => "未安装 (Not Installed)",
        VirtualCameraStatus::Active => "就绪 (Active)",
    }
}

fn endpoint_label(snapshot: &ReceiverSnapshot) -> String {
    if snapshot.advertise_host.is_empty() {
        return "—".into();
    }
    format!("{}:{DEFAULT_QUIC_PORT}", snapshot.advertise_host)
}

fn reicon_svg(data: &'static [u8], color: Hsla) -> Svg {
    svg().data(data).size_4().text_color(color)
}

fn reicon_named(name: &str, color: Hsla) -> Svg {
    let data: &'static [u8] = match name {
        "activity" => include_bytes!("../../../assets/icons/reicon/activity.svg"),
        "camera" => include_bytes!("../../../assets/icons/reicon/camera.svg"),
        "camera-rotate" => include_bytes!("../../../assets/icons/reicon/camera_rotate.svg"),
        "check-circle-filled" => {
            include_bytes!("../../../assets/icons/reicon/check_circle_filled.svg")
        }
        "copy" => include_bytes!("../../../assets/icons/reicon/copy.svg"),
        "desktop" => include_bytes!("../../../assets/icons/reicon/desktop.svg"),
        "help" => include_bytes!("../../../assets/icons/reicon/help.svg"),
        "help-circle" => include_bytes!("../../../assets/icons/reicon/help_circle.svg"),
        "home" => include_bytes!("../../../assets/icons/reicon/home.svg"),
        "info" => include_bytes!("../../../assets/icons/reicon/info.svg"),
        "iphone" => include_bytes!("../../../assets/icons/reicon/iphone.svg"),
        "key" => include_bytes!("../../../assets/icons/reicon/key.svg"),
        "flip-horizontal" => include_bytes!("../../../assets/icons/reicon/flip_horizontal.svg"),
        "mobile" => include_bytes!("../../../assets/icons/reicon/mobile.svg"),
        "monitor" => include_bytes!("../../../assets/icons/reicon/monitor.svg"),
        "monitor-camera" => {
            include_bytes!("../../../assets/icons/reicon/monitor_camera.svg")
        }
        "monitor-phone" => include_bytes!("../../../assets/icons/reicon/monitor_phone.svg"),
        "moon" => include_bytes!("../../../assets/icons/reicon/moon.svg"),
        "more-horizontal" => include_bytes!("../../../assets/icons/reicon/more_horizontal.svg"),
        "play-filled" => include_bytes!("../../../assets/icons/reicon/play_filled.svg"),
        "radio" => include_bytes!("../../../assets/icons/reicon/radio.svg"),
        "refresh" => include_bytes!("../../../assets/icons/reicon/refresh.svg"),
        "rocket" => include_bytes!("../../../assets/icons/reicon/rocket.svg"),
        "server" => include_bytes!("../../../assets/icons/reicon/server.svg"),
        "settings" => include_bytes!("../../../assets/icons/reicon/settings.svg"),
        "sidebar" => include_bytes!("../../../assets/icons/reicon/sidebar.svg"),
        "shield" => include_bytes!("../../../assets/icons/reicon/shield.svg"),
        "shield-check" => include_bytes!("../../../assets/icons/reicon/shield_check.svg"),
        "sun" => include_bytes!("../../../assets/icons/reicon/sun.svg"),
        "tuning" => include_bytes!("../../../assets/icons/reicon/tuning.svg"),
        "wifi" => include_bytes!("../../../assets/icons/reicon/wifi.svg"),
        "xmark" => include_bytes!("../../../assets/icons/reicon/xmark.svg"),
        _ => include_bytes!("../../../assets/icons/reicon/info.svg"),
    };
    reicon_svg(data, color)
}

fn reicon_button_content(label: &'static str, icon: &'static str, color: Hsla) -> impl IntoElement {
    div()
        .h_flex()
        .gap_2()
        .child(reicon_named(icon, color))
        .child(label)
}

fn hardware_topology(cx: &Context<PicooDesktopApp>) -> impl IntoElement {
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
                        .v_flex()
                        .items_center()
                        .justify_center()
                        .gap_0p5()
                        .overflow_hidden()
                        .rounded(rems(0.36))
                        .bg(cx.theme().group_box)
                        .text_color(cx.theme().foreground)
                        .child(
                            div()
                                .text_size(rems(0.56))
                                .font_weight(FontWeight::EXTRA_BOLD)
                                .text_color(cx.theme().primary)
                                .child("?"),
                        )
                        .child(
                            div()
                                .text_size(rems(0.42))
                                .font_weight(FontWeight::BOLD)
                                .child("PICO"),
                        ),
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

fn live_hud_pill(label: String, cx: &Context<PicooDesktopApp>) -> impl IntoElement {
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

fn page_header(
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

fn section_header(
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

fn placeholder_title(mode: crate::prefs::PlaceholderModePref) -> &'static str {
    mode.label()
}

fn placeholder_preview(
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

fn placeholder_choice_indicator(selected: bool, cx: &Context<PicooDesktopApp>) -> AnyElement {
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

fn settings_toggle_row(
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

fn network_detail_row(
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

fn status_badge(
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

fn onboarding_step(
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

fn network_status_row(
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

fn metric_row(
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

fn status_row(
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

fn connection_code_hero(code: &str, cx: &Context<PicooDesktopApp>) -> gpui::AnyElement {
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

fn pairing_code_panel(code: &str, ttl_label: String, cx: &App) -> gpui::AnyElement {
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

fn pairing_code_box(digit: char, cx: &App) -> gpui::AnyElement {
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
    std::env::set_var("RUST_LOG", prefs.log_level.env_filter());
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
    let (vcam_status, _vcam_registration) = if should_auto_start_vcam(vcam_status) {
        match crate::vcam_register::start_registered_on_worker() {
            Ok(reg) => {
                tracing::info!("Picoo Camera virtual camera started for this session");
                runtime.set_virtual_camera_status(VirtualCameraStatus::Active);
                (VirtualCameraStatus::Active, Some(reg))
            }
            Err(err) => {
                tracing::warn!(
                    "MF virtual camera start deferred: {err} (open 虚拟摄像头 → 安装或修复…)"
                );
                (vcam_status, None)
            }
        }
    } else {
        tracing::info!(
            "virtual camera is not installed; skip privileged startup repair and keep the explicit repair action available"
        );
        (vcam_status, None)
    };

    let app = gpui_platform::application().with_assets(PicooAssets);
    let prefs_for_window = prefs.clone();
    app.run(move |cx| {
        gpui_component::init(cx);
        crate::picoo_theme::install(cx);
        cx.set_app_identity("com.picoo.camera", "Picoo Camera");
        cx.activate(true);

        cx.open_window(
            WindowOptions {
                window_bounds: Some(WindowBounds::Windowed(Bounds {
                    origin: point(px(100.), px(100.)),
                    size: size(px(1920.), px(1080.)),
                })),
                window_min_size: Some(size(px(1180.), px(720.))),
                ..TitleBar::window_options()
            },
            move |window, cx| {
                let window_handle = window.window_handle();
                let view = cx.new(|cx| {
                    PicooDesktopApp::new(
                        runtime,
                        prefs_for_window,
                        vcam_status,
                        window_handle,
                        window,
                        cx,
                    )
                });
                // Start frame/tray pump after the view exists — not inside Render.
                view.update(cx, |this, cx| {
                    this.ensure_pump_loop(cx);
                });
                // REQ-PICOO-UI-008: Windows closes to tray when enabled; macOS
                // keeps the app in Dock/background without a fake tray icon.
                let tray_view = view.clone();
                window.on_window_should_close(cx, move |window, cx| {
                    let outcome = tray_view.read(cx).close_outcome();
                    if outcome.hide_to_background {
                        #[cfg(all(windows, feature = "windows-vcam"))]
                        {
                            let status = tray_view.read(cx).runtime.snapshot().status;
                            let tip = crate::tray::tip_for_status(status);
                            crate::tray::note_hidden_to_tray_with_tip(&tip);
                        }
                        // App-level hide keeps the process; minimize covers hosts
                        // where hide() is a no-op.
                        cx.hide();
                        window.minimize_window();
                        return false;
                    }
                    #[cfg(all(windows, feature = "windows-vcam"))]
                    crate::tray::note_tray_cleared();
                    outcome.allow_close
                });
                #[cfg(all(windows, feature = "windows-vcam"))]
                crate::tray::pump_win32_tray_messages();
                cx.new(|cx| Root::new(view, window, cx).bg(cx.theme().background))
            },
        )
        .expect("open window");
    });

    Ok(())
}

#[cfg(any(test, all(windows, feature = "windows-vcam")))]
fn should_auto_start_vcam(status: VirtualCameraStatus) -> bool {
    matches!(
        status,
        VirtualCameraStatus::Installed | VirtualCameraStatus::Active
    )
}

#[cfg(test)]
mod tests {
    use super::{
        should_auto_start_vcam, PicooAssets, SidebarWidthTransition, SIDEBAR_COLLAPSED_WIDTH,
        SIDEBAR_EXPANDED_WIDTH,
    };
    use crate::model::VirtualCameraStatus;
    use gpui::AssetSource;

    #[test]
    fn only_an_installed_virtual_camera_is_started_automatically() {
        assert!(should_auto_start_vcam(VirtualCameraStatus::Installed));
        assert!(should_auto_start_vcam(VirtualCameraStatus::Active));
        assert!(!should_auto_start_vcam(VirtualCameraStatus::NotInstalled));
        assert!(!should_auto_start_vcam(VirtualCameraStatus::Bundled));
        assert!(!should_auto_start_vcam(VirtualCameraStatus::Unknown));
    }

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

    #[test]
    fn hardware_topology_svgs_are_embedded_with_intrinsic_dimensions() {
        for path in [
            "device-frames/iphone-16-max.svg",
            "device-frames/macbook-pro-light.svg",
            "device-frames/macbook-pro-dark.svg",
        ] {
            let asset = PicooAssets
                .load(path)
                .expect("device asset lookup should succeed")
                .expect("device asset should be embedded");
            let svg = std::str::from_utf8(&asset).expect("device asset should be UTF-8 SVG");
            assert!(svg.contains("<svg width=\""));
            assert!(svg.contains(" height=\""));
            assert!(svg.contains(" viewBox=\""));
        }

        let iphone = PicooAssets
            .load("device-frames/iphone-16-max.svg")
            .expect("iPhone asset lookup should succeed")
            .expect("iPhone asset should be embedded");
        let iphone = std::str::from_utf8(&iphone).expect("iPhone asset should be UTF-8 SVG");
        assert!(iphone.contains("<linearGradient"));
        assert!(iphone.contains("<clipPath"));
        assert!(iphone.contains("<feGaussianBlur"));
        assert!(!iphone.contains("<lineargradient"));
        assert!(!iphone.contains("<clippath"));
        assert!(iphone.contains("M415 343"), "right-side control is missing");
        assert!(iphone.contains("M0 151"), "left-side controls are missing");
    }

    #[test]
    fn desktop_semantic_reicons_are_valid_svg_assets() {
        for (name, asset) in [
            (
                "activity",
                include_bytes!("../../../assets/icons/reicon/activity.svg").as_slice(),
            ),
            (
                "camera",
                include_bytes!("../../../assets/icons/reicon/camera.svg").as_slice(),
            ),
            (
                "check-circle-filled",
                include_bytes!("../../../assets/icons/reicon/check_circle_filled.svg").as_slice(),
            ),
            (
                "help-circle",
                include_bytes!("../../../assets/icons/reicon/help_circle.svg").as_slice(),
            ),
            (
                "iphone",
                include_bytes!("../../../assets/icons/reicon/iphone.svg").as_slice(),
            ),
            (
                "key",
                include_bytes!("../../../assets/icons/reicon/key.svg").as_slice(),
            ),
            (
                "mobile",
                include_bytes!("../../../assets/icons/reicon/mobile.svg").as_slice(),
            ),
            (
                "monitor",
                include_bytes!("../../../assets/icons/reicon/monitor.svg").as_slice(),
            ),
            (
                "monitor-camera",
                include_bytes!("../../../assets/icons/reicon/monitor_camera.svg").as_slice(),
            ),
            (
                "monitor-phone",
                include_bytes!("../../../assets/icons/reicon/monitor_phone.svg").as_slice(),
            ),
            (
                "moon",
                include_bytes!("../../../assets/icons/reicon/moon.svg").as_slice(),
            ),
            (
                "more-horizontal",
                include_bytes!("../../../assets/icons/reicon/more_horizontal.svg").as_slice(),
            ),
            (
                "play-filled",
                include_bytes!("../../../assets/icons/reicon/play_filled.svg").as_slice(),
            ),
            (
                "radio",
                include_bytes!("../../../assets/icons/reicon/radio.svg").as_slice(),
            ),
            (
                "rocket",
                include_bytes!("../../../assets/icons/reicon/rocket.svg").as_slice(),
            ),
            (
                "server",
                include_bytes!("../../../assets/icons/reicon/server.svg").as_slice(),
            ),
            (
                "sidebar",
                include_bytes!("../../../assets/icons/reicon/sidebar.svg").as_slice(),
            ),
            (
                "shield",
                include_bytes!("../../../assets/icons/reicon/shield.svg").as_slice(),
            ),
            (
                "shield-check",
                include_bytes!("../../../assets/icons/reicon/shield_check.svg").as_slice(),
            ),
        ] {
            let svg =
                std::str::from_utf8(asset).unwrap_or_else(|_| panic!("{name} should be UTF-8 SVG"));
            assert!(
                svg.starts_with("<svg "),
                "{name} should start with an SVG root"
            );
            assert!(svg.contains("viewBox=\"0 0 24 24\""));
            assert!(svg.contains("currentColor"));
        }
    }
}
