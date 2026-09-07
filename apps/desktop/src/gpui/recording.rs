//! Desktop recording commands and presentation — REQ-PICOO-MEDIA-075.
use super::PicooDesktopApp;
use crate::receiver_runtime::{await_receiver_reply, ReceiverSnapshot, RecordingSnapshot};
use gpui_kit::component::dialog::DialogButtonProps;
use gpui_kit::component::{button::*, *};
use gpui_kit::prelude::FluentBuilder;
use gpui_kit::*;
use picoo_recording::bundle::RecordingState;

fn active(snapshot: &RecordingSnapshot) -> bool {
    snapshot.result.is_none()
        && matches!(
            snapshot.state,
            Some(RecordingState::Arming | RecordingState::Recording)
        )
}
fn status(snapshot: &RecordingSnapshot) -> &'static str {
    if snapshot.stalled && snapshot.result.is_none() {
        return "录像写入无响应";
    }
    if snapshot.stopping {
        return "正在结束录像";
    }
    match snapshot.state {
        None => "保存接收到的原始画面",
        Some(RecordingState::Arming) => "正在准备录制",
        Some(RecordingState::Recording) => "正在录制",
        Some(RecordingState::Complete) => "录像已保存",
        Some(RecordingState::HasGaps) => "录像已保存，含画面缺口",
        Some(RecordingState::Failed) => "录像未完成",
    }
}

impl PicooDesktopApp {
    fn begin_recording(&mut self, cx: &mut Context<Self>) {
        if self.recording_command_pending {
            return;
        }
        self.recording_command_pending = true;
        self.recording_error = None;
        let selection = cx.prompt_for_paths(PathPromptOptions {
            files: false,
            directories: true,
            multiple: false,
            prompt: Some("选择录像保存文件夹".into()),
        });
        cx.spawn(async move |this, cx| {
            let selected = match selection.await {
                Ok(Ok(paths)) => Ok(paths.and_then(|paths| paths.into_iter().next())),
                Ok(Err(error)) => Err(error.to_string()),
                Err(error) => Err(error.to_string()),
            };
            let result = match selected {
                Ok(Some(path)) => {
                    match this.update(cx, |this, _| this.runtime.start_recording(path)) {
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
                this.recording_command_pending = false;
                this.recording_error = result.err().map(|error| format!("无法开始录像：{error}"));
                cx.notify();
            });
        })
        .detach();
        cx.notify();
    }

    fn end_recording(&mut self, cx: &mut Context<Self>) {
        if self.recording_command_pending {
            return;
        }
        self.recording_command_pending = true;
        self.recording_error = None;
        let reply = self.runtime.stop_recording();
        cx.spawn(async move |this, cx| {
            let result = await_receiver_reply(reply).await;
            let _ = this.update(cx, |this, cx| {
                this.recording_command_pending = false;
                this.recording_error = result.err().map(|error| format!("无法停止录像：{error}"));
                cx.notify();
            });
        })
        .detach();
        cx.notify();
    }

    pub(super) fn render_recording_bar(
        &self,
        snapshot: &ReceiverSnapshot,
        cx: &Context<Self>,
    ) -> AnyElement {
        let recording = &snapshot.recording;
        if !recording.available {
            return div().into_any_element();
        }
        let is_active = active(recording);
        let is_stopping = recording.stopping;
        let unavailable = snapshot.stream_config.is_none() || snapshot.active_sender.is_none();
        let label = if is_stopping {
            "正在结束录像"
        } else if is_active {
            "停止录像"
        } else {
            "开始原码流录像…"
        };
        let detail = self.recording_error.clone().or_else(|| {
            recording
                .result
                .as_ref()
                .and_then(|result| result.error.clone())
        }).or_else(|| recording.stalled.then(||
            "录制工作者超过 15 秒未推进，可能仍在等待系统写入。直播不受影响；文件结果将在写入与清理返回后更新。".to_owned()
        ));
        let message = if self.recording_error.is_some() {
            "录像操作未完成"
        } else {
            status(recording)
        };
        let reveal = recording
            .result
            .as_ref()
            .and_then(|result| result.path.clone());
        div()
            .v_flex()
            .flex_shrink_0()
            .gap_1()
            .px_3()
            .py_2()
            .border_b_1()
            .border_color(cx.theme().border)
            .child(
                div()
                    .h_flex()
                    .items_center()
                    .gap_3()
                    .child(
                        Button::new("encoded-recording-toggle")
                            .outline()
                            .small()
                            .label(label)
                            .accessibility_label(label)
                            .disabled(
                                self.recording_command_pending
                                    || is_stopping
                                    || (!is_active && unavailable),
                            )
                            .tooltip(if !is_active && unavailable {
                                "连接手机并准备视频后可录制"
                            } else {
                                "录像不受预览窗口大小和页面切换影响"
                            })
                            .on_click(cx.listener(move |this, _, _, cx| {
                                if is_active {
                                    this.end_recording(cx);
                                } else {
                                    this.begin_recording(cx);
                                }
                            })),
                    )
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
                            Button::new("recording-error-details")
                                .ghost()
                                .small()
                                .label("查看原因")
                                .on_click(move |_, window, cx| {
                                    let error = error.clone();
                                    window.open_alert_dialog(cx, move |alert, _, _| {
                                        alert
                                            .title("录像详情")
                                            .description(error.clone())
                                            .button_props(
                                                DialogButtonProps::default().ok_text("关闭"),
                                            )
                                    });
                                }),
                        )
                    })
                    .when_some(reveal, |row, path| {
                        row.child(
                            Button::new("reveal-recording")
                                .ghost()
                                .small()
                                .label("打开文件夹")
                                .on_click(move |_, _, cx| cx.reveal_path(&path)),
                        )
                    }),
            )
            .into_any_element()
    }
}

#[cfg(test)]
mod tests {
    use super::{active, status, RecordingSnapshot, RecordingState};
    #[test]
    fn preparing_finalizing_and_gap_results_are_not_presented_as_complete() {
        let mut snapshot = RecordingSnapshot {
            available: true,
            state: Some(RecordingState::Arming),
            ..Default::default()
        };
        assert!(active(&snapshot));
        assert_eq!(status(&snapshot), "正在准备录制");
        snapshot.stopping = true;
        assert_eq!(status(&snapshot), "正在结束录像");
        snapshot.stalled = true;
        assert_eq!(status(&snapshot), "录像写入无响应");
        snapshot.stalled = false;
        snapshot.stopping = false;
        snapshot.state = Some(RecordingState::HasGaps);
        assert!(!active(&snapshot));
        assert_eq!(status(&snapshot), "录像已保存，含画面缺口");
        snapshot.state = Some(RecordingState::Failed);
        assert_eq!(status(&snapshot), "录像未完成");
    }
}
