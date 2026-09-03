//! Generic readiness queue and package-specific build scheduling.
//!
//! [`BuildQueue`] owns stable rotation only. [`OutputBuildQueue`] supplies the
//! rendered-output dependency policy used by the build and publish loops.

use std::collections::{HashSet, VecDeque};

use rattler_build_recipe::stage1::{Dependency, TestType};
use rattler_build_types::PinError;
use rattler_conda_types::{
    MatchSpec, ParseMatchSpecError, ParseMatchSpecOptions, RepodataRevision,
};
use thiserror::Error;

use crate::{metadata::Output, types::PackageIdentifier};

/// Errors raised while selecting the next output to build.
#[derive(Debug, Error)]
pub(crate) enum BuildQueueError {
    /// A strong run export from a sibling output is not a valid match specification.
    #[error("failed to parse strong run export `{specification}` from `{provider}`: {source}")]
    InvalidStrongRunExport {
        /// Rendered export that could not be parsed.
        specification: String,
        /// Output that emitted the export.
        provider: String,
        /// Match-spec parsing failure.
        #[source]
        source: ParseMatchSpecError,
    },

    /// A sibling pin could not be resolved from the consumer's rendered subpackage metadata.
    #[error("failed to resolve pin for sibling `{subpackage}` from `{consumer}`: {source}")]
    InvalidSubpackagePin {
        /// Referenced sibling package.
        subpackage: String,
        /// Output containing the pin.
        consumer: String,
        /// Pin rendering failure.
        #[source]
        source: PinError,
    },

    /// A sibling referenced by a pin is missing from the consumer's rendered metadata.
    #[error("sibling `{subpackage}` pinned by `{consumer}` is missing from the build plan")]
    MissingPinnedSubpackage {
        /// Referenced sibling package.
        subpackage: String,
        /// Output containing the pin.
        consumer: String,
    },

    /// Every pending output depends on another unavailable output in the same plan.
    #[error("could not determine a buildable output; blocked outputs: {blocked_outputs}")]
    NoBuildableOutput {
        /// Comma-separated identifiers retained in queue order.
        blocked_outputs: String,
    },
}

/// Evaluates build and test readiness against sibling outputs from the current build.
///
/// The complete plan distinguishes sibling dependencies from packages supplied by
/// configured channels. Finalized outputs describe which sibling dependency
/// closures are currently installable.
struct BuildReadiness<'a> {
    /// Package identities that participate in this local build.
    planned_outputs: &'a [PackageIdentifier],
    /// Outputs already processed by the build loop.
    done_outputs: &'a [Output],
}

impl<'a> BuildReadiness<'a> {
    /// Creates a readiness view over the complete plan and the outputs processed so far.
    fn new(planned_outputs: &'a [PackageIdentifier], done_outputs: &'a [Output]) -> Self {
        Self {
            planned_outputs,
            done_outputs,
        }
    }

    /// Checks whether a planned package identifier satisfies a rendered [`MatchSpec`].
    fn identifier_matches_spec(spec: &MatchSpec, output: &PackageIdentifier) -> bool {
        spec.name.as_exact() == Some(&output.name)
            && spec
                .version
                .as_ref()
                .is_none_or(|version| version.matches(&output.version))
            && spec
                .build
                .as_ref()
                .is_none_or(|build| build.matches(&output.build_string))
    }

    /// Checks whether a processed [`Output`] satisfies a rendered [`MatchSpec`].
    fn output_matches_spec(spec: &MatchSpec, output: &Output) -> bool {
        spec.name.as_exact() == Some(output.name())
            && spec
                .version
                .as_ref()
                .is_none_or(|version| version.matches(output.version()))
            && spec
                .build
                .as_ref()
                .is_none_or(|build| build.matches(&output.build_string()))
    }

    /// Renders the part of a recipe dependency known before solving.
    ///
    /// Subpackage pins are rendered against the consumer's variant-specific
    /// subpackage metadata. Compatible pins depend on solved package records and
    /// therefore retain name-based readiness.
    fn dependency_match_spec(
        dependency: &Dependency,
        consumer: &Output,
    ) -> Result<Option<MatchSpec>, BuildQueueError> {
        match dependency {
            Dependency::Spec(spec) => Ok(Some((**spec).clone())),
            Dependency::PinSubpackage(pin) => {
                let name = &pin.pin_subpackage.name;
                let subpackage = consumer
                    .build_configuration
                    .subpackages
                    .get(name)
                    .ok_or_else(|| BuildQueueError::MissingPinnedSubpackage {
                        subpackage: name.as_normalized().to_string(),
                        consumer: consumer.identifier(),
                    })?;
                let spec = pin
                    .pin_subpackage
                    .apply(&subpackage.version, &subpackage.build_string)
                    .map_err(|source| BuildQueueError::InvalidSubpackagePin {
                        subpackage: name.as_normalized().to_string(),
                        consumer: consumer.identifier(),
                        source,
                    })?;
                Ok(Some(spec))
            }
            Dependency::PinCompatible(_) => Ok(None),
        }
    }

    /// Checks whether two outputs identify the same package artifact.
    fn outputs_have_same_identifier(left: &Output, right: &Output) -> bool {
        left.name() == right.name()
            && left.version() == right.version()
            && left.build_string() == right.build_string()
    }

    /// Collects an output's installable sibling runtime closure.
    ///
    /// Returns `false` when the output or any planned sibling in its finalized
    /// runtime dependencies is not available.
    fn collect_install_closure(
        &self,
        output_index: usize,
        visited: &mut HashSet<usize>,
        closure: &mut Vec<usize>,
    ) -> bool {
        let Some(finalized_dependencies) = &self.done_outputs[output_index].finalized_dependencies
        else {
            return false;
        };
        if !visited.insert(output_index) {
            return true;
        }
        closure.push(output_index);

        for dependency in &finalized_dependencies.run.depends {
            if !self.collect_spec_install_closure(dependency.spec(), visited, closure) {
                return false;
            }
        }

        true
    }

    /// Collects the installable sibling closure for a match specification.
    ///
    /// Specifications without a matching planned sibling are external dependencies
    /// and therefore do not block local scheduling.
    fn collect_spec_install_closure(
        &self,
        spec: &MatchSpec,
        visited: &mut HashSet<usize>,
        closure: &mut Vec<usize>,
    ) -> bool {
        if !self
            .planned_outputs
            .iter()
            .any(|output| Self::identifier_matches_spec(spec, output))
        {
            return true;
        }

        for (output_index, output) in self.done_outputs.iter().enumerate() {
            if !Self::output_matches_spec(spec, output) {
                continue;
            }

            let mut candidate_visited = visited.clone();
            let mut candidate_closure = closure.clone();
            if self.collect_install_closure(
                output_index,
                &mut candidate_visited,
                &mut candidate_closure,
            ) {
                *visited = candidate_visited;
                *closure = candidate_closure;
                return true;
            }
        }

        false
    }

    /// Collects the installable sibling closure for a recipe dependency.
    ///
    /// Dependencies without a matching planned sibling are left for the solver and
    /// therefore do not block local scheduling.
    fn collect_dependency_install_closure(
        &self,
        dependency: &Dependency,
        consumer: &Output,
        visited: &mut HashSet<usize>,
        closure: &mut Vec<usize>,
    ) -> Result<bool, BuildQueueError> {
        let match_spec = Self::dependency_match_spec(dependency, consumer)?;
        let matches_identifier = |output: &PackageIdentifier| match &match_spec {
            Some(spec) => Self::identifier_matches_spec(spec, output),
            None => dependency.name().is_some_and(|name| name == &output.name),
        };
        if !self.planned_outputs.iter().any(matches_identifier) {
            return Ok(true);
        }

        for (output_index, output) in self.done_outputs.iter().enumerate() {
            let matches_output = match &match_spec {
                Some(spec) => Self::output_matches_spec(spec, output),
                None => dependency.name().is_some_and(|name| name == output.name()),
            };
            if !matches_output {
                continue;
            }

            let mut candidate_visited = visited.clone();
            let mut candidate_closure = closure.clone();
            if self.collect_install_closure(
                output_index,
                &mut candidate_visited,
                &mut candidate_closure,
            ) {
                *visited = candidate_visited;
                *closure = candidate_closure;
                return Ok(true);
            }
        }

        Ok(false)
    }

    /// Checks strong run exports contributed by direct build dependencies.
    ///
    /// Ignored exports and exports targeting packages outside the current plan do
    /// not affect readiness.
    fn strong_build_run_exports_are_built(
        &self,
        build_environment_outputs: &[usize],
        consumer: &Output,
    ) -> Result<bool, BuildQueueError> {
        let ignore_run_exports = &consumer.recipe.requirements().ignore_run_exports;

        for &output_index in build_environment_outputs {
            let provider = &self.done_outputs[output_index];
            if ignore_run_exports.from_package.contains(provider.name()) {
                continue;
            }

            let Some(finalized_dependencies) = &provider.finalized_dependencies else {
                continue;
            };

            for run_export in &finalized_dependencies.run.run_exports.strong {
                let spec = MatchSpec::from_str(
                    run_export,
                    ParseMatchSpecOptions::lenient().with_repodata_revision(RepodataRevision::V3),
                )
                .map_err(|source| BuildQueueError::InvalidStrongRunExport {
                    specification: run_export.clone(),
                    provider: provider.identifier(),
                    source,
                })?;

                if spec
                    .name
                    .as_exact()
                    .is_some_and(|name| ignore_run_exports.by_name.contains(name))
                {
                    continue;
                }

                if !self.collect_spec_install_closure(&spec, &mut HashSet::new(), &mut Vec::new()) {
                    return Ok(false);
                }
            }
        }

        Ok(true)
    }

    /// Checks whether all locally planned inputs needed to build an output are available.
    fn can_build(&self, output: &Output) -> Result<bool, BuildQueueError> {
        let requirements = output.recipe.requirements();
        let mut direct_build_outputs = Vec::new();

        for dependency in &requirements.build {
            let mut closure = Vec::new();
            if !self.collect_dependency_install_closure(
                dependency,
                output,
                &mut HashSet::new(),
                &mut closure,
            )? {
                return Ok(false);
            }
            if let Some(&direct_output) = closure.first() {
                direct_build_outputs.push(direct_output);
            }
        }

        for dependency in &requirements.host {
            if !self.collect_dependency_install_closure(
                dependency,
                output,
                &mut HashSet::new(),
                &mut Vec::new(),
            )? {
                return Ok(false);
            }
        }

        if !output.recipe.build().merge_build_and_host_envs
            && !self.strong_build_run_exports_are_built(&direct_build_outputs, output)?
        {
            return Ok(false);
        }

        Ok(true)
    }

    /// Checks whether an output and its command-test dependencies are installable.
    fn can_test(&self, output: &Output) -> Result<bool, BuildQueueError> {
        let Some(output_index) = self
            .done_outputs
            .iter()
            .position(|candidate| Self::outputs_have_same_identifier(candidate, output))
        else {
            return Ok(false);
        };

        if !self.collect_install_closure(output_index, &mut HashSet::new(), &mut Vec::new()) {
            return Ok(false);
        }

        for test in output.recipe.tests() {
            if let TestType::Commands(command) = test {
                for dependency in command
                    .requirements
                    .build
                    .iter()
                    .chain(command.requirements.run.iter())
                {
                    if !self.collect_dependency_install_closure(
                        dependency,
                        output,
                        &mut HashSet::new(),
                        &mut Vec::new(),
                    )? {
                        return Ok(false);
                    }
                }
            }
        }

        Ok(true)
    }
}

/// Result of searching a [`BuildQueue`] for a ready item.
#[derive(Debug, PartialEq, Eq)]
pub(crate) enum NextReady<T> {
    /// An item satisfied the supplied readiness predicate.
    Ready(T),
    /// The queue contains no items.
    Empty,
    /// The queue contains items, but none currently satisfy the predicate.
    Blocked,
}

/// A stable queue that rotates blocked items behind ready work.
///
/// Readiness is supplied by the caller, keeping the queue independent of the
/// item type and the policy that determines whether an item can proceed.
pub(crate) struct BuildQueue<T> {
    /// Items awaiting a successful readiness check.
    pending: VecDeque<T>,
}

impl<T> BuildQueue<T> {
    /// Creates a queue that preserves the input order.
    pub(crate) fn new(items: impl IntoIterator<Item = T>) -> Self {
        Self {
            pending: items.into_iter().collect(),
        }
    }

    /// Returns the number of items still waiting in the queue.
    pub(crate) fn len(&self) -> usize {
        self.pending.len()
    }

    /// Finds and removes the first item that satisfies `is_ready`.
    ///
    /// Rejected items are moved to the back of the queue. If readiness
    /// evaluation fails, the current item is restored to the front.
    pub(crate) fn next_ready<E>(
        &mut self,
        mut is_ready: impl FnMut(&T) -> Result<bool, E>,
    ) -> Result<NextReady<T>, E> {
        for _ in 0..self.pending.len() {
            let Some(item) = self.pending.pop_front() else {
                return Ok(NextReady::Empty);
            };

            match is_ready(&item) {
                Ok(true) => return Ok(NextReady::Ready(item)),
                Ok(false) => self.pending.push_back(item),
                Err(error) => {
                    self.pending.push_front(item);
                    return Err(error);
                }
            }
        }

        if self.pending.is_empty() {
            Ok(NextReady::Empty)
        } else {
            Ok(NextReady::Blocked)
        }
    }

    /// Removes the first item without checking readiness.
    fn pop_front(&mut self) -> Option<T> {
        self.pending.pop_front()
    }

    /// Iterates over queued items in their current stable order.
    fn iter(&self) -> impl Iterator<Item = &T> {
        self.pending.iter()
    }
}

/// Policy applied when no output is currently ready.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum BlockedOutputPolicy {
    /// Return a typed scheduling error.
    Error,
    /// Attempt the first blocked output so failure-tolerant builds can continue.
    AttemptFirst,
}

/// Applies rendered-output readiness rules to a generic [`BuildQueue`].
pub(crate) struct OutputBuildQueue {
    /// Outputs awaiting a build attempt.
    queue: BuildQueue<Output>,
    /// Complete local plan, including outputs not processed yet.
    planned_outputs: Vec<PackageIdentifier>,
    /// Attempted outputs used to determine which local artifacts are available.
    processed_outputs: Vec<Output>,
}

impl OutputBuildQueue {
    /// Creates a queue in render order and records every output in the sibling plan.
    pub(crate) fn new(outputs: Vec<Output>) -> Self {
        let planned_outputs = outputs
            .iter()
            .map(|output| PackageIdentifier {
                name: output.name().clone(),
                version: output.version().clone(),
                build_string: output.build_string().into_owned(),
            })
            .collect();
        Self {
            queue: BuildQueue::new(outputs),
            planned_outputs,
            processed_outputs: Vec::new(),
        }
    }

    /// Returns the number of outputs still waiting to build.
    pub(crate) fn len(&self) -> usize {
        self.queue.len()
    }

    /// Records an attempted output for subsequent readiness checks.
    ///
    /// Only outputs with finalized dependencies are considered installable.
    pub(crate) fn record_processed(&mut self, output: Output) {
        self.processed_outputs.push(output);
    }

    /// Checks whether an output and its command-test dependencies are installable.
    pub(crate) fn can_test(&self, output: &Output) -> Result<bool, BuildQueueError> {
        BuildReadiness::new(&self.planned_outputs, &self.processed_outputs).can_test(output)
    }

    /// Removes the next buildable output or applies the blocked-output policy.
    pub(crate) fn next_ready(
        &mut self,
        blocked_policy: BlockedOutputPolicy,
    ) -> Result<Option<Output>, BuildQueueError> {
        let readiness = BuildReadiness::new(&self.planned_outputs, &self.processed_outputs);
        match self
            .queue
            .next_ready(|output| readiness.can_build(output))?
        {
            NextReady::Ready(output) => Ok(Some(output)),
            NextReady::Empty => Ok(None),
            NextReady::Blocked => match blocked_policy {
                BlockedOutputPolicy::AttemptFirst => {
                    let output = self.queue.pop_front();
                    if let Some(output) = &output {
                        tracing::warn!(
                            "No output is currently buildable; attempting {} because failures may continue",
                            output.identifier()
                        );
                    }
                    Ok(output)
                }
                BlockedOutputPolicy::Error => {
                    let blocked_outputs = self
                        .queue
                        .iter()
                        .map(Output::identifier)
                        .collect::<Vec<_>>()
                        .join(", ");
                    Err(BuildQueueError::NoBuildableOutput { blocked_outputs })
                }
            },
        }
    }
}

#[cfg(test)]
mod tests {
    use std::convert::Infallible;

    use super::{BuildQueue, NextReady};

    #[test]
    fn ready_items_preserve_rotated_order() {
        let mut queue = BuildQueue::new([1, 2, 3]);

        assert_eq!(
            queue.next_ready(|item| Ok::<_, Infallible>(*item == 2)),
            Ok(NextReady::Ready(2))
        );
        assert_eq!(
            queue.next_ready(|_| Ok::<_, Infallible>(true)),
            Ok(NextReady::Ready(3))
        );
        assert_eq!(
            queue.next_ready(|_| Ok::<_, Infallible>(true)),
            Ok(NextReady::Ready(1))
        );
        assert_eq!(
            queue.next_ready(|_| Ok::<_, Infallible>(true)),
            Ok(NextReady::Empty)
        );
    }

    #[test]
    fn reports_blocked_without_removing_items() {
        let mut queue = BuildQueue::new([1, 2]);

        assert_eq!(
            queue.next_ready(|_| Ok::<_, Infallible>(false)),
            Ok(NextReady::Blocked)
        );
        assert_eq!(queue.len(), 2);
        assert_eq!(
            queue.next_ready(|_| Ok::<_, Infallible>(true)),
            Ok(NextReady::Ready(1))
        );
    }

    #[derive(Debug, PartialEq, Eq)]
    struct ReadinessError;

    #[test]
    fn readiness_errors_preserve_the_current_item() {
        let mut queue = BuildQueue::new([1, 2]);

        assert_eq!(
            queue.next_ready(|_| Err(ReadinessError)),
            Err(ReadinessError)
        );
        assert_eq!(
            queue.next_ready(|_| Ok::<_, ReadinessError>(true)),
            Ok(NextReady::Ready(1))
        );
    }
}
