use std::time::Duration;

use gpui_kit::component::input::{InputEvent, InputState};
use gpui_kit::component::*;
use gpui_kit::prelude::FluentBuilder;
use gpui_kit::*;
use picoo_session::ReceiverStatus;

use crate::model::VirtualCameraStatus;
use crate::prefs::save_prefs;
use crate::prefs::DesktopPreferences;
#[cfg(target_os = "macos")]
use crate::prefs::{
    current_macos_boot_session, MacosCameraExtensionIntent, PendingMacosCameraExtension,
};
use crate::preview_pipeline::PreviewPipeline;
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
        let mut preview_pipeline = PreviewPipeline::default();
        preview_pipeline.set_viewport_physical_width(
            window.viewport_size().width.as_f32() * window.scale_factor(),
        );
        let _subscriptions = vec![
            cx.subscribe_in(
                &display_name_input,
                window,
                |this, _, event: &InputEvent, _, cx| {
                    if matches!(event, InputEvent::Change) {
                        this.save_display_name(cx);
                    }
                },
            ),
            cx.observe_window_bounds(window, |this, window, _| {
                this.preview_pipeline.set_viewport_physical_width(
                    window.viewport_size().width.as_f32() * window.scale_factor(),
                );
            }),
        ];
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
            preview_pipeline,
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
            pairing_dialog: Default::default(),
            pairing_locally_confirmed: false,
            identity_replacement_dialog_revision: None,
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
                    if let Err(error) = this.runtime.pump() {
                        tracing::warn!(%error, "Receiver pump failed");
                    }
                    if let Some(slot) = this.runtime.receiver().latest_frame() {
                        this.preview_pipeline.submit_latest(slot);
                    }
                    let video_changed = this
                        .preview_pipeline
                        .take_prepared()
                        .is_some_and(|preview| this.video_surface.present(preview, cx));
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
                                    this.pairing_dialog.mark_opened();
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

                    // Present identity cleanup only after the pairing dialog has
                    // left the window. The candidate is already trusted; name
                    // matching never bypasses the short-code gate (PAIRING-006).
                    let identity_replacement_request = snapshot
                        .trusted_identity_replacement
                        .as_ref()
                        .filter(|_| !matches!(snapshot.status, ReceiverStatus::Pairing))
                        .filter(|_| !this.pairing_dialog.is_visible())
                        .filter(|replacement| {
                            this.identity_replacement_dialog_revision
                                != Some(replacement.revision)
                        })
                        .cloned();

                    if let Some(replacement) = identity_replacement_request {
                        let revision = replacement.revision;
                        this.identity_replacement_dialog_revision = Some(revision);
                        let app = cx.entity().downgrade();
                        let dialog_app = app.clone();
                        let window_handle = this.window_handle;
                        cx.spawn(async move |_, cx| {
                            cx.background_executor()
                                .timer(Duration::from_millis(0))
                                .await;
                            let opened = window_handle
                                .update(cx, move |_, window, cx| {
                                    if window.has_active_dialog(cx) {
                                        return false;
                                    }
                                    PicooDesktopApp::open_identity_replacement_dialog(
                                        dialog_app,
                                        replacement,
                                        window,
                                        cx,
                                    );
                                    true
                                })
                                .unwrap_or(false);
                            if !opened {
                                // Keep one retry reservation while another
                                // decision surface owns the window.
                                cx.background_executor()
                                    .timer(Duration::from_millis(250))
                                    .await;
                                let _ = app.update(cx, |this, cx| {
                                    if this.identity_replacement_dialog_revision == Some(revision) {
                                        this.identity_replacement_dialog_revision = None;
                                    }
                                    cx.notify();
                                });
                            }
                        })
                        .detach();
                    }

                    if !matches!(snapshot.status, ReceiverStatus::Pairing) {
                        this.pairing_dialog_code = None;
                        this.pairing_dialog_pending = None;
                        this.pairing_locally_confirmed = false;
                        if this.pairing_dialog.request_close() {
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
