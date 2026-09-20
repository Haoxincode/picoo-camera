//! Run blocking OS work off the GPUI executors.
//!
//! `BackgroundExecutor::spawn_dedicated` currently aborts the desktop process
//! when its typed task is polled (`core::option::expect_failed` inside
//! `scheduler::executor::Task::poll`). Camera Extension detection hits that
//! path about two seconds after launch.

use gpui_kit::{Context, Task};

pub(super) fn spawn_os_thread<V: 'static, T, F>(cx: &Context<V>, work: F) -> Task<Result<T, String>>
where
    T: Send + 'static,
    F: FnOnce() -> T + Send + 'static,
{
    let (tx, rx) = futures_channel::oneshot::channel();
    if let Err(err) = std::thread::Builder::new()
        .name("picoo-os-thread".into())
        .spawn(move || {
            let _ = tx.send(work());
        })
    {
        return cx
            .background_executor()
            .spawn(async move { Err(format!("无法启动后台线程：{err}")) });
    }
    cx.background_executor()
        .spawn(async move { rx.await.map_err(|_| "后台任务已中断".into()) })
}
