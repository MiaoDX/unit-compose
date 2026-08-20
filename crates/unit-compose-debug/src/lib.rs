//! Optional inspection adapter boundary with explicit failure and allocation policy.

use std::fmt;

use unit_compose_core::{DiagnosticSink, FixedModuleDescription, RunEvent, RunReportSnapshot};

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct AdapterDescriptor {
    pub name: &'static str,
    pub allocation_domains: &'static [&'static str],
    pub overhead: &'static str,
}

pub trait InspectionAdapter {
    type Error: fmt::Display;

    fn descriptor(&self) -> AdapterDescriptor;
    fn fixed_description(
        &mut self,
        description: &FixedModuleDescription,
    ) -> Result<(), Self::Error>;
    fn run_snapshot(&mut self, report: &RunReportSnapshot) -> Result<(), Self::Error>;
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum AdapterFailurePolicy {
    /// Return a separate diagnostics error. Module state and algorithm result are untouched.
    Report,
    /// Record the error and permanently disable subsequent adapter calls.
    Disable,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub enum AdapterError {
    Failed {
        adapter: &'static str,
        message: String,
    },
}

impl fmt::Display for AdapterError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Failed { adapter, message } => {
                write!(formatter, "adapter {adapter} failed: {message}")
            }
        }
    }
}

impl std::error::Error for AdapterError {}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum AdapterOutcome {
    Delivered,
    Disabled,
    DisabledAfterFailure,
}

pub struct AdapterController<A> {
    adapter: A,
    policy: AdapterFailurePolicy,
    enabled: bool,
    last_error: Option<AdapterError>,
}

impl<A: InspectionAdapter> AdapterController<A> {
    #[must_use]
    pub const fn new(adapter: A, policy: AdapterFailurePolicy) -> Self {
        Self {
            adapter,
            policy,
            enabled: true,
            last_error: None,
        }
    }

    #[must_use]
    pub const fn is_enabled(&self) -> bool {
        self.enabled
    }

    #[must_use]
    pub const fn last_error(&self) -> Option<&AdapterError> {
        self.last_error.as_ref()
    }

    pub fn disable(&mut self) {
        self.enabled = false;
    }

    pub fn fixed_description(
        &mut self,
        description: &FixedModuleDescription,
    ) -> Result<AdapterOutcome, AdapterError> {
        if !self.enabled {
            return Ok(AdapterOutcome::Disabled);
        }
        let name = self.adapter.descriptor().name;
        match self.adapter.fixed_description(description) {
            Ok(()) => Ok(AdapterOutcome::Delivered),
            Err(error) => self.handle_failure(name, error),
        }
    }

    pub fn run_snapshot(
        &mut self,
        report: &RunReportSnapshot,
    ) -> Result<AdapterOutcome, AdapterError> {
        if !self.enabled {
            return Ok(AdapterOutcome::Disabled);
        }
        let name = self.adapter.descriptor().name;
        match self.adapter.run_snapshot(report) {
            Ok(()) => Ok(AdapterOutcome::Delivered),
            Err(error) => self.handle_failure(name, error),
        }
    }

    fn handle_failure(
        &mut self,
        adapter: &'static str,
        error: A::Error,
    ) -> Result<AdapterOutcome, AdapterError> {
        let error = AdapterError::Failed {
            adapter,
            message: error.to_string(),
        };
        match self.policy {
            AdapterFailurePolicy::Report => Err(error),
            AdapterFailurePolicy::Disable => {
                self.enabled = false;
                self.last_error = Some(error);
                Ok(AdapterOutcome::DisabledAfterFailure)
            }
        }
    }
}

/// Allocation-free event sink with deterministic reject-and-count overflow behavior.
pub struct BoundedRunSink<const N: usize> {
    events: [Option<RunEvent>; N],
    len: usize,
    dropped: usize,
    last_record_retained: bool,
}

impl<const N: usize> Default for BoundedRunSink<N> {
    fn default() -> Self {
        Self {
            events: [None; N],
            len: 0,
            dropped: 0,
            last_record_retained: false,
        }
    }
}

impl<const N: usize> BoundedRunSink<N> {
    pub fn events(&self) -> impl Iterator<Item = &RunEvent> {
        self.events[..self.len].iter().flatten()
    }

    #[must_use]
    pub const fn dropped_events(&self) -> usize {
        self.dropped
    }

    pub fn clear(&mut self) {
        self.events[..self.len].fill(None);
        self.len = 0;
        self.dropped = 0;
        self.last_record_retained = false;
    }
}

impl<const N: usize> DiagnosticSink for BoundedRunSink<N> {
    fn record(&mut self, event: RunEvent) {
        if self.len < N {
            self.events[self.len] = Some(event);
            self.len += 1;
            self.last_record_retained = true;
        } else {
            self.dropped = self.dropped.saturating_add(1);
            self.last_record_retained = false;
        }
    }

    fn correct_last(&mut self, event: RunEvent) {
        if self.last_record_retained {
            self.events[self.len - 1] = Some(event);
        }
    }
}

#[cfg(test)]
mod tests {
    use std::fmt;
    use std::time::Duration;

    use unit_compose_core::{
        DiagnosticSink, FixedModuleDescription, RunEventKind, RunReportSnapshot, TimingOverhead,
        TimingScope,
    };

    use super::{
        AdapterController, AdapterDescriptor, AdapterFailurePolicy, AdapterOutcome, BoundedRunSink,
        InspectionAdapter,
    };

    fn event(capacity: usize) -> unit_compose_core::RunEvent {
        unit_compose_core::RunEvent {
            kind: RunEventKind::Success,
            observed_capacity: capacity,
            elapsed: Duration::ZERO,
            timing_scope: TimingScope::ModuleExecution,
            timing_overhead: TimingOverhead {
                clock_reads: 2,
                bounded_report_write_in_elapsed: false,
            },
        }
    }

    #[test]
    fn bounded_sink_keeps_prefix_and_counts_every_drop() {
        let mut sink = BoundedRunSink::<2>::default();
        for capacity in 0..5 {
            sink.record(event(capacity));
        }
        assert_eq!(
            sink.events()
                .map(|event| event.observed_capacity)
                .collect::<Vec<_>>(),
            [0, 1]
        );
        assert_eq!(sink.dropped_events(), 3);
        sink.clear();
        assert_eq!(sink.events().count(), 0);
        assert_eq!(sink.dropped_events(), 0);
    }

    #[test]
    fn bounded_sink_only_corrects_an_event_retained_for_the_current_run() {
        let mut retained = BoundedRunSink::<1>::default();
        retained.record(event(1));
        retained.correct_last(event(3));
        assert_eq!(
            retained
                .events()
                .map(|event| event.observed_capacity)
                .collect::<Vec<_>>(),
            [3]
        );
        assert_eq!(retained.dropped_events(), 0);

        let mut sink = BoundedRunSink::<1>::default();
        sink.record(event(1));
        sink.record(event(2));
        sink.correct_last(event(3));
        assert_eq!(
            sink.events()
                .map(|event| event.observed_capacity)
                .collect::<Vec<_>>(),
            [1]
        );
        assert_eq!(sink.dropped_events(), 1);

        let mut empty = BoundedRunSink::<0>::default();
        empty.record(event(1));
        empty.correct_last(event(2));
        assert_eq!(empty.events().count(), 0);
        assert_eq!(empty.dropped_events(), 1);
    }

    struct Fails {}

    #[derive(Clone, Copy, Debug)]
    struct Failure;

    impl fmt::Display for Failure {
        fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
            formatter.write_str("expected failure")
        }
    }

    impl InspectionAdapter for Fails {
        type Error = Failure;

        fn descriptor(&self) -> AdapterDescriptor {
            AdapterDescriptor {
                name: "fails",
                allocation_domains: &["rust-global"],
                overhead: "test adapter",
            }
        }

        fn fixed_description(&mut self, _: &FixedModuleDescription) -> Result<(), Self::Error> {
            Err(Failure)
        }

        fn run_snapshot(&mut self, _: &RunReportSnapshot) -> Result<(), Self::Error> {
            Err(Failure)
        }
    }

    fn snapshot() -> RunReportSnapshot {
        RunReportSnapshot::default()
    }

    #[test]
    fn failure_policy_is_separate_or_disables_adapter() {
        let report = snapshot();
        let mut reporting = AdapterController::new(Fails {}, AdapterFailurePolicy::Report);
        assert!(reporting.run_snapshot(&report).is_err());
        assert!(reporting.is_enabled());

        let mut disabling = AdapterController::new(Fails {}, AdapterFailurePolicy::Disable);
        assert_eq!(
            disabling.run_snapshot(&report).unwrap(),
            AdapterOutcome::DisabledAfterFailure
        );
        assert!(!disabling.is_enabled());
        assert!(disabling.last_error().is_some());
        assert_eq!(
            disabling.run_snapshot(&report).unwrap(),
            AdapterOutcome::Disabled
        );
    }
}
