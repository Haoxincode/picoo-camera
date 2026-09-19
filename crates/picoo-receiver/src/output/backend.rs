//! Output backend selection and generation gate.
//!
//! The selector is deliberately independent from platform resources. A
//! platform adapter must prove that its native interop is available before it
//! advertises `GpuNative`; a runtime failure is not an implicit permission to
//! hide a GPU/codec failure behind a CPU fallback.

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum OutputBackend {
    GpuNative,
    CpuBridge,
}

/// A native probe failure is evidence for an explicit CpuBridge decision. It
/// is not a per-frame retry signal and must not be used to hide source/GPU
/// loss after a plan has been committed.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum BackendFailureReason {
    NativeImportUnsupported,
    AdapterMismatch,
    NativeAllocatorUnavailable,
    SharingUnavailable,
    UnauthorizedProducer,
    DeviceLost,
    InvalidContract,
}

impl BackendFailureReason {
    pub(crate) const fn allows_cpu_bridge(self) -> bool {
        matches!(
            self,
            Self::NativeImportUnsupported
                | Self::AdapterMismatch
                | Self::NativeAllocatorUnavailable
                | Self::SharingUnavailable
        )
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) struct BackendCapabilities {
    pub(crate) gpu_native: bool,
    pub(crate) cpu_bridge: bool,
}

impl BackendCapabilities {
    pub(crate) const fn cpu_bridge_only() -> Self {
        Self {
            gpu_native: false,
            cpu_bridge: true,
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) struct BackendSelection {
    pub(crate) backend: OutputBackend,
    pub(crate) reason: BackendSelectionReason,
    native_failure: Option<BackendFailureReason>,
}

impl BackendSelection {
    pub(crate) const fn native_failure(self) -> Option<BackendFailureReason> {
        self.native_failure
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum BackendSelectionReason {
    NativeInteropAvailable,
    NativeInteropUnavailable,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) struct BackendGeneration(u64);

impl BackendGeneration {
    pub(crate) const fn first() -> Self {
        Self(1)
    }

    pub(crate) const fn get(self) -> u64 {
        self.0
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum BackendSelectionError {
    NoSupportedBackend,
    GenerationExhausted,
    NativeProbeContradiction,
    NativeFailureNotFallbackable(BackendFailureReason),
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum BackendTransition {
    Unchanged(BackendSelection),
    Switched {
        from: BackendSelection,
        to: BackendSelection,
        generation: BackendGeneration,
    },
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) struct BackendState {
    selection: BackendSelection,
    generation: BackendGeneration,
}

/// Immutable sink plan captured by a worker before doing platform work.
/// A completed result must be checked against the current state before it is
/// published; a backend switch invalidates the old plan by generation.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) struct OutputPlan {
    selection: BackendSelection,
    generation: BackendGeneration,
    source_generation: u64,
    output_revision: u64,
}

impl OutputPlan {
    pub(crate) const fn selection(self) -> BackendSelection {
        self.selection
    }

    pub(crate) const fn generation(self) -> BackendGeneration {
        self.generation
    }

    pub(crate) const fn source_generation(self) -> u64 {
        self.source_generation
    }

    pub(crate) const fn output_revision(self) -> u64 {
        self.output_revision
    }
}

impl BackendState {
    pub(crate) fn auto(capabilities: BackendCapabilities) -> Result<Self, BackendSelectionError> {
        Self::auto_with_native_failure(capabilities, None)
    }

    pub(crate) fn auto_with_native_failure(
        capabilities: BackendCapabilities,
        native_failure: Option<BackendFailureReason>,
    ) -> Result<Self, BackendSelectionError> {
        Ok(Self {
            selection: select_auto(capabilities, native_failure)?,
            generation: BackendGeneration::first(),
        })
    }

    pub(crate) const fn selection(self) -> BackendSelection {
        self.selection
    }

    pub(crate) const fn generation(self) -> BackendGeneration {
        self.generation
    }

    pub(crate) const fn plan_for(self, source_generation: u64, output_revision: u64) -> OutputPlan {
        OutputPlan {
            selection: self.selection,
            generation: self.generation,
            source_generation,
            output_revision,
        }
    }

    pub(crate) const fn native_failure(self) -> Option<BackendFailureReason> {
        self.selection.native_failure
    }

    /// Re-evaluate the sink backend at a transaction boundary.
    ///
    /// A changed backend always receives a new generation. Work submitted
    /// under the previous generation must be rejected by the adapter before it
    /// can publish a system sample.
    pub(crate) fn transition_auto(
        &mut self,
        capabilities: BackendCapabilities,
    ) -> Result<BackendTransition, BackendSelectionError> {
        self.transition_auto_with_native_failure(capabilities, None)
    }

    pub(crate) fn transition_auto_with_native_failure(
        &mut self,
        capabilities: BackendCapabilities,
        native_failure: Option<BackendFailureReason>,
    ) -> Result<BackendTransition, BackendSelectionError> {
        let next = select_auto(capabilities, native_failure)?;
        if next.backend == self.selection.backend {
            // Diagnostics may become more precise while the active sink stays
            // on the same backend. Do not invalidate in-flight work merely
            // because the explanatory reason changed.
            self.selection = next;
            return Ok(BackendTransition::Unchanged(next));
        }
        let generation = BackendGeneration(
            self.generation
                .0
                .checked_add(1)
                .ok_or(BackendSelectionError::GenerationExhausted)?,
        );
        let from = self.selection;
        self.selection = next;
        self.generation = generation;
        Ok(BackendTransition::Switched {
            from,
            to: next,
            generation,
        })
    }

    pub(crate) fn accepts(
        self,
        selection: BackendSelection,
        generation: BackendGeneration,
    ) -> bool {
        self.selection.backend == selection.backend && self.generation == generation
    }

    pub(crate) fn accepts_plan(
        self,
        plan: OutputPlan,
        source_generation: u64,
        output_revision: u64,
    ) -> bool {
        self.selection.backend == plan.selection.backend
            && self.generation == plan.generation
            && plan.source_generation == source_generation
            && plan.output_revision == output_revision
    }
}

fn select_auto(
    capabilities: BackendCapabilities,
    native_failure: Option<BackendFailureReason>,
) -> Result<BackendSelection, BackendSelectionError> {
    if capabilities.gpu_native && native_failure.is_some() {
        return Err(BackendSelectionError::NativeProbeContradiction);
    }
    if capabilities.gpu_native {
        return Ok(BackendSelection {
            backend: OutputBackend::GpuNative,
            reason: BackendSelectionReason::NativeInteropAvailable,
            native_failure: None,
        });
    }
    if let Some(failure) = native_failure {
        if !failure.allows_cpu_bridge() {
            return Err(BackendSelectionError::NativeFailureNotFallbackable(failure));
        }
    }
    if capabilities.cpu_bridge {
        return Ok(BackendSelection {
            backend: OutputBackend::CpuBridge,
            reason: BackendSelectionReason::NativeInteropUnavailable,
            native_failure,
        });
    }
    Err(BackendSelectionError::NoSupportedBackend)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn auto_prefers_native_interop_when_proven_available() {
        let state = BackendState::auto(BackendCapabilities {
            gpu_native: true,
            cpu_bridge: true,
        })
        .expect("backend");
        assert_eq!(state.selection().backend, OutputBackend::GpuNative);
        assert_eq!(
            state.selection().reason,
            BackendSelectionReason::NativeInteropAvailable
        );
    }

    #[test]
    fn cpu_bridge_is_selected_only_when_native_interop_is_unavailable() {
        let state = BackendState::auto(BackendCapabilities::cpu_bridge_only()).expect("backend");
        assert_eq!(state.selection().backend, OutputBackend::CpuBridge);
        assert_eq!(
            state.selection().reason,
            BackendSelectionReason::NativeInteropUnavailable
        );
    }

    #[test]
    fn backend_switch_advances_generation_and_rejects_old_work() {
        let mut state =
            BackendState::auto(BackendCapabilities::cpu_bridge_only()).expect("backend");
        let old_selection = state.selection();
        let old_generation = state.generation();
        let transition = state
            .transition_auto(BackendCapabilities {
                gpu_native: true,
                cpu_bridge: true,
            })
            .expect("switch");
        let new_selection = state.selection();
        let new_generation = state.generation();
        assert!(matches!(transition, BackendTransition::Switched { .. }));
        assert_eq!(new_generation.get(), old_generation.get() + 1);
        assert!(!state.accepts(old_selection, old_generation));
        assert!(state.accepts(new_selection, new_generation));
    }

    #[test]
    fn same_backend_does_not_reset_generation() {
        let mut state =
            BackendState::auto(BackendCapabilities::cpu_bridge_only()).expect("backend");
        let generation = state.generation();
        assert!(matches!(
            state
                .transition_auto(BackendCapabilities::cpu_bridge_only())
                .expect("same backend"),
            BackendTransition::Unchanged(_)
        ));
        assert_eq!(state.generation(), generation);
    }

    #[test]
    fn same_cpu_backend_can_refresh_reason_without_new_generation() {
        let mut state = BackendState::auto_with_native_failure(
            BackendCapabilities::cpu_bridge_only(),
            Some(BackendFailureReason::AdapterMismatch),
        )
        .expect("backend");
        let generation = state.generation();
        let plan = state.plan_for(7, 11);
        let transition = state
            .transition_auto(BackendCapabilities::cpu_bridge_only())
            .expect("same backend");
        assert!(matches!(transition, BackendTransition::Unchanged(_)));
        assert_eq!(state.generation(), generation);
        assert_eq!(state.native_failure(), None);
        assert!(state.accepts_plan(plan, 7, 11));
    }

    #[test]
    fn cpu_bridge_plan_retains_explicit_native_probe_failure() {
        let state = BackendState::auto_with_native_failure(
            BackendCapabilities::cpu_bridge_only(),
            Some(BackendFailureReason::AdapterMismatch),
        )
        .expect("backend");
        assert_eq!(
            state.native_failure(),
            Some(BackendFailureReason::AdapterMismatch)
        );
        let plan = state.plan_for(7, 11);
        assert!(state.accepts_plan(plan, 7, 11));
        assert!(!state.accepts_plan(plan, 8, 11));
        assert!(!state.accepts_plan(plan, 7, 12));
        assert_eq!(plan.selection().native_failure(), state.native_failure());
        assert_eq!(plan.generation(), state.generation());
        assert_eq!(plan.source_generation(), 7);
        assert_eq!(plan.output_revision(), 11);
    }

    #[test]
    fn native_failure_reasons_are_explicit_and_finite() {
        let reasons = [
            BackendFailureReason::NativeImportUnsupported,
            BackendFailureReason::AdapterMismatch,
            BackendFailureReason::NativeAllocatorUnavailable,
            BackendFailureReason::SharingUnavailable,
            BackendFailureReason::UnauthorizedProducer,
            BackendFailureReason::DeviceLost,
            BackendFailureReason::InvalidContract,
        ];
        assert_eq!(reasons.len(), 7);
        assert!(BackendFailureReason::AdapterMismatch.allows_cpu_bridge());
        assert!(!BackendFailureReason::UnauthorizedProducer.allows_cpu_bridge());
        assert!(!BackendFailureReason::DeviceLost.allows_cpu_bridge());
        assert!(!BackendFailureReason::InvalidContract.allows_cpu_bridge());
    }

    #[test]
    fn hard_native_failures_cannot_be_hidden_by_cpu_bridge() {
        for failure in [
            BackendFailureReason::UnauthorizedProducer,
            BackendFailureReason::DeviceLost,
            BackendFailureReason::InvalidContract,
        ] {
            assert_eq!(
                BackendState::auto_with_native_failure(
                    BackendCapabilities::cpu_bridge_only(),
                    Some(failure),
                ),
                Err(BackendSelectionError::NativeFailureNotFallbackable(failure))
            );
        }
    }

    #[test]
    fn native_probe_cannot_claim_available_and_failed_at_once() {
        assert_eq!(
            BackendState::auto_with_native_failure(
                BackendCapabilities {
                    gpu_native: true,
                    cpu_bridge: true,
                },
                Some(BackendFailureReason::SharingUnavailable),
            ),
            Err(BackendSelectionError::NativeProbeContradiction)
        );
    }

    #[test]
    fn no_backend_is_an_explicit_error() {
        assert_eq!(
            BackendState::auto(BackendCapabilities {
                gpu_native: false,
                cpu_bridge: false,
            }),
            Err(BackendSelectionError::NoSupportedBackend)
        );
    }
}
