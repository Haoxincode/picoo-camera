use gpui::prelude::FluentBuilder;
use gpui::*;
use gpui_component::alert::Alert;
use gpui_component::button::*;
use gpui_component::*;
use picoo_receiver::ReceiverError;

use crate::receiver_runtime::{repair_receiver_identity_and_reset_trust, reset_receiver_trust};

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(super) enum PairingRecoveryKind {
    Identity,
    TrustedStore,
}

impl PairingRecoveryKind {
    pub(super) fn classify(error: &ReceiverError) -> Option<Self> {
        match error {
            ReceiverError::Identity(_) => Some(Self::Identity),
            ReceiverError::Store(_) => Some(Self::TrustedStore),
            _ => None,
        }
    }

    fn title(self) -> &'static str {
        match self {
            Self::Identity => "设备身份损坏",
            Self::TrustedStore => "配对数据损坏",
        }
    }

    fn description(self) -> &'static str {
        match self {
            Self::Identity => {
                "Picoo Camera 无法验证保存在系统安全存储中的设备身份，因此没有启动连接或媒体服务。修复会生成新的设备身份并清除全部配对；所有手机都需要重新核对配对短码。"
            }
            Self::TrustedStore => {
                "Picoo Camera 无法验证本机配对数据，因此没有启动连接或媒体服务。重置会保留本机设备身份，但清除全部可信设备；所有手机都需要重新核对配对短码。"
            }
        }
    }

    fn action_label(self) -> &'static str {
        match self {
            Self::Identity => "修复身份",
            Self::TrustedStore => "重置配对",
        }
    }

    fn recover(self, display_name: &str) -> Result<(), String> {
        match self {
            Self::Identity => repair_receiver_identity_and_reset_trust(display_name),
            Self::TrustedStore => reset_receiver_trust(),
        }
        .map_err(|error| error.to_string())
    }
}

#[derive(Debug, Default, PartialEq, Eq)]
enum PairingRecoveryState {
    #[default]
    Ready,
    Running,
    Succeeded,
    Failed(String),
}

pub(super) struct IdentityRecoveryView {
    kind: PairingRecoveryKind,
    display_name: String,
    state: PairingRecoveryState,
}

impl IdentityRecoveryView {
    pub(super) fn new(kind: PairingRecoveryKind, display_name: String) -> Self {
        Self {
            kind,
            display_name,
            state: PairingRecoveryState::Ready,
        }
    }

    fn begin_recovery(&mut self, cx: &mut Context<Self>) {
        if self.state == PairingRecoveryState::Running {
            return;
        }
        self.state = PairingRecoveryState::Running;
        cx.notify();

        let kind = self.kind;
        let display_name = self.display_name.clone();
        let recovery = cx
            .background_executor()
            .spawn_dedicated(move |_| async move { kind.recover(&display_name) });
        cx.spawn(async move |this, cx| {
            let result = recovery.await;
            let _ = this.update(cx, |this, cx| {
                this.state = match result {
                    Ok(()) => PairingRecoveryState::Succeeded,
                    Err(error) => PairingRecoveryState::Failed(error),
                };
                cx.notify();
            });
        })
        .detach();
    }
}

impl Render for IdentityRecoveryView {
    fn render(&mut self, _: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let running = self.state == PairingRecoveryState::Running;
        let finished = self.state == PairingRecoveryState::Succeeded;

        div()
            .v_flex()
            .size_full()
            .items_center()
            .justify_center()
            .p_8()
            .bg(cx.theme().background)
            .child(
                div()
                    .v_flex()
                    .w_full()
                    .max_w_96()
                    .gap_6()
                    .p_6()
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
                                    .text_xl()
                                    .font_weight(FontWeight::SEMIBOLD)
                                    .child(self.kind.title()),
                            )
                            .child(
                                div()
                                    .text_sm()
                                    .text_color(cx.theme().muted_foreground)
                                    .child(self.kind.description()),
                            ),
                    )
                    .children(match &self.state {
                        PairingRecoveryState::Succeeded => vec![Alert::success(
                            "pairing-recovery-success",
                            "修复已完成。关闭并重新打开 Picoo Camera 后，即可重新配对。",
                        )
                        .title("本机已恢复")
                        .into_any_element()],
                        PairingRecoveryState::Failed(error) => vec![Alert::error(
                            "pairing-recovery-error",
                            format!("修复失败：{error}"),
                        )
                        .title("无法完成修复")
                        .into_any_element()],
                        PairingRecoveryState::Ready | PairingRecoveryState::Running => Vec::new(),
                    })
                    .child(
                        div()
                            .h_flex()
                            .items_center()
                            .justify_end()
                            .gap_2()
                            .child(
                                Button::new("quit-pairing-recovery")
                                    .ghost()
                                    .label(if finished { "关闭" } else { "退出" })
                                    .disabled(running)
                                    .on_click(|_, _, cx| cx.quit()),
                            )
                            .when(!finished, |this| {
                                this.child(
                                    Button::new("confirm-pairing-recovery")
                                        .danger()
                                        .label(self.kind.action_label())
                                        .loading(running)
                                        .disabled(running)
                                        .on_click(cx.listener(|this, _, _, cx| {
                                            this.begin_recovery(cx);
                                        })),
                                )
                            }),
                    ),
            )
    }
}

#[cfg(test)]
mod tests {
    use picoo_pairing::{IdentityError, StoreError};
    use picoo_receiver::ReceiverError;
    use picoo_transport::TransportError;

    use super::PairingRecoveryKind;

    #[test]
    fn only_identity_and_trust_corruption_enter_recovery_ui() {
        let identity = ReceiverError::Identity(IdentityError::Invalid("fixture".into()));
        let store = ReceiverError::Store(StoreError::InvalidData("fixture".into()));
        let transport = ReceiverError::Transport(TransportError::NotConnected);

        assert_eq!(
            PairingRecoveryKind::classify(&identity),
            Some(PairingRecoveryKind::Identity)
        );
        assert_eq!(
            PairingRecoveryKind::classify(&store),
            Some(PairingRecoveryKind::TrustedStore)
        );
        assert_eq!(PairingRecoveryKind::classify(&transport), None);
    }
}
