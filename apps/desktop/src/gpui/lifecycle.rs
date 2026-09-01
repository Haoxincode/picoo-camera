use std::time::Duration;

use gpui::prelude::FluentBuilder;
use gpui::*;
use gpui_component::input::{InputEvent, InputState};
use gpui_component::*;
use picoo_session::ReceiverStatus;

use crate::model::VirtualCameraStatus;
use crate::prefs::save_prefs;
use crate::prefs::DesktopPreferences;
#[cfg(target_os = "macos")]
use crate::prefs::{
    current_macos_boot_session, MacosCameraExtensionIntent, PendingMacosCameraExtension,
};
use crate::receiver_runtime::{ReceiverRuntime, ReceiverSnapshot};
use crate::video_surface::VideoSurface;

use super::pages::DiagnosticsExportState;
use super::vcam::VcamSetupState;
use super::{DesktopPage, DesktopSection, PicooDesktopApp};

impl PicooDesktopApp {
    pub(super) fn new(
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
            diagnostics_export: DiagnosticsExportState::default(),
            window_handle,
            pairing_dialog_code: None,
            pairing_dialog_pending: None,
            pairing_dialog_visible: false,
            pairing_locally_confirmed: false,
        }
    }

    pub(super) fn snapshot(&self) -> ReceiverSnapshot {
        self.runtime.snapshot()
    }

    /// Close-button policy from settings (REQ-PICOO-UI-008).
    pub(super) fn close_outcome(&self) -> crate::tray::CloseOutcome {
        self.tray_policy.close_outcome()
    }

    pub(super) fn persist_prefs(&mut self) -> Result<(), String> {
        save_prefs(&self.prefs)
    }

    #[cfg(target_os = "macos")]
    pub(super) fn persist_pending_macos_vcam(
        &mut self,
        intent: MacosCameraExtensionIntent,
    ) -> Result<(), String> {
        let boot_session = current_macos_boot_session()?;
        self.prefs.pending_macos_camera_extension = Some(PendingMacosCameraExtension {
            intent,
            boot_session,
        });
        self.persist_prefs()
    }

    pub(super) fn apply_log_level(&self) {
        let filter = self.prefs.log_level.env_filter();
        std::env::set_var("RUST_LOG", filter);
        if let Err(err) = crate::logging::reload_filter(filter) {
            tracing::warn!("log level reload failed: {err}");
        }
    }

    pub(super) fn complete_first_launch(&mut self, cx: &mut Context<Self>) {
        self.prefs.first_launch_completed = true;
        let _ = self.persist_prefs();
        self.page = DesktopPage::Waiting;
        cx.notify();
    }

    pub(super) fn save_display_name(&mut self, cx: &mut Context<Self>) {
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

    pub(super) fn status_label(status: ReceiverStatus) -> &'static str {
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

    pub(super) fn ensure_pump_loop(&mut self, cx: &mut Context<Self>) {
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
                    let snapshot = this.runtime.snapshot();
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
                                    cx.update(|cx| cx.activate(true));
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
                    let pairing_request = snapshot.pairing_short_code.as_ref().and_then(|code| {
                        if !matches!(snapshot.status, ReceiverStatus::Pairing)
                            || this.pairing_dialog_code.as_ref() == Some(code)
                            || this.pairing_dialog_pending.as_ref() == Some(code)
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

                        this.pairing_dialog_pending = Some(code.clone());
                        this.pairing_locally_confirmed = false;
                        Some((code.clone(), sender_name, first_time, ttl))
                    });

                    if let Some((code, sender_name, first_time, ttl)) = pairing_request {
                        let app = cx.entity().downgrade();
                        let dialog_app = app.clone();
                        let dialog_code = code.clone();
                        let window_handle = this.window_handle;
                        cx.spawn(async move |_, cx| {
                            cx.background_executor()
                                .timer(Duration::from_millis(0))
                                .await;
                            let opened = window_handle
                                .update(cx, move |_, window, cx| {
                                    window.activate_window();
                                    PicooDesktopApp::open_pairing_dialog(
                                        dialog_app,
                                        dialog_code,
                                        sender_name,
                                        first_time,
                                        ttl,
                                        window,
                                        cx,
                                    );
                                })
                                .is_ok();
                            let _ = app.update(cx, move |this, cx| {
                                if this.pairing_dialog_pending.as_ref() == Some(&code) {
                                    this.pairing_dialog_pending = None;
                                }
                                if opened {
                                    this.pairing_dialog_code = Some(code);
                                    this.pairing_dialog_visible = true;
                                } else {
                                    tracing::warn!(
                                        "pairing dialog could not reach the desktop window; inline confirmation remains available"
                                    );
                                }
                                cx.notify();
                            });
                        })
                        .detach();
                    }

                    if !matches!(snapshot.status, ReceiverStatus::Pairing) {
                        this.pairing_dialog_code = None;
                        this.pairing_dialog_pending = None;
                        this.pairing_locally_confirmed = false;
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
            div()
                .v_flex()
                .size_full()
                .child(self.render_window_title_bar(cx))
                .child(
                    div()
                        .v_flex()
                        .flex_1()
                        .min_w_0()
                        .min_h_0()
                        .child(self.render_first_launch(cx)),
                )
                .into_any_element()
        } else {
            let workspace = div()
                .h_flex()
                .flex_1()
                .min_w_0()
                .min_h_0()
                .overflow_hidden()
                .child(self.render_sidebar(window, cx))
                .child(
                    div()
                        .v_flex()
                        .h_full()
                        .flex_1()
                        .min_w_0()
                        .min_h_0()
                        .child(self.render_workspace_toolbar(cx))
                        .child(self.render_section(&snapshot, cx)),
                );
            div()
                .v_flex()
                .size_full()
                .min_w_0()
                .min_h_0()
                .when(cfg!(target_os = "macos"), |this| {
                    this.child(self.render_window_title_bar(cx))
                })
                .child(workspace)
                .into_any_element()
        };

        div()
            .size_full()
            .bg(cx.theme().background)
            .text_color(cx.theme().foreground)
            .child(content)
    }
}
