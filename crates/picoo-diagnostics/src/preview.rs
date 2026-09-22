//! Bounded, pixel-free desktop preview evidence — REQ-PICOO-PRIVACY-003.
//! Counters live for one preview pipeline; each progress point carries its
//! generation so late GPU callbacks cannot masquerade as a newer source.

use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;
use std::sync::{Arc, Mutex};
use std::time::Instant;

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum PreviewStage {
    Poll,
    VisiblePoll,
    SourceAvailable,
    SourceAdmitted,
    ViewportDemand,
    Submitted,
    PendingReplaced,
    PrepareStarted,
    PrepareSucceeded,
    PrepareFailed,
    StaleCompletion,
    SurfacePresented,
    SurfaceRejected,
    DrawAttempted,
    TextureReadAcquired,
    DrawSubmitted,
    DrawBusy,
    DrawFailed,
}

impl PreviewStage {
    const ALL: [Self; 18] = [
        Self::Poll,
        Self::VisiblePoll,
        Self::SourceAvailable,
        Self::SourceAdmitted,
        Self::ViewportDemand,
        Self::Submitted,
        Self::PendingReplaced,
        Self::PrepareStarted,
        Self::PrepareSucceeded,
        Self::PrepareFailed,
        Self::StaleCompletion,
        Self::SurfacePresented,
        Self::SurfaceRejected,
        Self::DrawAttempted,
        Self::TextureReadAcquired,
        Self::DrawSubmitted,
        Self::DrawBusy,
        Self::DrawFailed,
    ];
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct PreviewProgress {
    count: u64,
    last_generation: Option<u64>,
    last_age_ms: Option<u64>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct PreviewFailure {
    stage: PreviewStage,
    generation: u64,
    message: String,
    age_ms: u64,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct DiagnosticPreviewSnapshot {
    scope: String,
    current_generation: u64,
    /// False means draw stages are unobserved, not that no drawing occurred.
    draw_observation_supported: bool,
    stages: BTreeMap<PreviewStage, PreviewProgress>,
    last_error: Option<PreviewFailure>,
}

#[derive(Debug, Default)]
struct Progress {
    count: u64,
    last: Option<(u64, Instant)>,
}

#[derive(Debug)]
struct State {
    current_generation: u64,
    draw_observation_supported: bool,
    stages: BTreeMap<PreviewStage, Progress>,
    last_error: Option<(PreviewStage, u64, String, Instant)>,
}

/// Only short metadata updates hold this lock; never hold it across GPU work.
/// It retains no frames, device handles, paths, peer names or network addresses.
#[derive(Debug, Clone)]
pub struct PreviewDiagnostics(Arc<Mutex<State>>);

impl PreviewDiagnostics {
    pub fn new(draw_observation_supported: bool) -> Self {
        Self(Arc::new(Mutex::new(State {
            current_generation: 0,
            draw_observation_supported,
            stages: PreviewStage::ALL
                .into_iter()
                .map(|s| (s, Progress::default()))
                .collect(),
            last_error: None,
        })))
    }

    pub fn set_generation(&self, generation: u64) {
        self.0
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
            .current_generation = generation;
    }

    pub fn record(&self, stage: PreviewStage, generation: u64) {
        self.record_at(stage, generation, Instant::now());
    }

    fn record_at(&self, stage: PreviewStage, generation: u64, now: Instant) {
        let mut state = self
            .0
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        let progress = state.stages.get_mut(&stage).expect("fixed stage set");
        progress.count = progress.count.saturating_add(1);
        progress.last = Some((generation, now));
    }

    /// Call only with native preview errors, never arbitrary session/user text.
    /// At most one 512-character error is retained. Later success does not erase
    /// the evidence: age and generation distinguish recovered/historical errors.
    pub fn failure(&self, stage: PreviewStage, generation: u64, message: &str) {
        let message = message
            .chars()
            .filter(|c| !c.is_control())
            .take(512)
            .collect();
        let mut state = self
            .0
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        let now = Instant::now();
        let progress = state.stages.get_mut(&stage).expect("fixed stage set");
        progress.count = progress.count.saturating_add(1);
        progress.last = Some((generation, now));
        state.last_error = Some((stage, generation, message, now));
    }

    pub fn snapshot(&self) -> DiagnosticPreviewSnapshot {
        self.snapshot_at(Instant::now())
    }

    fn snapshot_at(&self, now: Instant) -> DiagnosticPreviewSnapshot {
        let state = self
            .0
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        let age = |at| {
            now.saturating_duration_since(at)
                .as_millis()
                .min(u128::from(u64::MAX)) as u64
        };
        DiagnosticPreviewSnapshot {
            scope: "desktop_preview_pipeline_lifetime".into(),
            current_generation: state.current_generation,
            draw_observation_supported: state.draw_observation_supported,
            stages: state
                .stages
                .iter()
                .map(|(stage, progress)| {
                    (
                        *stage,
                        PreviewProgress {
                            count: progress.count,
                            last_generation: progress.last.map(|(generation, _)| generation),
                            last_age_ms: progress.last.map(|(_, at)| age(at)),
                        },
                    )
                })
                .collect(),
            last_error: state
                .last_error
                .as_ref()
                .map(|(stage, generation, message, at)| PreviewFailure {
                    stage: *stage,
                    generation: *generation,
                    message: message.clone(),
                    age_ms: age(*at),
                }),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::time::Duration;

    #[test]
    fn stalled_preparation_and_busy_draw_are_distinct_from_success() {
        let diagnostics = PreviewDiagnostics::new(true);
        let at = Instant::now();
        diagnostics.record_at(PreviewStage::Submitted, 1, at);
        diagnostics.record_at(PreviewStage::PrepareStarted, 1, at);
        let snapshot = diagnostics.snapshot_at(at + Duration::from_secs(5));
        assert_eq!(
            snapshot.stages[&PreviewStage::PrepareStarted].last_age_ms,
            Some(5000)
        );
        assert_eq!(snapshot.stages[&PreviewStage::PrepareSucceeded].count, 0);
        assert_eq!(
            snapshot.stages[&PreviewStage::PrepareSucceeded].last_age_ms,
            None
        );
        diagnostics.record(PreviewStage::DrawAttempted, 1);
        diagnostics.record(PreviewStage::DrawBusy, 1);
        let snapshot = diagnostics.snapshot();
        assert_eq!(snapshot.stages[&PreviewStage::DrawBusy].count, 1);
        assert_eq!(snapshot.stages[&PreviewStage::TextureReadAcquired].count, 0);
        assert_eq!(snapshot.stages[&PreviewStage::DrawSubmitted].count, 0);
    }

    #[test]
    fn late_callback_keeps_old_generation_and_error_is_bounded() {
        let diagnostics = PreviewDiagnostics::new(true);
        diagnostics.set_generation(2);
        diagnostics.failure(PreviewStage::DrawFailed, 1, &"失败\n".repeat(600));
        diagnostics.record(PreviewStage::DrawSubmitted, 2);
        let snapshot = diagnostics.snapshot();
        assert_eq!(snapshot.current_generation, 2);
        assert_eq!(
            snapshot.stages[&PreviewStage::DrawFailed].last_generation,
            Some(1)
        );
        let error = snapshot.last_error.unwrap();
        assert_eq!(error.generation, 1);
        assert_eq!(error.message.chars().count(), 512);
        assert!(!error.message.contains('\n'));
    }

    #[test]
    fn unobserved_platform_drawing_and_concurrent_counts_export_explicitly() {
        let diagnostics = PreviewDiagnostics::new(false);
        std::thread::scope(|scope| {
            for _ in 0..4 {
                let diagnostics = diagnostics.clone();
                scope.spawn(move || {
                    for _ in 0..1000 {
                        diagnostics.record(PreviewStage::PrepareSucceeded, 0);
                    }
                });
            }
        });
        let json = serde_json::to_value(diagnostics.snapshot()).unwrap();
        assert_eq!(json["draw_observation_supported"], false);
        assert_eq!(json["stages"]["prepare_succeeded"]["count"], 4000);
        assert!(json["stages"]["draw_submitted"]["last_age_ms"].is_null());
        assert_eq!(
            json["stages"].as_object().unwrap().len(),
            PreviewStage::ALL.len()
        );
    }
}
