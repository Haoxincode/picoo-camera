//! Desktop recording commands and presentation — REQ-PICOO-MEDIA-075/083.
use std::path::PathBuf;

use super::icons::{reicon_button_content, DesktopIcon};
use super::PicooDesktopApp;
use crate::receiver_runtime::{
    await_receiver_reply, ReceiverSnapshot, RecordingRequest, RecordingSnapshot,
};
use gpui_kit::component::dialog::DialogButtonProps;
use gpui_kit::component::menu::{DropdownMenu, PopupMenu, PopupMenuItem};
use gpui_kit::component::{button::*, *};
use gpui_kit::prelude::FluentBuilder;
use gpui_kit::*;
use picoo_protocol::control::VideoCodec;
use picoo_recording::bundle::{RecordingMode, RecordingState};
use serde::Deserialize;

#[derive(Clone, Action, PartialEq, Eq, Deserialize)]
#[action(namespace = picoo_recording, no_json)]
enum RecordingAction {
    StartEncoded,
    StopEncoded,
    StartRenderedAvc30,
    StartRenderedAvc60,
    StartRenderedHevc30,
    StartRenderedHevc60,
    StopRendered,
}

impl RecordingAction {
    fn start_request(&self) -> Option<RecordingRequest> {
        Some(match self {
            Self::StartEncoded => RecordingRequest::Encoded,
            Self::StartRenderedAvc30 => RecordingRequest::Rendered {
                codec: VideoCodec::Avc,
                fps: 30,
            },
            Self::StartRenderedAvc60 => RecordingRequest::Rendered {
                codec: VideoCodec::Avc,
                fps: 60,
            },
            Self::StartRenderedHevc30 => RecordingRequest::Rendered {
                codec: VideoCodec::Hevc,
                fps: 30,
            },
            Self::StartRenderedHevc60 => RecordingRequest::Rendered {
                codec: VideoCodec::Hevc,
                fps: 60,
            },
            Self::StopEncoded | Self::StopRendered => return None,
        })
    }

    fn stop_mode(&self) -> Option<RecordingMode> {
        match self {
            Self::StopEncoded => Some(RecordingMode::Encoded),
            Self::StopRendered => Some(RecordingMode::Rendered),
            _ => None,
        }
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

fn toolbar_accessibility(
    any_active: bool,
    pending: Option<RecordingPending>,
    stopping: bool,
) -> &'static str {
    match pending {
        Some(RecordingPending::Starting) => return "正在开始录像",
        Some(RecordingPending::Stopping) => return "正在提交停止",
        None => {}
    }
    if stopping {
        "正在结束录像"
    } else if any_active {
        "正在录像"
    } else {
        "录像"
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

    pub(super) fn render_recording_controls(
        &self,
        snapshot: &ReceiverSnapshot,
        cx: &Context<Self>,
    ) -> Option<AnyElement> {
        let recordings = &snapshot.recordings;
        if !recordings.encoded.available && !recordings.rendered.available {
            return None;
        }
        let encoded_available = recordings.encoded.available;
        let rendered_available = recordings.rendered.available;
        let encoded_active = active(&recordings.encoded);
        let rendered_active = active(&recordings.rendered);
        let any_active = encoded_active || rendered_active;
        let encoded_pending = self.recording_ui.pending(RecordingMode::Encoded);
        let rendered_pending = self.recording_ui.pending(RecordingMode::Rendered);
        let pending = encoded_pending.or(rendered_pending);
        let stopping = recordings.encoded.stopping || recordings.rendered.stopping;
        let disconnected = snapshot.stream_config.is_none() || snapshot.active_sender.is_none();
        let unsupported_rendered_source = !recordings.rendered_source_supported;
        let encoded_detail = recording_detail(
            &self.recording_ui,
            RecordingMode::Encoded,
            &recordings.encoded,
        );
        let rendered_detail = recording_detail(
            &self.recording_ui,
            RecordingMode::Rendered,
            &recordings.rendered,
        );
        let encoded_reveal = recordings
            .encoded
            .result
            .as_ref()
            .and_then(|result| result.path.clone());
        let rendered_reveal = recordings
            .rendered
            .result
            .as_ref()
            .and_then(|result| result.path.clone());
        let source_fps = snapshot
            .stream_config
            .as_ref()
            .map(|config| config.fps)
            .unwrap_or(0);
        let can_start = !disconnected;
        let can_start_rendered = can_start && !unsupported_rendered_source;
        let encoded_stopping = recordings.encoded.stopping;
        let rendered_stopping = recordings.rendered.stopping;
        let icon_color = if any_active {
            cx.theme().primary_foreground
        } else {
            cx.theme().primary
        };
        Some(
            div()
                .flex_none()
                .on_action(cx.listener(|this, action: &RecordingAction, _, cx| {
                    if let Some(mode) = action.stop_mode() {
                        this.end_recording(mode, cx);
                    } else if let Some(request) = action.start_request() {
                        this.begin_recording(request, cx);
                    }
                }))
                .child(
                    Button::new("live-recording-toggle")
                        .outline()
                        .small()
                        .selected(any_active)
                        .toggled(any_active)
                        .accessibility_label(toolbar_accessibility(any_active, pending, stopping))
                        .loading(pending.is_some())
                        .disabled(
                            pending.is_some()
                                || (stopping && !any_active)
                                || (!any_active && disconnected),
                        )
                        .tooltip(recording_tooltip(
                            any_active,
                            disconnected,
                            encoded_detail.is_some() || rendered_detail.is_some(),
                            &recordings.encoded,
                            &recordings.rendered,
                        ))
                        .child(reicon_button_content(
                            "录像",
                            DesktopIcon::Recording,
                            icon_color,
                        ))
                        .dropdown_menu_with_anchor(Anchor::TopRight, move |menu, _, _| {
                            let encoded_busy = encoded_pending.is_some() || encoded_stopping;
                            let rendered_busy = rendered_pending.is_some() || rendered_stopping;
                            let menu = if encoded_available {
                                if encoded_active || encoded_stopping {
                                    menu.menu_with_enable(
                                        "停止原码流录像",
                                        Box::new(RecordingAction::StopEncoded),
                                        encoded_active && !encoded_busy,
                                    )
                                } else {
                                    menu.menu_with_enable(
                                        "原码流录像",
                                        Box::new(RecordingAction::StartEncoded),
                                        can_start && !encoded_busy,
                                    )
                                }
                            } else {
                                menu
                            };
                            let menu = if rendered_available {
                                let menu = menu.when(encoded_available, |menu| menu.separator());
                                if rendered_active || rendered_stopping {
                                    menu.menu_with_enable(
                                        "停止处理后录像",
                                        Box::new(RecordingAction::StopRendered),
                                        rendered_active && !rendered_busy,
                                    )
                                } else {
                                    menu.label("处理后录像")
                                        .menu_with_enable(
                                            "H.264 · 30 fps",
                                            Box::new(RecordingAction::StartRenderedAvc30),
                                            can_start_rendered
                                                && !rendered_busy
                                                && source_fps >= 30,
                                        )
                                        .menu_with_enable(
                                            "H.264 · 60 fps",
                                            Box::new(RecordingAction::StartRenderedAvc60),
                                            can_start_rendered
                                                && !rendered_busy
                                                && source_fps >= 60,
                                        )
                                        .separator()
                                        .menu_with_enable(
                                            "HEVC · 30 fps",
                                            Box::new(RecordingAction::StartRenderedHevc30),
                                            can_start_rendered
                                                && !rendered_busy
                                                && source_fps >= 30,
                                        )
                                        .menu_with_enable(
                                            "HEVC · 60 fps",
                                            Box::new(RecordingAction::StartRenderedHevc60),
                                            can_start_rendered
                                                && !rendered_busy
                                                && source_fps >= 60,
                                        )
                                }
                            } else {
                                menu
                            };
                            append_recording_result_items(
                                append_recording_result_items(
                                    menu,
                                    "原码流",
                                    encoded_reveal.clone(),
                                    encoded_detail.clone(),
                                ),
                                "处理后",
                                rendered_reveal.clone(),
                                rendered_detail.clone(),
                            )
                        }),
                )
                .into_any_element(),
        )
    }
}

fn recording_detail(
    ui: &RecordingUiState,
    mode: RecordingMode,
    recording: &RecordingSnapshot,
) -> Option<String> {
    ui.error(mode)
        .map(str::to_owned)
        .or_else(|| {
            recording
                .result
                .as_ref()
                .and_then(|result| result.error.clone())
        })
        .or_else(|| {
            recording.stalled.then(|| {
                "录制工作者超过 15 秒未推进，可能仍在等待系统写入。直播不受影响；文件结果将在写入与清理返回后更新。"
                    .to_owned()
            })
        })
}

fn recording_tooltip(
    any_active: bool,
    disconnected: bool,
    has_error: bool,
    encoded: &RecordingSnapshot,
    rendered: &RecordingSnapshot,
) -> &'static str {
    if disconnected && !any_active {
        "连接手机并准备视频后可录制"
    } else if has_error {
        "录像操作未完成"
    } else if encoded.stalled || encoded.stopping || encoded.state.is_some() {
        status(RecordingMode::Encoded, encoded)
    } else if rendered.stalled || rendered.stopping || rendered.state.is_some() {
        status(RecordingMode::Rendered, rendered)
    } else {
        "保存原码流或处理后画面"
    }
}

fn append_recording_result_items(
    menu: PopupMenu,
    kind: &'static str,
    reveal: Option<PathBuf>,
    detail: Option<String>,
) -> PopupMenu {
    menu.when(reveal.is_some() || detail.is_some(), |menu| {
        menu.separator()
    })
    .when_some(detail, |menu, error| {
        menu.item(
            PopupMenuItem::new(format!("查看{kind}原因")).on_click(move |_, window, cx| {
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
    .when_some(reveal, |menu, path| {
        menu.item(
            PopupMenuItem::new(format!("打开{kind}文件夹"))
                .on_click(move |_, _, cx| cx.reveal_path(&path)),
        )
    })
}

#[cfg(test)]
mod tests {
    use super::{
        active, status, toolbar_accessibility, RecordingAction, RecordingMode, RecordingPending,
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
    fn live_toolbar_uses_a_single_recording_label() {
        assert_eq!(toolbar_accessibility(false, None, false), "录像");
        assert_eq!(toolbar_accessibility(true, None, false), "正在录像");

        let mut ui = RecordingUiState::default();
        ui.set_pending(RecordingMode::Encoded, Some(RecordingPending::Starting));
        assert_eq!(
            ui.pending(RecordingMode::Encoded),
            Some(RecordingPending::Starting)
        );
        assert_eq!(ui.pending(RecordingMode::Rendered), None);
        assert_eq!(
            toolbar_accessibility(false, ui.pending(RecordingMode::Encoded), false),
            "正在开始录像"
        );

        ui.set_pending(RecordingMode::Rendered, Some(RecordingPending::Stopping));
        ui.set_pending(RecordingMode::Encoded, None);
        assert_eq!(ui.pending(RecordingMode::Encoded), None);
        assert_eq!(
            toolbar_accessibility(true, ui.pending(RecordingMode::Rendered), false),
            "正在提交停止"
        );
    }

    #[test]
    fn every_start_menu_action_carries_one_explicit_request() {
        assert_eq!(
            RecordingAction::StartEncoded.start_request(),
            Some(RecordingRequest::Encoded)
        );
        assert_eq!(
            RecordingAction::StopEncoded.stop_mode(),
            Some(RecordingMode::Encoded)
        );
        assert_eq!(
            RecordingAction::StopRendered.stop_mode(),
            Some(RecordingMode::Rendered)
        );
        assert_eq!(RecordingAction::StopEncoded.start_request(), None);
        for (action, codec, fps) in [
            (RecordingAction::StartRenderedAvc30, VideoCodec::Avc, 30),
            (RecordingAction::StartRenderedAvc60, VideoCodec::Avc, 60),
            (RecordingAction::StartRenderedHevc30, VideoCodec::Hevc, 30),
            (RecordingAction::StartRenderedHevc60, VideoCodec::Hevc, 60),
        ] {
            assert_eq!(
                action.start_request(),
                Some(RecordingRequest::Rendered { codec, fps })
            );
        }
    }

    #[test]
    fn recording_controls_are_not_mounted_on_workspace_chrome() {
        const LIFECYCLE_SOURCE: &str = include_str!("lifecycle.rs");
        const CONNECT_SOURCE: &str = include_str!("connect.rs");
        assert!(
            !LIFECYCLE_SOURCE.contains("render_recording"),
            "recording must not sit under the section title"
        );
        assert!(CONNECT_SOURCE.contains("render_recording_controls"));
        let waiting = CONNECT_SOURCE
            .lines()
            .skip_while(|line| !line.contains("fn render_waiting"))
            .take_while(|line| !line.contains("fn render_manual_endpoint_card"))
            .collect::<Vec<_>>()
            .join("\n");
        assert!(!waiting.contains("render_recording"));
    }
}
