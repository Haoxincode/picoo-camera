use gpui::prelude::FluentBuilder;
use gpui::*;
use gpui_component::button::*;
use gpui_component::*;

use crate::model::VirtualCameraStatus;
#[cfg(target_os = "macos")]
use crate::prefs::current_macos_boot_session;
use crate::prefs::{MacosCameraExtensionIntent, PendingMacosCameraExtension};
use crate::receiver_runtime::ReceiverSnapshot;
#[cfg(target_os = "macos")]
use crate::vcam_status::query_macos_vcam_status;
#[cfg(not(any(target_os = "macos", all(windows, feature = "windows-vcam"))))]
use crate::vcam_status::vcam_setup_unavailable_message;
use crate::vcam_status::{detect_vcam_status, vcam_repair_hint, vcam_setup_action_label};

use super::icons::{reicon_button_content, reicon_named};
use super::widgets::{
    page_header, placeholder_choice_indicator, placeholder_preview, placeholder_title,
    section_header, status_badge, status_row,
};
use super::PicooDesktopApp;

#[derive(Clone, PartialEq, Eq)]
pub(super) enum VcamSetupState {
    Idle,
    Running(VcamSetupOperation),
    Succeeded(String),
    Failed(String),
}

#[derive(Clone, Copy, PartialEq, Eq)]
pub(super) enum VcamSetupOperation {
    Activate,
    Deactivate,
    Detect,
}

impl VcamSetupState {
    fn is_running(&self) -> bool {
        matches!(self, Self::Running(_))
    }
}

fn macos_activation_action_visible(status: VirtualCameraStatus) -> bool {
    status == VirtualCameraStatus::Bundled
}

fn macos_deactivation_action_visible(status: VirtualCameraStatus) -> bool {
    matches!(
        status,
        VirtualCameraStatus::Installed | VirtualCameraStatus::Active
    )
}

fn resolve_pending_macos_vcam_status(
    status: VirtualCameraStatus,
    pending: Option<&PendingMacosCameraExtension>,
    current_boot_session: Option<&str>,
) -> (VirtualCameraStatus, bool, bool) {
    match pending {
        Some(pending)
            if pending.intent == MacosCameraExtensionIntent::Activate
                && status == VirtualCameraStatus::Active =>
        {
            (status, true, false)
        }
        Some(pending)
            if pending.intent == MacosCameraExtensionIntent::Deactivate
                && matches!(
                    status,
                    VirtualCameraStatus::Bundled | VirtualCameraStatus::NotInstalled
                ) =>
        {
            (status, true, false)
        }
        Some(pending) if current_boot_session.is_some_and(|boot| boot != pending.boot_session) => {
            // macOS promised completion after reboot, but the queried system
            // state still did not converge. Clear the stale lock so the user
            // can retry the actual lifecycle action.
            (status, true, true)
        }
        Some(pending) if pending.intent == MacosCameraExtensionIntent::Activate => {
            (VirtualCameraStatus::RestartRequired, false, false)
        }
        Some(_) => (VirtualCameraStatus::Uninstalling, false, false),
        None => (status, false, false),
    }
}

fn pending_macos_vcam_display_status(
    pending: Option<&PendingMacosCameraExtension>,
    fallback: VirtualCameraStatus,
) -> VirtualCameraStatus {
    match pending.map(|pending| pending.intent) {
        Some(MacosCameraExtensionIntent::Activate) => VirtualCameraStatus::RestartRequired,
        Some(MacosCameraExtensionIntent::Deactivate) => VirtualCameraStatus::Uninstalling,
        None => fallback,
    }
}

impl PicooDesktopApp {
    pub(super) fn refresh_vcam_status(&mut self, cx: &mut Context<Self>) {
        #[cfg(not(target_os = "macos"))]
        let _ = cx;

        #[cfg(target_os = "macos")]
        {
            if self.vcam_setup_state.is_running() {
                return;
            }
            self.vcam_status = VirtualCameraStatus::Unknown;
            self.runtime
                .set_virtual_camera_status(VirtualCameraStatus::Unknown);
            self.vcam_setup_state = VcamSetupState::Running(VcamSetupOperation::Detect);
            cx.notify();

            let query = cx.background_executor().spawn_dedicated(|_| async move {
                (query_macos_vcam_status(), current_macos_boot_session())
            });
            cx.spawn(async move |this, cx| {
                let (result, boot_session) = query.await;
                let _ = this.update(cx, |this, cx| {
                    match result {
                        Ok(status) => {
                            let (status, clear_pending, failed_after_reboot) =
                                resolve_pending_macos_vcam_status(
                                    status,
                                    this.prefs.pending_macos_camera_extension.as_ref(),
                                    boot_session.as_deref().ok(),
                                );
                            let persistence_error = if clear_pending {
                                this.prefs.pending_macos_camera_extension = None;
                                if let Err(err) = this.persist_prefs() {
                                    tracing::warn!(
                                        "clear Camera Extension pending state failed: {err}"
                                    );
                                    Some(err)
                                } else {
                                    None
                                }
                            } else {
                                None
                            };
                            this.vcam_status = status;
                            this.runtime.set_virtual_camera_status(status);
                            if failed_after_reboot {
                                this.vcam_setup_state = VcamSetupState::Failed(
                                    "Mac 已重新启动，但 Camera Extension 未完成系统变更。请重试。"
                                        .into(),
                                );
                            } else if let Some(err) = persistence_error {
                                this.vcam_setup_state = VcamSetupState::Failed(format!(
                                    "无法保存 Camera Extension 状态：{err}"
                                ));
                            } else {
                                this.vcam_setup_state = VcamSetupState::Idle;
                            }
                        }
                        Err(err) => {
                            tracing::warn!("Camera Extension status query failed: {err}");
                            let bundled = detect_vcam_status();
                            let status = pending_macos_vcam_display_status(
                                this.prefs.pending_macos_camera_extension.as_ref(),
                                bundled,
                            );
                            this.vcam_status = status;
                            this.runtime.set_virtual_camera_status(status);
                            this.vcam_setup_state = VcamSetupState::Failed(format!(
                                "无法读取 Camera Extension 系统状态：{err}"
                            ));
                        }
                    }
                    cx.notify();
                });
            })
            .detach();
        }

        #[cfg(not(target_os = "macos"))]
        {
            let status = detect_vcam_status();
            self.vcam_status = status;
            self.runtime.set_virtual_camera_status(status);
            self.vcam_setup_state = VcamSetupState::Idle;
        }
    }

    pub(super) fn vcam_setup_button_label(&self) -> &'static str {
        match self.vcam_setup_state {
            VcamSetupState::Running(VcamSetupOperation::Activate) => {
                if cfg!(target_os = "macos") {
                    "正在等待系统批准…"
                } else {
                    "正在等待管理员授权…"
                }
            }
            VcamSetupState::Running(VcamSetupOperation::Detect) => "正在检测…",
            VcamSetupState::Running(VcamSetupOperation::Deactivate) => "正在停用…",
            _ => vcam_setup_action_label(),
        }
    }

    pub(super) fn render_vcam_setup_feedback(&self, cx: &Context<Self>) -> Option<AnyElement> {
        match &self.vcam_setup_state {
            VcamSetupState::Idle => None,
            VcamSetupState::Running(VcamSetupOperation::Activate) => Some(
                div()
                    .text_sm()
                    .text_color(cx.theme().muted_foreground)
                    .child(if cfg!(target_os = "macos") {
                        "若 macOS 要求批准，请打开系统设置中的“登录项与扩展”，允许 Picoo Camera，然后返回此窗口。"
                    } else {
                        "请在 Windows 用户账户控制中允许 Picoo Camera 修改设备，然后返回此窗口。"
                    })
                    .into_any_element(),
            ),
            VcamSetupState::Running(VcamSetupOperation::Deactivate) => Some(
                div()
                    .text_sm()
                    .text_color(cx.theme().muted_foreground)
                    .child("正在请求 macOS 停用并移除 Camera Extension…")
                    .into_any_element(),
            ),
            VcamSetupState::Running(VcamSetupOperation::Detect) => None,
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

    pub(super) fn try_register_vcam(&mut self, cx: &mut Context<Self>) {
        if self.vcam_setup_state.is_running() {
            return;
        }

        #[cfg(all(windows, feature = "windows-vcam"))]
        {
            self.vcam_setup_state = VcamSetupState::Running(VcamSetupOperation::Activate);
            cx.notify();

            let repair = cx.background_executor().spawn_dedicated(|_| async move {
                crate::vcam_register::repair_system_registration_elevated()?;
                match detect_vcam_status() {
                    VirtualCameraStatus::Active => Ok(()),
                    status => Err(format!(
                        "修复进程已结束，但 Windows 摄像头枚举状态仍为 {status:?}"
                    )),
                }
            });
            cx.spawn(async move |this, cx| {
                let repair_result = repair.await;
                let _ = this.update(cx, |this, cx| {
                    match repair_result {
                        Ok(()) => {
                            this.vcam_status = VirtualCameraStatus::Active;
                            this.runtime
                                .set_virtual_camera_status(VirtualCameraStatus::Active);
                            this.vcam_setup_state =
                                VcamSetupState::Succeeded("虚拟摄像头已修复并激活。".into());
                        }
                        Err(err) => {
                            tracing::warn!("Install or repair Virtual Camera failed: {err}");
                            let status = detect_vcam_status();
                            this.vcam_status = status;
                            this.runtime.set_virtual_camera_status(status);
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
            if detect_vcam_status() == VirtualCameraStatus::NotInstalled {
                self.vcam_setup_state = VcamSetupState::Failed(
                    "当前应用包未包含 Camera Extension，请安装完整的 Picoo Camera.app。".into(),
                );
                cx.notify();
                return;
            }
            self.vcam_setup_state = VcamSetupState::Running(VcamSetupOperation::Activate);
            cx.notify();

            let activation = cx
                .background_executor()
                .spawn_dedicated(|_| async move { crate::macos_system_extension::activate() });
            cx.spawn(async move |this, cx| {
                let result = activation.await;
                let _ = this.update(cx, |this, cx| {
                    match result {
                        Ok(crate::macos_system_extension::LifecycleOutcome::Completed) => {
                            this.prefs.pending_macos_camera_extension = None;
                            let _ = this.persist_prefs();
                            this.vcam_status = VirtualCameraStatus::Active;
                            this.runtime
                                .set_virtual_camera_status(VirtualCameraStatus::Active);
                            this.vcam_setup_state = VcamSetupState::Succeeded(
                                "Camera Extension 已激活。请重启已打开的会议应用。".into(),
                            );
                        }
                        Ok(crate::macos_system_extension::LifecycleOutcome::RestartRequired) => {
                            this.vcam_status = VirtualCameraStatus::RestartRequired;
                            this.runtime
                                .set_virtual_camera_status(VirtualCameraStatus::RestartRequired);
                            this.vcam_setup_state = match this
                                .persist_pending_macos_vcam(MacosCameraExtensionIntent::Activate)
                            {
                                Ok(()) => VcamSetupState::Succeeded(
                                    "Camera Extension 将在重新启动 Mac 后激活。".into(),
                                ),
                                Err(err) => {
                                    tracing::warn!(
                                        "persist Camera Extension activation intent failed: {err}"
                                    );
                                    VcamSetupState::Failed(format!(
                                        "系统要求重新启动，但无法保存待处理状态：{err}"
                                    ))
                                }
                            };
                        }
                        Err(err) => {
                            tracing::warn!("Camera Extension activation failed: {err}");
                            let status = detect_vcam_status();
                            this.vcam_status = status;
                            this.runtime.set_virtual_camera_status(status);
                            this.vcam_setup_state =
                                VcamSetupState::Failed(format!("无法激活 Camera Extension：{err}"));
                        }
                    }
                    cx.notify();
                });
            })
            .detach();
        }
        #[cfg(not(any(target_os = "macos", all(windows, feature = "windows-vcam"))))]
        {
            let status = detect_vcam_status();
            self.vcam_status = status;
            self.runtime.set_virtual_camera_status(status);
            self.vcam_setup_state = VcamSetupState::Failed(vcam_setup_unavailable_message().into());
            cx.notify();
        }
    }

    #[cfg(target_os = "macos")]
    pub(super) fn deactivate_vcam(&mut self, cx: &mut Context<Self>) {
        if self.vcam_setup_state.is_running() {
            return;
        }
        self.vcam_setup_state = VcamSetupState::Running(VcamSetupOperation::Deactivate);
        cx.notify();

        let deactivation = cx
            .background_executor()
            .spawn_dedicated(|_| async move { crate::macos_system_extension::deactivate() });
        cx.spawn(async move |this, cx| {
            let result = deactivation.await;
            let _ = this.update(cx, |this, cx| {
                match result {
                    Ok(crate::macos_system_extension::LifecycleOutcome::Completed) => {
                        this.prefs.pending_macos_camera_extension = None;
                        let _ = this.persist_prefs();
                        this.vcam_status = VirtualCameraStatus::Bundled;
                        this.runtime
                            .set_virtual_camera_status(VirtualCameraStatus::Bundled);
                        this.vcam_setup_state = VcamSetupState::Succeeded(
                            "Camera Extension 已停用并从系统移除，可随时重新激活。".into(),
                        );
                    }
                    Ok(crate::macos_system_extension::LifecycleOutcome::RestartRequired) => {
                        this.vcam_status = VirtualCameraStatus::Uninstalling;
                        this.runtime
                            .set_virtual_camera_status(VirtualCameraStatus::Uninstalling);
                        this.vcam_setup_state = match this
                            .persist_pending_macos_vcam(MacosCameraExtensionIntent::Deactivate)
                        {
                            Ok(()) => VcamSetupState::Succeeded(
                                "Camera Extension 将在重新启动 Mac 后完成移除。".into(),
                            ),
                            Err(err) => {
                                tracing::warn!(
                                    "persist Camera Extension deactivation intent failed: {err}"
                                );
                                VcamSetupState::Failed(format!(
                                    "系统要求重新启动，但无法保存待处理状态：{err}"
                                ))
                            }
                        };
                    }
                    Err(err) => {
                        tracing::warn!("Camera Extension deactivation failed: {err}");
                        this.vcam_setup_state =
                            VcamSetupState::Failed(format!("无法停用 Camera Extension：{err}"));
                    }
                }
                cx.notify();
            });
        })
        .detach();
    }

    pub(super) fn render_first_launch(&self, cx: &Context<Self>) -> impl IntoElement {
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
                    .disabled(self.vcam_setup_state.is_running())
                    .on_click(cx.listener(|this, _, _, cx| {
                        this.refresh_vcam_status(cx);
                    })),
            )
            .when(
                !cfg!(target_os = "macos") || macos_activation_action_visible(self.vcam_status),
                |this| {
                    this.child(
                        Button::new("install-vcam")
                            .label(self.vcam_setup_button_label())
                            .loading(self.vcam_setup_state.is_running())
                            .disabled(self.vcam_setup_state.is_running())
                            .on_click(cx.listener(|this, _, _, cx| {
                                this.try_register_vcam(cx);
                            })),
                    )
                },
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

    pub(super) fn render_virtual_camera_page(
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
                                    .small()
                                    .accessibility_label("重新检测")
                                    .child(reicon_button_content(
                                        "重新检测",
                                        "refresh",
                                        cx.theme().primary,
                                    ))
                                    .disabled(self.vcam_setup_state.is_running())
                                    .on_click(cx.listener(|this, _, _, cx| {
                                        this.refresh_vcam_status(cx);
                                    })),
                            )
                            .when(
                                !cfg!(target_os = "macos")
                                    || macos_activation_action_visible(snapshot.virtual_camera),
                                |this| {
                                    this.child(
                                        Button::new("repair-vcam-page")
                                            .primary()
                                            .small()
                                            .accessibility_label(self.vcam_setup_button_label())
                                            .child(reicon_button_content(
                                                self.vcam_setup_button_label(),
                                                "play-filled",
                                                cx.theme().primary_foreground,
                                            ))
                                            .loading(self.vcam_setup_state.is_running())
                                            .disabled(self.vcam_setup_state.is_running())
                                            .on_click(cx.listener(|this, _, _, cx| {
                                                this.try_register_vcam(cx);
                                            })),
                                    )
                                },
                            ),
                    )
                    .when(
                        cfg!(target_os = "macos")
                            && macos_deactivation_action_visible(snapshot.virtual_camera),
                        |this| {
                            this.child(
                                Button::new("deactivate-vcam-page")
                                    .outline()
                                    .label("停用 Camera Extension")
                                    .disabled(self.vcam_setup_state.is_running())
                                    .on_click(cx.listener(|_this, _, _, _cx| {
                                        #[cfg(target_os = "macos")]
                                        _this.deactivate_vcam(_cx);
                                    })),
                            )
                        },
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
}

pub(super) fn vcam_label_zh(status: VirtualCameraStatus) -> &'static str {
    match status {
        VirtualCameraStatus::Unknown => "检测中",
        VirtualCameraStatus::Bundled => "已随附 · 待激活",
        VirtualCameraStatus::AwaitingApproval => "等待系统批准",
        VirtualCameraStatus::RestartRequired => "重启后生效",
        VirtualCameraStatus::Uninstalling => "正在移除",
        VirtualCameraStatus::Installed => "就绪 (Ready)",
        VirtualCameraStatus::NotInstalled => "未安装 (Not Installed)",
        VirtualCameraStatus::Active => "就绪 (Active)",
    }
}

#[cfg(test)]
mod tests {
    use super::{
        macos_activation_action_visible, macos_deactivation_action_visible,
        resolve_pending_macos_vcam_status,
    };
    use crate::model::VirtualCameraStatus;
    use crate::prefs::{MacosCameraExtensionIntent, PendingMacosCameraExtension};

    #[test]
    fn macos_camera_extension_actions_follow_lifecycle_state() {
        for status in [
            VirtualCameraStatus::Unknown,
            VirtualCameraStatus::AwaitingApproval,
            VirtualCameraStatus::RestartRequired,
            VirtualCameraStatus::Uninstalling,
            VirtualCameraStatus::Installed,
            VirtualCameraStatus::NotInstalled,
            VirtualCameraStatus::Active,
        ] {
            assert!(!macos_activation_action_visible(status));
        }
        assert!(macos_activation_action_visible(
            VirtualCameraStatus::Bundled
        ));
        assert!(macos_deactivation_action_visible(
            VirtualCameraStatus::Installed
        ));
        assert!(macos_deactivation_action_visible(
            VirtualCameraStatus::Active
        ));
        assert!(!macos_deactivation_action_visible(
            VirtualCameraStatus::RestartRequired
        ));
        assert!(!macos_deactivation_action_visible(
            VirtualCameraStatus::Uninstalling
        ));
    }

    #[test]
    fn macos_reboot_pending_intent_survives_until_system_state_converges() {
        let activation = PendingMacosCameraExtension {
            intent: MacosCameraExtensionIntent::Activate,
            boot_session: "boot-a".into(),
        };
        let deactivation = PendingMacosCameraExtension {
            intent: MacosCameraExtensionIntent::Deactivate,
            boot_session: "boot-a".into(),
        };
        assert_eq!(
            resolve_pending_macos_vcam_status(
                VirtualCameraStatus::Bundled,
                Some(&activation),
                Some("boot-a")
            ),
            (VirtualCameraStatus::RestartRequired, false, false)
        );
        assert_eq!(
            resolve_pending_macos_vcam_status(
                VirtualCameraStatus::Active,
                Some(&activation),
                Some("boot-a")
            ),
            (VirtualCameraStatus::Active, true, false)
        );
        assert_eq!(
            resolve_pending_macos_vcam_status(
                VirtualCameraStatus::Active,
                Some(&deactivation),
                Some("boot-a")
            ),
            (VirtualCameraStatus::Uninstalling, false, false)
        );
        assert_eq!(
            resolve_pending_macos_vcam_status(
                VirtualCameraStatus::Bundled,
                Some(&deactivation),
                Some("boot-a")
            ),
            (VirtualCameraStatus::Bundled, true, false)
        );
    }

    #[test]
    fn macos_reboot_pending_intent_unlocks_retry_when_system_did_not_converge() {
        let activation = PendingMacosCameraExtension {
            intent: MacosCameraExtensionIntent::Activate,
            boot_session: "boot-a".into(),
        };
        assert_eq!(
            resolve_pending_macos_vcam_status(
                VirtualCameraStatus::Bundled,
                Some(&activation),
                Some("boot-b")
            ),
            (VirtualCameraStatus::Bundled, true, true)
        );
    }
}
