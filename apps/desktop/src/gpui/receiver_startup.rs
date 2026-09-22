use gpui_kit::component::alert::Alert;
use gpui_kit::component::button::*;
use gpui_kit::component::spinner::Spinner;
use gpui_kit::component::*;
use gpui_kit::*;
use picoo_receiver::ReceiverError;
#[cfg(all(windows, feature = "windows-vcam"))]
use picoo_session::ReceiverStatus;

use super::blocking::spawn_os_thread;
use super::identity_recovery::{IdentityRecoveryView, PairingRecoveryKind};
use super::PicooDesktopApp;
use crate::model::VirtualCameraStatus;
use crate::prefs::DesktopPreferences;
use crate::receiver_runtime::ReceiverRuntimeHandle;

enum StartupState {
    Loading,
    Ready,
    Recovery,
    Failed(String),
}

/// REQ-PICOO-UI-002 / REQ-PICOO-UI-011: keep the GPUI message pump responsive
/// while the Receiver performs platform and network initialization on its owner
/// thread.
pub(super) struct ReceiverStartupView {
    prefs: DesktopPreferences,
    window_handle: AnyWindowHandle,
    state: StartupState,
    dispatched: bool,
    content: Option<AnyView>,
    desktop_view: Option<Entity<PicooDesktopApp>>,
}

impl ReceiverStartupView {
    pub(super) fn new(prefs: DesktopPreferences, window_handle: AnyWindowHandle) -> Self {
        Self {
            prefs,
            window_handle,
            state: StartupState::Loading,
            dispatched: false,
            content: None,
            desktop_view: None,
        }
    }

    pub(super) fn close_outcome(&self, cx: &App) -> crate::tray::CloseOutcome {
        self.desktop_view
            .as_ref()
            .map(|view| view.read(cx).close_outcome())
            .unwrap_or_else(|| {
                crate::tray::TrayPolicy::for_current_platform(self.prefs.minimize_to_tray)
                    .close_outcome()
            })
    }

    #[cfg(all(windows, feature = "windows-vcam"))]
    pub(super) fn tray_status(&self, cx: &App) -> ReceiverStatus {
        self.desktop_view
            .as_ref()
            .map(|view| view.read(cx).runtime.snapshot().status)
            .unwrap_or(ReceiverStatus::Discovering)
    }

    pub(super) fn start(&mut self, cx: &mut Context<Self>) {
        if self.dispatched {
            return;
        }
        self.dispatched = true;

        #[cfg(all(windows, feature = "windows-vcam"))]
        self.start_tray_pump(cx);

        let prefs = self.prefs.clone();
        let window_handle = self.window_handle;
        let startup = spawn_os_thread(cx, move || {
            // VCam enumeration is a best-effort status probe.  It may enter
            // Media Foundation / COM code that waits for a device refresh, so
            // it must not gate creation of the receiver UI or its quit path.
            ReceiverRuntimeHandle::start_from_prefs(prefs, VirtualCameraStatus::Unknown)
        });

        cx.spawn(async move |this, cx| {
            let result = match startup.await {
                Ok(result) => result,
                Err(error) => Err(ReceiverError::Protocol(error)),
            };
            match result {
                Ok(runtime) => {
                    let _ = window_handle.update(cx, |_, window, cx| {
                        let _ = this.update(cx, |startup, cx| {
                            let prefs = startup.prefs.clone();
                            let view = cx.new(|cx| {
                                PicooDesktopApp::new(
                                    runtime,
                                    prefs,
                                    VirtualCameraStatus::Unknown,
                                    window_handle,
                                    window,
                                    cx,
                                )
                            });
                            view.update(cx, |this, cx| {
                                this.ensure_pump_loop(cx);
                                #[cfg(target_os = "macos")]
                                this.refresh_vcam_status(cx);
                            });
                            startup.desktop_view = Some(view.clone());
                            startup.content = Some(view.into());
                            startup.state = StartupState::Ready;
                            cx.notify();

                            // Refresh the non-critical VCam status after the
                            // receiver is visible.  This task is independent
                            // from the startup state and cannot strand the
                            // loading/quit UI if Media Foundation is slow.
                            #[cfg(all(windows, feature = "windows-vcam"))]
                            {
                                let probe = spawn_os_thread(cx, crate::vcam_status::detect_vcam_status);
                                let view_for_probe = startup.desktop_view.clone();
                                cx.spawn(async move |_, cx| {
                                    if let Ok(status) = probe.await {
                                        if let Some(view) = view_for_probe {
                                            view.update(cx, |this, cx| {
                                                this.apply_detected_vcam_status(status, cx);
                                            });
                                        }
                                    }
                                })
                                .detach();
                            }
                        });
                    });
                }
                Err(error) => {
                    let _ = this.update(cx, |startup, cx| {
                        if let Some(kind) = PairingRecoveryKind::classify(&error) {
                            tracing::error!(%error, "Receiver identity/trust startup failed closed");
                            startup.state = StartupState::Recovery;
                            let display_name = startup.prefs.display_name.clone();
                            let recovery =
                                cx.new(|_| IdentityRecoveryView::new(kind, display_name));
                            startup.content = Some(recovery.into());
                        } else {
                            tracing::error!(%error, "Receiver startup failed");
                            #[cfg(all(windows, feature = "gpui-ui"))]
                            crate::windows_startup::append_log(&format!(
                                "receiver startup failed: {error}"
                            ));
                            startup.state =
                                StartupState::Failed(receiver_startup_error_message(&error.to_string()));
                        }
                        cx.notify();
                    });
                }
            }
        })
        .detach();
    }

    #[cfg(all(windows, feature = "windows-vcam"))]
    fn start_tray_pump(&self, cx: &mut Context<Self>) {
        cx.spawn(async move |this, cx| loop {
            cx.background_executor()
                .timer(std::time::Duration::from_millis(16))
                .await;
            let should_stop = this
                .update(cx, |startup, cx| {
                    crate::tray::pump_win32_tray_messages();
                    if let Some(action) = crate::tray::take_pending_menu_action() {
                        let outcome = action.apply();
                        if outcome.quit {
                            crate::tray::note_tray_cleared();
                            cx.quit();
                        } else if outcome.restore_window {
                            crate::tray::force_show_product_window();
                            cx.spawn(async move |_, cx| {
                                cx.background_executor()
                                    .timer(std::time::Duration::from_millis(0))
                                    .await;
                                cx.update(|cx| cx.activate(true));
                            })
                            .detach();
                        }
                    }
                    matches!(&startup.state, StartupState::Ready)
                })
                .unwrap_or(true);
            if should_stop {
                break;
            }
        })
        .detach();
    }
}

impl Render for ReceiverStartupView {
    fn render(&mut self, _: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        if let Some(content) = self.content.clone() {
            return content.into_any_element();
        }

        let body = match &self.state {
            StartupState::Loading => div()
                .v_flex()
                .items_center()
                .gap_4()
                .child(Spinner::new().large())
                .child(div().text_lg().child("正在启动"))
                .child(
                    div()
                        .text_sm()
                        .text_color(cx.theme().muted_foreground)
                        .child("正在初始化摄像头服务…"),
                )
                .child(
                    Button::new("quit-receiver-startup-loading")
                        .danger()
                        .label("退出")
                        .on_click(|_, _, cx| cx.quit()),
                )
                .into_any_element(),
            StartupState::Failed(error) => div()
                .v_flex()
                .w_full()
                .max_w_96()
                .gap_4()
                .child(Alert::error("receiver-startup-error", error.clone()).title("启动失败"))
                .child(
                    div().h_flex().justify_end().child(
                        Button::new("quit-receiver-startup")
                            .danger()
                            .label("退出")
                            .on_click(|_, _, cx| cx.quit()),
                    ),
                )
                .into_any_element(),
            StartupState::Ready | StartupState::Recovery => div().into_any_element(),
        };

        div()
            .size_full()
            .v_flex()
            .items_center()
            .justify_center()
            .p_8()
            .bg(cx.theme().background)
            .child(body)
            .into_any_element()
    }
}

pub(crate) fn receiver_startup_error_message(error: &str) -> String {
    if error.contains("10048")
        || error.contains("只允许使用一次")
        || error.contains("Address already in use")
    {
        if cfg!(windows) {
            "无法启动 Picoo Camera：局域网端口 4433 已被占用。请先退出任务栏托盘中已运行的 Picoo Camera，或关闭占用该端口的程序。".into()
        } else {
            "无法启动 Picoo Camera：局域网端口 4433 已被占用。请先退出已运行的 Picoo Camera，或关闭占用该端口的程序。".into()
        }
    } else {
        format!("无法启动 Picoo Camera：{error}")
    }
}

#[cfg(test)]
mod tests {
    use super::receiver_startup_error_message;

    #[test]
    fn address_in_use_explains_the_quic_port() {
        let message = receiver_startup_error_message(
            "transport: connection failed: I/O error: 通常每个套接字地址(协议/网络地址/端口)只允许使用一次。 (os error 10048)",
        );
        assert!(message.contains("4433"), "{message}");
        assert!(message.contains("已运行"), "{message}");
        #[cfg(windows)]
        assert!(message.contains("托盘"), "{message}");
        #[cfg(not(windows))]
        assert!(!message.contains("托盘"), "{message}");
    }

    #[test]
    fn receiver_startup_leaves_the_gpui_pump_free() {
        let source = include_str!("receiver_startup.rs");
        let start = source.find("pub(super) fn start(").expect("start method");
        let render = source
            .find("impl Render for ReceiverStartupView")
            .expect("render impl");
        let tests = source.find("#[cfg(test)]").expect("tests");
        assert!(source.contains("spawn_os_thread"));
        assert!(
            source.contains(
                "ReceiverRuntimeHandle::start_from_prefs(prefs, VirtualCameraStatus::Unknown)"
            ),
            "VCam probing must not gate receiver UI startup"
        );
        assert!(
            source[start..render].contains("start_from_prefs"),
            "Receiver factory belongs on the startup worker"
        );
        assert!(
            !source[render..tests].contains("start_from_prefs"),
            "Receiver factory must not run inside render"
        );
        assert!(
            source[start..render].contains("if self.dispatched"),
            "startup dispatch must be idempotent"
        );
    }
}
