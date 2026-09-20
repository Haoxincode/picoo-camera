//! Desktop recording commands and presentation — REQ-PICOO-MEDIA-075/083.
use super::PicooDesktopApp;
use crate::receiver_runtime::{
    await_receiver_reply, ReceiverSnapshot, RecordingRequest, RecordingSnapshot,
};
use gpui_kit::component::dialog::DialogButtonProps;
use gpui_kit::component::menu::DropdownMenu;
use gpui_kit::component::{button::*, *};
use gpui_kit::prelude::FluentBuilder;
use gpui_kit::*;
use picoo_protocol::control::VideoCodec;
use picoo_recording::bundle::{RecordingMode, RecordingState};
use serde::Deserialize;

#[derive(Clone, Action, PartialEq, Eq, Deserialize)]
#[action(namespace = picoo_recording, no_json)]
enum RecordingAction {
    StartRenderedAvc30,
    StartRenderedAvc60,
    StartRenderedHevc30,
    StartRenderedHevc60,
}

impl RecordingAction {
    fn request(&self) -> RecordingRequest {
        let (codec, fps) = match self {
            Self::StartRenderedAvc30 => (VideoCodec::Avc, 30),
            Self::StartRenderedAvc60 => (VideoCodec::Avc, 60),
            Self::StartRenderedHevc30 => (VideoCodec::Hevc, 30),
            Self::StartRenderedHevc60 => (VideoCodec::Hevc, 60),
        };
        RecordingRequest::Rendered { codec, fps }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum RecordingPending {
    Starting,
    Stopping,
}

#[derive(Default)]
pub(super) struct RecordingUiState {
    encoded_pending: Option<RecordingPending>,
    rendered_pending: Option<RecordingPending>,
    encoded_error: Option<String>,
    rendered_error: Option<String>,
}

impl RecordingUiState {
    fn pending(&self, mode: RecordingMode) -> Option<RecordingPending> {
        match mode {
            RecordingMode::Encoded => self.encoded_pending,
            RecordingMode::Rendered => self.rendered_pending,
        }
    }

    fn set_pending(&mut self, mode: RecordingMode, pending: Option<RecordingPending>) {
        match mode {
            RecordingMode::Encoded => self.encoded_pending = pending,
            RecordingMode::Rendered => self.rendered_pending = pending,
        }
    }

    fn error(&self, mode: RecordingMode) -> Option<&str> {
        match mode {
            RecordingMode::Encoded => self.encoded_error.as_deref(),
            RecordingMode::Rendered => self.rendered_error.as_deref(),
        }
    }

    fn set_error(&mut self, mode: RecordingMode, error: Option<String>) {
        match mode {
            RecordingMode::Encoded => self.encoded_error = error,
            RecordingMode::Rendered => self.rendered_error = error,
        }
    }
}

fn button_label(
    mode: RecordingMode,
    snapshot: &RecordingSnapshot,
    pending: Option<RecordingPending>,
) -> &'static str {
    match pending {
        Some(RecordingPending::Starting) => return "正在开始录像…",
        Some(RecordingPending::Stopping) => return "正在提交停止…",
        None => {}
    }
    if snapshot.stopping {
        "正在结束录像"
    } else if active(snapshot) {
        "停止录像"
    } else {
        match mode {
            RecordingMode::Encoded => "开始原码流录像…",
            RecordingMode::Rendered => "开始处理后录像…",
        }
    }
}

fn active(snapshot: &RecordingSnapshot) -> bool {
    snapshot.result.is_none()
        && matches!(
            snapshot.state,
            Some(RecordingState::Arming | RecordingState::Recording)
        )
}

fn status(mode: RecordingMode, snapshot: &RecordingSnapshot) -> &'static str {
    if snapshot.stalled && snapshot.result.is_none() {
        return "录像写入无响应";
    }
    if snapshot.stopping {
        return "正在结束录像";
    }
    match snapshot.state {
        None => match mode {
            RecordingMode::Encoded => "保存接收到的原始画面",
            RecordingMode::Rendered => "保存方向与处理效果一致的画面",
        },
        Some(RecordingState::Arming) => "正在准备录制",
        Some(RecordingState::Recording) => "正在录制",
        Some(RecordingState::Complete) => "录像已保存",
        Some(RecordingState::HasGaps) => "录像已保存，含画面缺口",
        Some(RecordingState::Failed) => "录像未完成",
    }
}

impl PicooDesktopApp {
    fn begin_recording(&mut self, request: RecordingRequest, cx: &mut Context<Self>) {
        let mode = request.mode();
        if self.recording_ui.pending(mode).is_some() {
            return;
        }
        self.recording_ui
            .set_pending(mode, Some(RecordingPending::Starting));
        self.recording_ui.set_error(mode, None);
        let selection = cx.prompt_for_paths(PathPromptOptions {
            files: false,
            directories: true,
            multiple: false,
            prompt: Some(
                match mode {
                    RecordingMode::Encoded => "选择原码流录像保存文件夹",
                    RecordingMode::Rendered => "选择处理后录像保存文件夹",
                }
                .into(),
            ),
        });
        cx.spawn(async move |this, cx| {
            let selected = match selection.await {
                Ok(Ok(paths)) => Ok(paths.and_then(|paths| paths.into_iter().next())),
                Ok(Err(error)) => Err(error.to_string()),
                Err(error) => Err(error.to_string()),
            };
            let result = match selected {
                Ok(Some(path)) => {
                    match this.update(cx, |this, _| this.runtime.start_recording(request, path)) {
                        Ok(reply) => await_receiver_reply(reply)
                            .await
                            .map_err(|error| error.to_string()),
                        Err(_) => return,
                    }
                }
                Ok(None) => Ok(()),
                Err(error) => Err(error),
            };
            let _ = this.update(cx, |this, cx| {
                this.recording_ui.set_pending(mode, None);
                this.recording_ui.set_error(
                    mode,
                    result.err().map(|error| format!("无法开始录像：{error}")),
                );
                cx.notify();
            });
        })
        .detach();
        cx.notify();
    }

    fn end_recording(&mut self, mode: RecordingMode, cx: &mut Context<Self>) {
        if self.recording_ui.pending(mode).is_some() {
            return;
        }
        self.recording_ui
            .set_pending(mode, Some(RecordingPending::Stopping));
        self.recording_ui.set_error(mode, None);
        let reply = self.runtime.stop_recording(mode);
        cx.spawn(async move |this, cx| {
            let result = await_receiver_reply(reply).await;
            let _ = this.update(cx, |this, cx| {
                this.recording_ui.set_pending(mode, None);
                this.recording_ui.set_error(
                    mode,
                    result.err().map(|error| format!("无法停止录像：{error}")),
                );
                cx.notify();
            });
        })
        .detach();
        cx.notify();
    }

    fn render_recording_row(
        &self,
        mode: RecordingMode,
        recording: &RecordingSnapshot,
        snapshot: &ReceiverSnapshot,
        cx: &Context<Self>,
    ) -> AnyElement {
        let is_active = active(recording);
        let is_stopping = recording.stopping;
        let disconnected = snapshot.stream_config.is_none() || snapshot.active_sender.is_none();
        let unsupported_rendered_source =
            mode == RecordingMode::Rendered && !snapshot.recordings.rendered_source_supported;
        let unavailable = disconnected || unsupported_rendered_source;
        let pending = self.recording_ui.pending(mode);
        let label = button_label(mode, recording, pending);
        let operation_error = self.recording_ui.error(mode).map(str::to_owned);
        let detail = operation_error
            .clone()
            .or_else(|| {
                recording
                    .result
                    .as_ref()
                    .and_then(|result| result.error.clone())
            })
            .or_else(|| {
                recording.stalled.then(|| {
                    "录制工作者超过 15 秒未推进，可能仍在等待系统写入。直播不受影响；文件结果将在写入与清理返回后更新。".to_owned()
                })
            });
        let message = if operation_error.is_some() {
            "录像操作未完成"
        } else {
            status(mode, recording)
        };
        let reveal = recording
            .result
            .as_ref()
            .and_then(|result| result.path.clone());
        let source_fps = snapshot
            .stream_config
            .as_ref()
            .map(|config| config.fps)
            .unwrap_or(0);
        let button_id = match mode {
            RecordingMode::Encoded => "encoded-recording-toggle",
            RecordingMode::Rendered => "rendered-recording-toggle",
        };
        let button = Button::new(button_id)
            .outline()
            .small()
            .label(label)
            .accessibility_label(label)
            .loading(pending.is_some())
            .disabled(pending.is_some() || is_stopping || (!is_active && unavailable))
            .tooltip(if !is_active && disconnected {
                "连接手机并准备视频后可录制"
            } else if !is_active && unsupported_rendered_source {
                "当前视频规格不支持处理后录像；请选择720p或1080p、30或60fps"
            } else {
                match mode {
                    RecordingMode::Encoded => "保存手机发送的原始码流",
                    RecordingMode::Rendered => "选择编码格式与帧率；输出尺寸不随窗口变化",
                }
            });
        let button: AnyElement = if is_active {
            button
                .on_click(cx.listener(move |this, _, _, cx| {
                    this.end_recording(mode, cx);
                }))
                .into_any_element()
        } else {
            match mode {
                RecordingMode::Encoded => button
                    .on_click(cx.listener(|this, _, _, cx| {
                        this.begin_recording(RecordingRequest::Encoded, cx);
                    }))
                    .into_any_element(),
                RecordingMode::Rendered => button
                    .dropdown_menu_with_anchor(Anchor::TopLeft, move |menu, _, _| {
                        menu.label("处理后录像格式")
                            .menu_with_enable(
                                "H.264 · 30 fps",
                                Box::new(RecordingAction::StartRenderedAvc30),
                                source_fps >= 30,
                            )
                            .menu_with_enable(
                                "H.264 · 60 fps",
                                Box::new(RecordingAction::StartRenderedAvc60),
                                source_fps >= 60,
                            )
                            .separator()
                            .menu_with_enable(
                                "HEVC · 30 fps",
                                Box::new(RecordingAction::StartRenderedHevc30),
                                source_fps >= 30,
                            )
                            .menu_with_enable(
                                "HEVC · 60 fps",
                                Box::new(RecordingAction::StartRenderedHevc60),
                                source_fps >= 60,
                            )
                    })
                    .into_any_element(),
            }
        };

        div()
            .h_flex()
            .items_center()
            .gap_3()
            .child(button)
            .child(
                div()
                    .flex_1()
                    .min_w_0()
                    .truncate()
                    .text_sm()
                    .text_color(if detail.is_some() {
                        cx.theme().danger
                    } else {
                        cx.theme().muted_foreground
                    })
                    .child(message),
            )
            .when_some(detail, |row, error| {
                row.child(
                    Button::new(match mode {
                        RecordingMode::Encoded => "encoded-recording-error-details",
                        RecordingMode::Rendered => "rendered-recording-error-details",
                    })
                    .ghost()
                    .small()
                    .label("查看原因")
                    .on_click(move |_, window, cx| {
                        let error = error.clone();
                        window.open_alert_dialog(cx, move |alert, _, _| {
                            alert
                                .title("录像详情")
                                .description(error.clone())
                                .button_props(DialogButtonProps::default().ok_text("关闭"))
                        });
                    }),
                )
            })
            .when_some(reveal, |row, path| {
                row.child(
                    Button::new(match mode {
                        RecordingMode::Encoded => "reveal-encoded-recording",
                        RecordingMode::Rendered => "reveal-rendered-recording",
                    })
                    .ghost()
                    .small()
                    .label("打开文件夹")
                    .on_click(move |_, _, cx| cx.reveal_path(&path)),
                )
            })
            .into_any_element()
    }

    pub(super) fn render_recording_bar(
        &self,
        snapshot: &ReceiverSnapshot,
        cx: &Context<Self>,
    ) -> AnyElement {
        let recordings = &snapshot.recordings;
        if !recordings.encoded.available && !recordings.rendered.available {
            return div().into_any_element();
        }
        div()
            .v_flex()
            .flex_shrink_0()
            .gap_2()
            .px_3()
            .py_2()
            .border_b_1()
            .border_color(cx.theme().border)
            .when(recordings.encoded.available, |bar| {
                bar.child(self.render_recording_row(
                    RecordingMode::Encoded,
                    &recordings.encoded,
                    snapshot,
                    cx,
                ))
            })
            .when(recordings.rendered.available, |bar| {
                bar.child(self.render_recording_row(
                    RecordingMode::Rendered,
                    &recordings.rendered,
                    snapshot,
                    cx,
                ))
            })
            .on_action(cx.listener(|this, action: &RecordingAction, _, cx| {
                this.begin_recording(action.request(), cx);
            }))
            .into_any_element()
    }
}

#[cfg(test)]
mod tests {
    use super::{
        active, button_label, status, RecordingAction, RecordingMode, RecordingPending,
        RecordingRequest, RecordingSnapshot, RecordingState, RecordingUiState, VideoCodec,
    };

    #[test]
    fn preparing_finalizing_and_gap_results_are_not_presented_as_complete() {
        let mut snapshot = RecordingSnapshot {
            available: true,
            state: Some(RecordingState::Arming),
            ..Default::default()
        };
        assert!(active(&snapshot));
        assert_eq!(status(RecordingMode::Encoded, &snapshot), "正在准备录制");
        snapshot.stopping = true;
        assert_eq!(status(RecordingMode::Encoded, &snapshot), "正在结束录像");
        snapshot.stalled = true;
        assert_eq!(status(RecordingMode::Encoded, &snapshot), "录像写入无响应");
        snapshot.stalled = false;
        snapshot.stopping = false;
        snapshot.state = Some(RecordingState::HasGaps);
        assert!(!active(&snapshot));
        assert_eq!(
            status(RecordingMode::Rendered, &snapshot),
            "录像已保存，含画面缺口"
        );
        snapshot.state = Some(RecordingState::Failed);
        assert_eq!(status(RecordingMode::Rendered, &snapshot), "录像未完成");
    }

    #[test]
    fn idle_copy_distinguishes_encoded_and_rendered_recording() {
        let snapshot = RecordingSnapshot::default();
        assert_eq!(
            status(RecordingMode::Encoded, &snapshot),
            "保存接收到的原始画面"
        );
        assert_eq!(
            status(RecordingMode::Rendered, &snapshot),
            "保存方向与处理效果一致的画面"
        );
    }

    #[test]
    fn pending_and_loading_copy_are_independent_per_mode() {
        let mut ui = RecordingUiState::default();
        ui.set_pending(RecordingMode::Encoded, Some(RecordingPending::Starting));
        assert_eq!(
            ui.pending(RecordingMode::Encoded),
            Some(RecordingPending::Starting)
        );
        assert_eq!(ui.pending(RecordingMode::Rendered), None);
        assert_eq!(
            button_label(
                RecordingMode::Encoded,
                &RecordingSnapshot::default(),
                ui.pending(RecordingMode::Encoded),
            ),
            "正在开始录像…"
        );

        ui.set_pending(RecordingMode::Rendered, Some(RecordingPending::Stopping));
        ui.set_pending(RecordingMode::Encoded, None);
        assert_eq!(ui.pending(RecordingMode::Encoded), None);
        assert_eq!(
            button_label(
                RecordingMode::Rendered,
                &RecordingSnapshot::default(),
                ui.pending(RecordingMode::Rendered),
            ),
            "正在提交停止…"
        );
    }

    #[test]
    fn every_rendered_menu_action_carries_one_explicit_profile() {
        for (action, codec, fps) in [
            (RecordingAction::StartRenderedAvc30, VideoCodec::Avc, 30),
            (RecordingAction::StartRenderedAvc60, VideoCodec::Avc, 60),
            (RecordingAction::StartRenderedHevc30, VideoCodec::Hevc, 30),
            (RecordingAction::StartRenderedHevc60, VideoCodec::Hevc, 60),
        ] {
            assert_eq!(action.request(), RecordingRequest::Rendered { codec, fps });
        }
    }
}
