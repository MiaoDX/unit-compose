use std::collections::BTreeMap;
use std::fmt::Write;

use crate::{
    CompiledGraph, PreparedModuleDescription, ResourceId, ResourceRequirement, RunReportSnapshot,
    StorageReport, UnitId,
};

/// Host-provided normalized configuration text for one Unit.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct UnitConfigurationSummary {
    pub unit: UnitId,
    pub summary: String,
}

/// Fixed workspace requirement resolved during Module preparation.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct UnitWorkspaceDescription {
    pub unit: UnitId,
    pub bytes: usize,
}

/// Immutable, owned description of a prepared Module.
///
/// It is assembled after validation and contains no mutable execution state.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct FixedModuleDescription {
    pub graph: CompiledGraph,
    pub configurations: Vec<UnitConfigurationSummary>,
    pub requirements: BTreeMap<ResourceId, ResourceRequirement>,
    pub workspaces: Vec<UnitWorkspaceDescription>,
    pub storage: StorageReport,
    pub prepared: PreparedModuleDescription,
}

impl FixedModuleDescription {
    #[must_use]
    pub fn new(
        graph: CompiledGraph,
        mut configurations: Vec<UnitConfigurationSummary>,
        requirements: BTreeMap<ResourceId, ResourceRequirement>,
        workspace_bytes: BTreeMap<UnitId, usize>,
        storage: StorageReport,
        prepared: PreparedModuleDescription,
    ) -> Self {
        configurations.sort_by(|left, right| left.unit.cmp(&right.unit));
        let workspaces = workspace_bytes
            .into_iter()
            .map(|(unit, bytes)| UnitWorkspaceDescription { unit, bytes })
            .collect();
        Self {
            graph,
            configurations,
            requirements,
            workspaces,
            storage,
            prepared,
        }
    }

    /// Complete stable text view, including requirements and storage evidence.
    #[must_use]
    pub fn to_text(&self) -> String {
        let mut output = self.graph.to_text();
        for unit in &self.graph.units {
            writeln!(
                output,
                "unit {}: {}; dependencies: [{}]",
                unit.id.as_str(),
                unit.unit_type.as_str(),
                unit.dependencies
                    .iter()
                    .map(UnitId::as_str)
                    .collect::<Vec<_>>()
                    .join(", ")
            )
            .expect("String writes cannot fail");
        }
        for config in &self.configurations {
            writeln!(
                output,
                "config {}: {}",
                config.unit.as_str(),
                config.summary
            )
            .expect("String writes cannot fail");
        }
        for (resource, requirement) in &self.requirements {
            writeln!(
                output,
                "requirement {}: capacity={}",
                resource.as_str(),
                requirement.capacity
            )
            .expect("String writes cannot fail");
        }
        for workspace in &self.workspaces {
            writeln!(
                output,
                "workspace {}: bytes={}",
                workspace.unit.as_str(),
                workspace.bytes
            )
            .expect("String writes cannot fail");
        }
        for assignment in &self.storage.assignments {
            writeln!(
                output,
                "storage {}: slot={} live={}..={} capacity={} bytes={}",
                assignment.resource.as_str(),
                assignment.slot,
                assignment.live_range.start,
                assignment.live_range.end,
                assignment.capacity,
                assignment.bytes
            )
            .expect("String writes cannot fail");
        }
        writeln!(
            output,
            "storage peak: slots={} estimated_bytes={}",
            self.storage.slot_count, self.storage.estimated_peak_bytes
        )
        .expect("String writes cannot fail");
        writeln!(
            output,
            "allocation: guarantee={:?} requirement={:?} trusted_declarations={}",
            self.prepared.options.allocation_guarantee(),
            self.prepared.requirement_status,
            self.prepared
                .allocation_capability
                .declarations_are_trusted()
        )
        .expect("String writes cannot fail");
        for domain in self.prepared.allocation_capability.domains() {
            writeln!(
                output,
                "allocation domain {}: {:?}",
                domain.name, domain.evidence
            )
            .expect("String writes cannot fail");
        }
        writeln!(
            output,
            "description overhead: build-time owned clones and summary strings; outside Module::run"
        )
        .expect("String writes cannot fail");
        writeln!(
            output,
            "rendering overhead: text, DOT, and Mermaid return allocated Strings; outside strict runs"
        )
        .expect("String writes cannot fail");
        output
    }

    #[must_use]
    pub fn to_dot(&self) -> String {
        self.graph.to_dot()
    }

    #[must_use]
    pub fn to_mermaid(&self) -> String {
        self.graph.to_mermaid()
    }

    /// Renders the fixed graph with aggregate timing observations from completed runs.
    #[must_use]
    pub fn to_mermaid_with_runs(&self, reports: &[RunReportSnapshot]) -> String {
        let mut samples = vec![Vec::with_capacity(reports.len()); self.graph.execution_order.len()];
        for event in reports.iter().flat_map(RunReportSnapshot::unit_timings) {
            if let Some(unit_samples) = samples.get_mut(event.unit_ordinal) {
                unit_samples.push(event.elapsed.as_secs_f64());
            }
        }
        let annotations = samples
            .iter_mut()
            .enumerate()
            .filter_map(|(unit_ordinal, samples)| {
                if samples.is_empty() {
                    return None;
                }
                samples.sort_by(f64::total_cmp);
                let average = samples.iter().sum::<f64>() / samples.len() as f64;
                let p99_index = (samples.len() * 99).div_ceil(100) - 1;
                let unit = self.graph.execution_order.get(unit_ordinal)?.clone();
                Some((
                    unit,
                    format!(
                        "avg {} / p99 {} / n={}",
                        format_duration(average),
                        format_duration(samples[p99_index]),
                        samples.len()
                    ),
                ))
            })
            .collect();
        self.graph.to_mermaid_with_unit_annotations(&annotations)
    }
}

fn format_duration(seconds: f64) -> String {
    if seconds < 0.001 {
        format!("{:.1} us", seconds * 1_000_000.0)
    } else {
        format!("{:.3} ms", seconds * 1_000.0)
    }
}
