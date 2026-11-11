//! Generic readiness queue and package-specific build scheduling.
//!
//! [`BuildQueue`] owns stable rotation only. [`OutputBuildQueue`] supplies the
//! rendered-output dependency policy used by the build and publish loops.

use std::{
    borrow::Cow,
    cell::{Cell, RefCell},
    collections::{BTreeMap, HashMap, HashSet, VecDeque},
};

use rattler_build_core::Variable;
use rattler_build_recipe::stage1::{Dependency, TestType};
use rattler_build_types::{NormalizedKey, PinError};
use rattler_conda_types::{
    MatchSpec, PackageName, ParseMatchSpecError, ParseMatchSpecOptions, RepodataRevision,
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
}

/// The variant an output is built for.
type Variant = BTreeMap<NormalizedKey, Variable>;

/// A planned output: its package identity and the variant it is built for.
struct PlannedOutput {
    identifier: PackageIdentifier,
    variant: Variant,
}

/// Variant keys that describe where a package is built or installed. They are
/// ignored when one side is `noarch`, which can be used with any platform.
const PLATFORM_VARIANT_KEYS: [&str; 3] = ["target_platform", "host_platform", "build_platform"];

/// Whether a variant is for a `noarch` package.
fn is_noarch(variant: &Variant) -> bool {
    variant
        .get(&NormalizedKey::from("target_platform"))
        .and_then(Variable::as_str)
        == Some("noarch")
}

/// Whether two variants agree on every key they both use, e.g. are built for
/// the same Python version. Platform keys only count when neither is `noarch`.
fn variants_agree(left: &Variant, right: &Variant) -> bool {
    let ignore_platform = is_noarch(left) || is_noarch(right);
    left.iter().all(|(key, value)| {
        (ignore_platform && PLATFORM_VARIANT_KEYS.contains(&key.normalize().as_str()))
            || right.get(key).is_none_or(|other| other == value)
    })
}

/// The variant that the run dependencies of `output` must agree with when it
/// is installed for a package built for `requirer`: the output's own variant,
/// plus the keys it does not use from `requirer`. This keeps, for example, the
/// Python version across a `noarch` package in between.
fn installed_variant<'v>(output: &'v Variant, requirer: &Variant) -> Cow<'v, Variant> {
    let inherited = inherited_keys(output, requirer);
    if inherited.is_empty() {
        return Cow::Borrowed(output);
    }
    let mut variant = output.clone();
    variant.extend(inherited);
    Cow::Owned(variant)
}

/// The keys of `requirer` that `output` does not use, which
/// [`installed_variant`] adds to the output's variant.
fn inherited_keys(output: &Variant, requirer: &Variant) -> Variant {
    requirer
        .iter()
        .filter(|(key, _)| !output.contains_key(*key))
        .map(|(key, value)| (key.clone(), value.clone()))
        .collect()
}

/// Identifies an output installed for a requirer: the output's index and the
/// keys it inherits from the requirer (see [`installed_variant`]). Whether the
/// output is installable depends on both, so results are cached by this key.
type InstallKey = (usize, Variant);

/// Evaluates build and test readiness against sibling outputs from the current build.
///
/// The complete plan distinguishes sibling dependencies from packages supplied by
/// configured channels. Finalized outputs describe which sibling dependency
/// closures are currently installable.
struct BuildReadiness<'a> {
    /// Package identities that participate in this local build.
    planned_outputs: &'a [PlannedOutput],
    /// Outputs already processed by the build loop.
    done_outputs: &'a [Output],
    /// Outputs not built because they already exist.
    skipped: SkippedOutputs<'a>,
}

/// Outputs not built because they already exist. They are installed from a
/// channel, together with their run dependencies, which may be planned.
///
/// Keeps, for one scheduling pass, which of them are installable, so shared
/// dependencies are checked once.
struct SkippedOutputs<'a> {
    outputs: &'a [Output],
    /// Indices into `outputs` by package name.
    by_name: HashMap<&'a PackageName, Vec<usize>>,
    /// Whether an output is installable, per output and inherited keys. Only
    /// results that did not depend on the cycle guard are kept.
    installable: RefCell<HashMap<InstallKey, bool>>,
    /// Outputs whose dependencies are being checked (cycle guard).
    being_checked: RefCell<HashSet<InstallKey>>,
    /// Set when the cycle guard answered for an output still being checked.
    reached_cycle: Cell<bool>,
}

impl<'a> SkippedOutputs<'a> {
    fn new(outputs: &'a [Output]) -> Self {
        let mut by_name: HashMap<&PackageName, Vec<usize>> = HashMap::new();
        for (index, output) in outputs.iter().enumerate() {
            by_name.entry(output.name()).or_default().push(index);
        }
        Self {
            outputs,
            by_name,
            installable: RefCell::new(HashMap::new()),
            being_checked: RefCell::new(HashSet::new()),
            reached_cycle: Cell::new(false),
        }
    }

    /// The indices of the outputs named `name`, or of all outputs when the
    /// name is not known.
    fn candidates(&self, name: Option<&PackageName>) -> Vec<usize> {
        match name {
            Some(name) => self.by_name.get(name).cloned().unwrap_or_default(),
            None => (0..self.outputs.len()).collect(),
        }
    }

    /// Whether the output at `key` is installable, as decided by
    /// `installable` and cached. In a dependency cycle, the check of an output
    /// that is already being checked answers `true`: the check further up
    /// decides.
    fn check<E>(
        &self,
        key: InstallKey,
        installable: impl FnOnce(&'a Output) -> Result<bool, E>,
    ) -> Result<bool, E> {
        if let Some(&result) = self.installable.borrow().get(&key) {
            return Ok(result);
        }
        if !self.being_checked.borrow_mut().insert(key.clone()) {
            self.reached_cycle.set(true);
            return Ok(true);
        }
        let reached_cycle_before = self.reached_cycle.replace(false);

        let result = installable(&self.outputs[key.0]);

        self.being_checked.borrow_mut().remove(&key);
        let reached_cycle = self.reached_cycle.get();
        if !reached_cycle && let Ok(result) = result {
            self.installable.borrow_mut().insert(key, result);
        }
        self.reached_cycle
            .set(reached_cycle_before || reached_cycle);
        result
    }
}

impl<'a> BuildReadiness<'a> {
    /// Creates a readiness view over the complete plan, the outputs processed so
    /// far and the outputs skipped because they already exist.
    fn new(
        planned_outputs: &'a [PlannedOutput],
        done_outputs: &'a [Output],
        skipped_outputs: &'a [Output],
    ) -> Self {
        Self {
            planned_outputs,
            done_outputs,
            skipped: SkippedOutputs::new(skipped_outputs),
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
                let Some(subpackage) = consumer.build_configuration.subpackages.get(name) else {
                    return Ok(None);
                };
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
    /// runtime dependencies is not available. `requirer` is the variant of the
    /// package the output is installed for (see [`installed_variant`]).
    fn collect_install_closure(
        &self,
        output_index: usize,
        requirer: &Variant,
        visited: &mut HashSet<InstallKey>,
        availability: &mut HashMap<InstallKey, bool>,
        closure: &mut Vec<usize>,
    ) -> Result<bool, BuildQueueError> {
        let output = &self.done_outputs[output_index];
        let key = (output_index, inherited_keys(output.variant(), requirer));
        if !visited.insert(key.clone()) {
            return Ok(true);
        }
        if let Some(&available) = availability.get(&key) {
            if available {
                closure.push(output_index);
            }
            return Ok(available);
        }

        let Some(finalized_dependencies) = &output.finalized_dependencies else {
            availability.insert(key, false);
            return Ok(false);
        };
        closure.push(output_index);

        let variant = installed_variant(output.variant(), requirer);
        let mut available = true;
        for dependency in &finalized_dependencies.run.depends {
            if !self.collect_spec_install_closure(
                dependency.spec(),
                &variant,
                visited,
                availability,
                closure,
            )? {
                available = false;
                break;
            }
        }
        availability.insert(key, available);
        Ok(available)
    }

    /// Collects the installable sibling closure for a match specification
    /// needed by a package built for `variant`.
    fn collect_spec_install_closure(
        &self,
        spec: &MatchSpec,
        variant: &Variant,
        visited: &mut HashSet<InstallKey>,
        availability: &mut HashMap<InstallKey, bool>,
        closure: &mut Vec<usize>,
    ) -> Result<bool, BuildQueueError> {
        self.collect_matching_install_closure(
            spec.name.as_exact(),
            |output| Self::identifier_matches_spec(spec, output),
            |output| Self::output_matches_spec(spec, output),
            variant,
            visited,
            availability,
            closure,
        )
    }

    /// Collects the installable sibling closure for a recipe dependency of
    /// `consumer`, installed for a package built for `variant`.
    #[allow(clippy::too_many_arguments)]
    fn collect_dependency_install_closure(
        &self,
        dependency: &Dependency,
        consumer: &Output,
        variant: &Variant,
        visited: &mut HashSet<InstallKey>,
        availability: &mut HashMap<InstallKey, bool>,
        closure: &mut Vec<usize>,
    ) -> Result<bool, BuildQueueError> {
        let match_spec = Self::dependency_match_spec(dependency, consumer)?;
        self.collect_matching_install_closure(
            dependency.name(),
            |output| match &match_spec {
                Some(spec) => Self::identifier_matches_spec(spec, output),
                None => dependency.name().is_some_and(|name| name == &output.name),
            },
            |output| match &match_spec {
                Some(spec) => Self::output_matches_spec(spec, output),
                None => dependency.name().is_some_and(|name| name == output.name()),
            },
            variant,
            visited,
            availability,
            closure,
        )
    }

    /// Collects the installable sibling closure for a dependency, named `name`
    /// if known, of a package built for `variant`.
    ///
    /// Only the variants of the dependency that agree with `variant` (e.g. are
    /// built for the same Python version) can be installed with the package, so
    /// when the plan or the skipped outputs have such a variant, the others are
    /// not considered. Without one, any matching variant will do.
    ///
    /// Dependencies that are neither planned nor skipped are left to the solver
    /// and do not block local scheduling. Skipped outputs are installed from a
    /// channel, but their run dependencies may be planned.
    #[allow(clippy::too_many_arguments)]
    fn collect_matching_install_closure(
        &self,
        name: Option<&PackageName>,
        matches_identifier: impl Fn(&PackageIdentifier) -> bool,
        matches_output: impl Fn(&Output) -> bool,
        variant: &Variant,
        visited: &mut HashSet<InstallKey>,
        availability: &mut HashMap<InstallKey, bool>,
        closure: &mut Vec<usize>,
    ) -> Result<bool, BuildQueueError> {
        let mut planned = false;
        let mut agreeing_planned = false;
        for output in self.planned_outputs {
            if matches_identifier(&output.identifier) {
                planned = true;
                if variants_agree(variant, &output.variant) {
                    agreeing_planned = true;
                    break;
                }
            }
        }

        if !agreeing_planned {
            let skipped: Vec<usize> = self
                .skipped
                .candidates(name)
                .into_iter()
                .filter(|&index| matches_output(&self.skipped.outputs[index]))
                .collect();
            let agreeing_skipped: Vec<usize> = skipped
                .iter()
                .copied()
                .filter(|&index| variants_agree(variant, self.skipped.outputs[index].variant()))
                .collect();
            if !agreeing_skipped.is_empty() {
                return self.skipped_outputs_are_installable(&agreeing_skipped, variant);
            }
            if !planned {
                return self.skipped_outputs_are_installable(&skipped, variant);
            }
        }

        for (output_index, output) in self.done_outputs.iter().enumerate() {
            if !matches_output(output)
                || (agreeing_planned && !variants_agree(variant, output.variant()))
            {
                continue;
            }

            let mut candidate_visited = visited.clone();
            let mut candidate_closure = closure.clone();
            if self.collect_install_closure(
                output_index,
                variant,
                &mut candidate_visited,
                availability,
                &mut candidate_closure,
            )? {
                *visited = candidate_visited;
                *closure = candidate_closure;
                return Ok(true);
            }
        }

        Ok(false)
    }

    /// Checks skipped outputs that a dependency matches. The solver installs
    /// one of them, and with it its run dependencies, which may be planned.
    /// Which one it picks is not known before solving, so all of them must be
    /// installable.
    fn skipped_outputs_are_installable(
        &self,
        indices: &[usize],
        requirer: &Variant,
    ) -> Result<bool, BuildQueueError> {
        for &index in indices {
            if !self.skipped_output_is_installable(index, requirer)? {
                return Ok(false);
            }
        }
        Ok(true)
    }

    /// Checks whether a skipped output can be installed: whether its planned
    /// dependencies are built. Its run requirements are installed with it; its
    /// host and build requirements are checked too, because their run exports
    /// are part of the published package's dependencies. `requirer` is the
    /// variant of the package it is installed for (see [`installed_variant`]).
    fn skipped_output_is_installable(
        &self,
        index: usize,
        requirer: &Variant,
    ) -> Result<bool, BuildQueueError> {
        let own_variant = self.skipped.outputs[index].variant();
        let key = (index, inherited_keys(own_variant, requirer));
        self.skipped.check(key, |skipped| {
            let variant = installed_variant(skipped.variant(), requirer);
            let mut availability = HashMap::new();
            let requirements = skipped.recipe.requirements();
            for dependency in requirements
                .run
                .iter()
                .chain(&requirements.host)
                .chain(&requirements.build)
            {
                if !self.collect_dependency_install_closure(
                    dependency,
                    skipped,
                    &variant,
                    &mut HashSet::new(),
                    &mut availability,
                    &mut Vec::new(),
                )? {
                    return Ok(false);
                }
            }
            Ok(true)
        })
    }

    /// Checks strong run exports contributed by direct build dependencies.
    ///
    /// Ignored exports and exports targeting packages outside the current plan do
    /// not affect readiness.
    fn strong_build_run_exports_are_built(
        &self,
        build_environment_outputs: &[usize],
        consumer: &Output,
        availability: &mut HashMap<InstallKey, bool>,
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

                if !self.collect_spec_install_closure(
                    &spec,
                    consumer.variant(),
                    &mut HashSet::new(),
                    availability,
                    &mut Vec::new(),
                )? {
                    return Ok(false);
                }
            }
        }

        Ok(true)
    }

    /// Checks whether all locally planned inputs needed to build an output are available.
    fn can_build(&self, output: &Output) -> Result<bool, BuildQueueError> {
        let requirements = output.recipe.requirements();
        let mut availability = HashMap::new();
        let mut direct_build_outputs = Vec::new();

        for dependency in &requirements.build {
            let mut closure = Vec::new();
            if !self.collect_dependency_install_closure(
                dependency,
                output,
                output.variant(),
                &mut HashSet::new(),
                &mut availability,
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
                output.variant(),
                &mut HashSet::new(),
                &mut availability,
                &mut Vec::new(),
            )? {
                return Ok(false);
            }
        }

        if !output.recipe.build().merge_build_and_host_envs
            && !self.strong_build_run_exports_are_built(
                &direct_build_outputs,
                output,
                &mut availability,
            )?
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

        let mut availability = HashMap::new();
        if !self.collect_install_closure(
            output_index,
            output.variant(),
            &mut HashSet::new(),
            &mut availability,
            &mut Vec::new(),
        )? {
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
                        output.variant(),
                        &mut HashSet::new(),
                        &mut availability,
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

    /// Removes and returns all items matching `predicate`, keeping the order
    /// of the remaining items.
    pub(crate) fn take_where(&mut self, mut predicate: impl FnMut(&T) -> bool) -> Vec<T> {
        let (taken, kept): (VecDeque<T>, VecDeque<T>) =
            self.pending.drain(..).partition(|item| predicate(item));
        self.pending = kept;
        taken.into()
    }

    /// Finds and removes the ready item with the highest `score`, keeping
    /// the order of all other items. Ties go to the earliest item.
    pub(crate) fn best_ready<E>(
        &mut self,
        mut is_ready: impl FnMut(&T) -> Result<bool, E>,
        score: impl Fn(&T) -> usize,
    ) -> Result<Option<T>, E> {
        let mut best: Option<(usize, usize)> = None;
        for (index, item) in self.pending.iter().enumerate() {
            let item_score = score(item);
            if best.is_some_and(|(_, best_score)| item_score <= best_score) {
                continue;
            }
            if is_ready(item)? {
                best = Some((index, item_score));
            }
        }
        Ok(best.and_then(|(index, _)| self.pending.remove(index)))
    }

    /// Removes the first item without checking readiness.
    fn pop_front(&mut self) -> Option<T> {
        self.pending.pop_front()
    }
}

/// Applies rendered-output readiness rules to a generic [`BuildQueue`].
pub(crate) struct OutputBuildQueue {
    /// Outputs awaiting a build attempt.
    queue: BuildQueue<Output>,
    /// Complete local plan, including outputs not processed yet.
    planned_outputs: Vec<PlannedOutput>,
    /// Attempted outputs used to determine which local artifacts are available.
    processed_outputs: Vec<Output>,
    /// Outputs not built because they already exist; their run dependencies may
    /// be planned.
    skipped_outputs: Vec<Output>,
    /// For every planned output, the planned packages that must be built
    /// before it can build: its build and host dependencies plus their run
    /// dependency closure, also through skipped outputs.
    required_planned: HashMap<PackageName, HashSet<PackageName>>,
    /// Planned packages that failed to build or were skipped.
    failed: HashSet<PackageName>,
    /// For every planned package, how many other planned outputs need it,
    /// directly or transitively (through build, host or run requirements).
    dependents: HashMap<PackageName, usize>,
}

impl OutputBuildQueue {
    /// Creates a queue in render order and records every output in the sibling plan.
    ///
    /// `skipped_outputs` are the rendered outputs that are not built because they
    /// already exist (see `skip_existing`).
    pub(crate) fn new(outputs: Vec<Output>, skipped_outputs: Vec<Output>) -> Self {
        let planned_outputs = outputs
            .iter()
            .map(|output| PlannedOutput {
                identifier: PackageIdentifier {
                    name: output.name().clone(),
                    version: output.version().clone(),
                    build_string: output.build_string().into_owned(),
                },
                variant: output.variant().clone(),
            })
            .collect();
        let required_planned = required_planned_packages(&outputs, &skipped_outputs);
        let dependents = count_dependents(outputs.iter().map(|output| {
            let requirements = output.recipe.requirements();
            let needs = requirements
                .build
                .iter()
                .chain(&requirements.host)
                .chain(&requirements.run)
                .filter_map(dependency_name)
                .cloned()
                .collect();
            (output.name().clone(), needs)
        }));
        Self {
            queue: BuildQueue::new(outputs),
            planned_outputs,
            processed_outputs: Vec::new(),
            skipped_outputs,
            required_planned,
            failed: HashSet::new(),
            dependents,
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

    /// Records that `failed` did not build and removes every waiting output
    /// that needs it, directly, through the run dependencies of a build or
    /// host dependency, or through another removed output.
    ///
    /// Returns each removed output together with the failed package it needs.
    /// The caller decides how to report them; they are not built or attempted
    /// against the configured channels.
    pub(crate) fn skip_dependents_of_failed(
        &mut self,
        failed: &Output,
    ) -> Vec<(Output, PackageName)> {
        self.failed.insert(failed.name().clone());
        let required_planned = &self.required_planned;
        skip_blocked(&mut self.queue, &mut self.failed, |output| {
            (output.name(), required_planned.get(output.name()))
        })
    }

    /// Checks whether an output and its command-test dependencies are installable.
    pub(crate) fn can_test(&self, output: &Output) -> Result<bool, BuildQueueError> {
        BuildReadiness::new(
            &self.planned_outputs,
            &self.processed_outputs,
            &self.skipped_outputs,
        )
        .can_test(output)
    }

    /// Like [`Self::next_ready`], for a build loop with `in_flight` builds
    /// running.
    ///
    /// Returns `Ok(None)` while no output is buildable but a running build may
    /// still unblock one. The fallback to the first blocked output is only used
    /// when nothing is in flight, so the outputs that are attempted, and what
    /// the solver may pick from the configured channels, are the same as in a
    /// serial build.
    ///
    /// With `prioritize`, the ready output that the most other planned outputs
    /// need (directly or transitively) is started first, so that long chains
    /// of dependent builds start early. Used only for parallel builds; a
    /// serial build keeps the queue order.
    ///
    /// While jobs are in flight, outputs rejected by `can_start` stay queued.
    pub(crate) fn next_ready_parallel(
        &mut self,
        in_flight: usize,
        prioritize: bool,
        can_start: impl Fn(&Output) -> bool,
    ) -> Result<Option<Output>, BuildQueueError> {
        if prioritize {
            let readiness = BuildReadiness::new(
                &self.planned_outputs,
                &self.processed_outputs,
                &self.skipped_outputs,
            );
            let dependents = &self.dependents;
            if let Some(output) = self.queue.best_ready(
                |output| Ok(can_start(output) && readiness.can_build(output)?),
                |output| dependents.get(output.name()).copied().unwrap_or(0),
            )? {
                return Ok(Some(output));
            }
        }
        if in_flight == 0 {
            return self.next_ready();
        }
        let readiness = BuildReadiness::new(
            &self.planned_outputs,
            &self.processed_outputs,
            &self.skipped_outputs,
        );
        match self
            .queue
            .next_ready(|output| Ok(can_start(output) && readiness.can_build(output)?))?
        {
            NextReady::Ready(output) => Ok(Some(output)),
            NextReady::Empty | NextReady::Blocked => Ok(None),
        }
    }

    /// Removes the next buildable output, or the first blocked output as a fallback.
    pub(crate) fn next_ready(&mut self) -> Result<Option<Output>, BuildQueueError> {
        let readiness = BuildReadiness::new(
            &self.planned_outputs,
            &self.processed_outputs,
            &self.skipped_outputs,
        );
        match self
            .queue
            .next_ready(|output| readiness.can_build(output))?
        {
            NextReady::Ready(output) => Ok(Some(output)),
            NextReady::Empty => Ok(None),
            NextReady::Blocked => {
                let output = self.queue.pop_front();
                if let Some(output) = &output {
                    tracing::warn!(
                        "No output is currently known to be buildable; attempting {} so the solver can check configured channels",
                        output.identifier()
                    );
                }
                Ok(output)
            }
        }
    }
}

/// Removes every queued item that requires a failed package, recursively:
/// a removed item counts as failed for the items that require it.
///
/// `describe` returns an item's package name and the planned packages it
/// requires. Returns the removed items, each with the (alphabetically first)
/// failed package it requires.
fn skip_blocked<'a, T: 'a>(
    queue: &mut BuildQueue<T>,
    failed: &mut HashSet<PackageName>,
    describe: impl Fn(&T) -> (&PackageName, Option<&'a HashSet<PackageName>>),
) -> Vec<(T, PackageName)> {
    let mut skipped = Vec::new();
    loop {
        let blocked_by = |item: &T| {
            describe(item)
                .1
                .into_iter()
                .flatten()
                .filter(|name| failed.contains(*name))
                .min()
                .cloned()
        };
        let newly: Vec<(T, PackageName)> = queue
            .take_where(|item| blocked_by(item).is_some())
            .into_iter()
            .map(|item| {
                let cause = blocked_by(&item).expect("taken because it is blocked");
                (item, cause)
            })
            .collect();
        if newly.is_empty() {
            return skipped;
        }
        for (item, _) in &newly {
            failed.insert(describe(item).0.clone());
        }
        skipped.extend(newly);
    }
}

/// The package name a recipe dependency refers to, if it is known before
/// solving.
fn dependency_name(dependency: &Dependency) -> Option<&PackageName> {
    match dependency {
        Dependency::Spec(spec) => spec.name.as_exact(),
        Dependency::PinSubpackage(pin) => Some(&pin.pin_subpackage.name),
        Dependency::PinCompatible(_) => None,
    }
}

/// For every planned output, the planned packages that must be built before
/// it can build: its build and host dependencies plus their run dependency
/// closure, also through the run dependencies of skipped outputs. This follows
/// the rule of [`BuildReadiness::can_build`], but matches packages by name
/// only; variants that share a name are merged.
fn required_planned_packages(
    outputs: &[Output],
    skipped_outputs: &[Output],
) -> HashMap<PackageName, HashSet<PackageName>> {
    let planned: HashSet<&PackageName> = outputs.iter().map(|output| output.name()).collect();
    let local: HashSet<&PackageName> = planned
        .iter()
        .copied()
        .chain(skipped_outputs.iter().map(|output| output.name()))
        .collect();
    let local_only = |dependencies: &[Dependency]| -> Vec<PackageName> {
        dependencies
            .iter()
            .filter_map(dependency_name)
            .filter(|name| local.contains(name))
            .cloned()
            .collect()
    };
    let mut run: HashMap<PackageName, Vec<PackageName>> = HashMap::new();
    for output in outputs.iter().chain(skipped_outputs) {
        run.entry(output.name().clone())
            .or_default()
            .extend(local_only(&output.recipe.requirements().run));
    }
    let mut required: HashMap<PackageName, HashSet<PackageName>> = HashMap::new();
    for output in outputs {
        let requirements = output.recipe.requirements();
        let mut direct = local_only(&requirements.build);
        direct.extend(local_only(&requirements.host));
        required.entry(output.name().clone()).or_default().extend(
            closure_without(direct, &run, output.name())
                .into_iter()
                .filter(|name| planned.contains(name)),
        );
    }
    required
}

/// All packages reachable from `start` through `run`, except `own_name`.
fn closure_without(
    start: Vec<PackageName>,
    run: &HashMap<PackageName, Vec<PackageName>>,
    own_name: &PackageName,
) -> HashSet<PackageName> {
    let mut reached = HashSet::new();
    let mut stack = start;
    while let Some(name) = stack.pop() {
        if &name != own_name && reached.insert(name.clone()) {
            stack.extend(run.get(&name).into_iter().flatten().cloned());
        }
    }
    reached
}

/// For every planned package, counts the other planned packages that need it
/// directly or transitively. `needs` yields each planned package with the
/// names it depends on; names that are not planned are ignored.
fn count_dependents(
    needs: impl IntoIterator<Item = (PackageName, Vec<PackageName>)>,
) -> HashMap<PackageName, usize> {
    let needs: Vec<(PackageName, Vec<PackageName>)> = needs.into_iter().collect();
    let planned: HashSet<&PackageName> = needs.iter().map(|(name, _)| name).collect();
    let mut users: HashMap<&PackageName, HashSet<&PackageName>> = HashMap::new();
    for (name, deps) in &needs {
        for dep in deps {
            if dep != name && planned.contains(dep) {
                users.entry(dep).or_default().insert(name);
            }
        }
    }
    planned
        .iter()
        .map(|&name| {
            let mut seen = HashSet::new();
            let mut stack = vec![name];
            while let Some(current) = stack.pop() {
                for &user in users.get(current).into_iter().flatten() {
                    if user != name && seen.insert(user) {
                        stack.push(user);
                    }
                }
            }
            (name.clone(), seen.len())
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use std::{
        collections::{HashMap, HashSet},
        convert::Infallible,
    };

    use rattler_conda_types::PackageName;

    use rattler_build_core::Variable;
    use rattler_build_types::NormalizedKey;

    use super::{
        BuildQueue, NextReady, OutputBuildQueue, closure_without, count_dependents, skip_blocked,
    };
    use crate::metadata::Output;

    fn name(name: &str) -> PackageName {
        PackageName::new_unchecked(name)
    }

    fn names(list: &[&str]) -> HashSet<PackageName> {
        list.iter().map(|n| name(n)).collect()
    }

    /// A rendered output with the given host and run requirements. A `built`
    /// output also has finalized dependencies, so it counts as installable.
    fn output(name: &str, build_string: &str, host: &[&str], run: &[&str], built: bool) -> Output {
        let list = |specs: &[&str]| format!("[{}]", specs.join(", "));
        let finalized = if built {
            let depends: Vec<String> = run
                .iter()
                .map(|spec| format!("{{source: \"{spec}\"}}"))
                .collect();
            format!(
                "finalized_dependencies:\n  build: null\n  host: null\n  run:\n    depends: [{}]\n    constraints: []\n",
                depends.join(", ")
            )
        } else {
            String::new()
        };
        let yaml = format!(
            r#"recipe:
  schema_version: 1
  context: {{}}
  package:
    name: {name}
    version: "1"
  build:
    number: 0
    string: {build_string}
  requirements:
    host: {host}
    run: {run}
build_configuration:
  target_platform: linux-64
  host_platform: linux-64
  build_platform: linux-64
  variant: {{}}
  hash:
    hash: "0"
  directories:
    host_prefix: /bld/host
    build_prefix: /bld/build
    work_dir: /bld/work
    build_dir: /bld
  channels: []
  channel_priority: strict
  solve_strategy: highest
  timestamp: 2024-01-01T00:00:00Z
  subpackages: {{}}
  packaging_settings:
    archive_type: conda
    compression_level: 1
system_tools: {{}}
{finalized}"#,
            host = list(host),
            run = list(run),
        );
        serde_yaml::from_str(&yaml).unwrap()
    }

    /// The `sx` example: `sx-a` needs the skipped `sx-b` in its host
    /// environment, and installing `sx-b` needs the planned `sx-c`. `sx-a`
    /// must wait until `sx-c` is built, also while another build runs.
    fn assert_waits_for_dependency_of_skipped(skipped: Vec<Output>, sx_c_run: &[&str]) {
        let sx_a = output("sx-a", "h0_0", &["sx-b"], &[], false);
        let sx_c = output("sx-c", "h0_0", &[], sx_c_run, false);
        let mut queue = OutputBuildQueue::new(vec![sx_a, sx_c], skipped);
        let ready = |queue: &mut OutputBuildQueue, in_flight| {
            queue
                .next_ready_parallel(in_flight, true, |_| true)
                .unwrap()
                .map(|output| output.name().as_normalized().to_string())
        };

        assert_eq!(ready(&mut queue, 0).as_deref(), Some("sx-c"));
        // `sx-c` is building
        assert_eq!(ready(&mut queue, 1), None);

        queue.record_processed(output("sx-c", "h0_0", &[], sx_c_run, true));
        // still one build in flight, so the serial fallback cannot start it
        assert_eq!(ready(&mut queue, 1).as_deref(), Some("sx-a"));
    }

    /// `output` with a variant, e.g. `[("python", "3.12")]`.
    fn with_variant(mut output: Output, variant: &[(&str, &str)]) -> Output {
        output.build_configuration.variant = variant
            .iter()
            .map(|(key, value)| (NormalizedKey::from(*key), Variable::from(*value)))
            .collect();
        output
    }

    /// A planned output in a variant for the given Python version on linux-64.
    fn py_output(name: &str, python: &str, host: &[&str]) -> Output {
        let build_string = format!("py{}_0", python.replace('.', ""));
        with_variant(
            output(name, &build_string, host, &[], false),
            &[("python", python), ("target_platform", "linux-64")],
        )
    }

    /// `planned` after it was built, with the given run dependencies.
    fn built(planned: &Output, run: &[&str]) -> Output {
        let mut done = output(
            planned.name().as_normalized(),
            &planned.build_string(),
            &[],
            run,
            true,
        );
        done.build_configuration.variant = planned.variant().clone();
        done
    }

    /// Whether `name` is among the outputs that can start now, while another
    /// build is in flight (so the serial fallback does not apply).
    fn can_start(queue: &mut OutputBuildQueue, name: &str) -> bool {
        std::iter::from_fn(|| queue.next_ready_parallel(1, true, |_| true).unwrap())
            .any(|output| output.name().as_normalized() == name)
    }

    #[test]
    fn waits_for_the_dependency_variant_of_the_same_python() {
        // `lib` and its consumer `app` are both built for two Python versions
        let lib_312 = py_output("lib", "3.12", &[]);
        let lib_313 = py_output("lib", "3.13", &[]);
        let app_312 = py_output("app", "3.12", &["lib"]);
        let mut queue =
            OutputBuildQueue::new(vec![lib_313.clone(), lib_312.clone(), app_312], Vec::new());

        queue.record_processed(built(&lib_313, &[]));
        // the Python 3.13 `lib` cannot be installed with the Python 3.12 `app`
        assert!(!can_start(&mut queue, "app"));
        queue.record_processed(built(&lib_312, &[]));
        assert!(can_start(&mut queue, "app"));
    }

    #[test]
    fn waits_for_the_same_python_variant_of_a_run_dependency() {
        // `app` needs `mid`, whose run dependency `lib` is built for two Pythons;
        // the Python 3.13 `mid` is built, but only the Python 3.12 `lib`
        let lib_312 = py_output("lib", "3.12", &[]);
        let lib_313 = py_output("lib", "3.13", &[]);
        let mid_313 = py_output("mid", "3.13", &[]);
        let app_313 = py_output("app", "3.13", &["mid"]);
        let mut queue = OutputBuildQueue::new(
            vec![lib_312.clone(), lib_313.clone(), mid_313.clone(), app_313],
            Vec::new(),
        );

        queue.record_processed(built(&lib_312, &[]));
        queue.record_processed(built(&mid_313, &["lib"]));
        // the Python 3.12 `lib` cannot be installed with the Python 3.13 `mid`
        assert!(!can_start(&mut queue, "app"));
        queue.record_processed(built(&lib_313, &[]));
        assert!(can_start(&mut queue, "app"));
    }

    #[test]
    fn keeps_the_python_variant_through_a_noarch_dependency() {
        // `app` (Python 3.13) needs the noarch `mid`, which runs with `lib`,
        // built for two Pythons; only the Python 3.12 `lib` is built
        let lib_312 = py_output("lib", "3.12", &[]);
        let lib_313 = py_output("lib", "3.13", &[]);
        let mid = with_variant(
            output("mid", "pyh0_0", &[], &[], false),
            &[("target_platform", "noarch")],
        );
        let app_313 = py_output("app", "3.13", &["mid"]);
        let mut queue = OutputBuildQueue::new(
            vec![lib_312.clone(), lib_313.clone(), mid.clone(), app_313],
            Vec::new(),
        );

        queue.record_processed(built(&lib_312, &[]));
        queue.record_processed(built(&mid, &["lib"]));
        assert!(!can_start(&mut queue, "app"));
        queue.record_processed(built(&lib_313, &[]));
        assert!(can_start(&mut queue, "app"));
    }

    #[test]
    fn noarch_consumer_takes_any_platform_variant() {
        // the noarch `app` agrees with the linux-64 Python 3.12 `lib` only
        // when the platform is ignored; otherwise no `lib` variant agrees
        // and any would do
        let lib_312 = py_output("lib", "3.12", &[]);
        let lib_313 = py_output("lib", "3.13", &[]);
        let app = with_variant(
            output("app", "pyh0_0", &["lib"], &[], false),
            &[("python", "3.12"), ("target_platform", "noarch")],
        );
        let mut queue =
            OutputBuildQueue::new(vec![lib_312.clone(), lib_313.clone(), app], Vec::new());

        queue.record_processed(built(&lib_313, &[]));
        assert!(!can_start(&mut queue, "app"));
        queue.record_processed(built(&lib_312, &[]));
        assert!(can_start(&mut queue, "app"));
    }

    #[test]
    fn keeps_the_python_variant_through_a_skipped_noarch_dependency() {
        // as `keeps_the_python_variant_through_a_noarch_dependency`, but the
        // noarch `mid` already exists
        let lib_312 = py_output("lib", "3.12", &[]);
        let lib_313 = py_output("lib", "3.13", &[]);
        let mid = with_variant(
            output("mid", "pyh0_0", &[], &["lib"], false),
            &[("target_platform", "noarch")],
        );
        let app_313 = py_output("app", "3.13", &["mid"]);
        let mut queue =
            OutputBuildQueue::new(vec![lib_312.clone(), lib_313.clone(), app_313], vec![mid]);

        queue.record_processed(built(&lib_312, &[]));
        assert!(!can_start(&mut queue, "app"));
        queue.record_processed(built(&lib_313, &[]));
        assert!(can_start(&mut queue, "app"));
    }

    #[test]
    fn checks_a_dependency_again_for_each_variant_it_is_installed_for() {
        // `app` reaches the noarch `mid` directly, where any `lib` will do, and
        // through `x`, where `mid` needs the numpy 1.26 `lib`
        let numpy = |name: &str, version: &str, platform: &str| {
            with_variant(
                output(name, "h0_0", &[], &[], false),
                &[("numpy", version), ("target_platform", platform)],
            )
        };
        let lib_126 = numpy("lib", "1.26", "linux-64");
        let lib_200 = numpy("lib", "2.0", "linux-64");
        let x = numpy("x", "1.26", "linux-64");
        let mid = with_variant(
            output("mid", "h0_0", &[], &[], false),
            &[("target_platform", "noarch")],
        );
        let app = with_variant(
            output("app", "h0_0", &["mid", "x"], &[], false),
            &[("target_platform", "linux-64")],
        );
        let mut queue = OutputBuildQueue::new(
            vec![
                lib_126.clone(),
                lib_200.clone(),
                x.clone(),
                mid.clone(),
                app,
            ],
            Vec::new(),
        );

        queue.record_processed(built(&lib_200, &[]));
        queue.record_processed(built(&mid, &["lib"]));
        queue.record_processed(built(&x, &["mid"]));
        assert!(!can_start(&mut queue, "app"));
        queue.record_processed(built(&lib_126, &[]));
        assert!(can_start(&mut queue, "app"));
    }

    #[test]
    fn waits_for_the_dependency_of_the_same_target_platform() {
        let platform = |p| [("target_platform", p)];
        let lib_x64 = with_variant(
            output("lib", "h0_0", &[], &[], false),
            &platform("linux-64"),
        );
        let lib_arm = with_variant(
            output("lib", "h1_0", &[], &[], false),
            &platform("linux-aarch64"),
        );
        let app_arm = with_variant(
            output("app", "h1_0", &["lib"], &[], false),
            &platform("linux-aarch64"),
        );
        let mut queue =
            OutputBuildQueue::new(vec![lib_x64.clone(), lib_arm.clone(), app_arm], Vec::new());

        queue.record_processed(built(&lib_x64, &[]));
        // the linux-64 `lib` cannot be installed with the linux-aarch64 `app`
        assert!(!can_start(&mut queue, "app"));
        queue.record_processed(built(&lib_arm, &[]));
        assert!(can_start(&mut queue, "app"));
    }

    #[test]
    fn uses_an_existing_dependency_of_the_same_python() {
        // the Python 3.12 `lib` already exists (skipped), the Python 3.13 one
        // is planned: the Python 3.12 `app` does not wait for it
        let lib_312 = py_output("lib", "3.12", &[]);
        let lib_313 = py_output("lib", "3.13", &[]);
        let app_312 = py_output("app", "3.12", &["lib"]);
        let mut queue = OutputBuildQueue::new(vec![lib_313, app_312], vec![lib_312]);

        assert!(can_start(&mut queue, "app"));
    }

    #[test]
    fn waits_for_run_dependencies_of_skipped_outputs() {
        let sx_b = output("sx-b", "h0_0", &[], &["sx-c"], false);
        assert_waits_for_dependency_of_skipped(vec![sx_b], &[]);
    }

    #[test]
    fn waits_for_host_dependencies_of_skipped_outputs() {
        // `sx-c` reaches `sx-b`'s run dependencies through a run export
        let sx_b = output("sx-b", "h0_0", &["sx-c"], &[], false);
        assert_waits_for_dependency_of_skipped(vec![sx_b], &[]);
    }

    #[test]
    fn waits_for_dependencies_of_skipped_outputs_in_a_cycle() {
        let sx_b = output("sx-b", "h0_0", &[], &["sx-c"], false);
        assert_waits_for_dependency_of_skipped(vec![sx_b], &["sx-b"]);
    }

    #[test]
    fn waits_for_dependencies_of_every_skipped_variant() {
        // the solver may pick either variant of `sx-b`; only one needs `sx-c`
        let needs_sx_c = output("sx-b", "h1_0", &[], &["sx-c"], false);
        let standalone = output("sx-b", "h2_0", &[], &[], false);
        assert_waits_for_dependency_of_skipped(vec![standalone, needs_sx_c], &[]);
    }

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

    #[test]
    fn take_where_keeps_the_order_of_the_rest() {
        let mut queue = BuildQueue::new([1, 2, 3, 4, 5]);

        assert_eq!(queue.take_where(|n| n % 2 == 0), vec![2, 4]);
        assert_eq!(
            queue.next_ready(|_| Ok::<_, Infallible>(true)),
            Ok(NextReady::Ready(1))
        );
        assert_eq!(queue.len(), 2);
    }

    #[test]
    fn best_ready_picks_the_highest_score_among_ready_items() {
        let mut queue = BuildQueue::new([1, 2, 3, 4]);

        // 4 has the highest score but is not ready; 2 and 3 tie, 2 is earlier.
        let best = queue.best_ready(
            |n| Ok::<_, Infallible>(*n != 4),
            |n| if *n == 1 { 1 } else { 5 },
        );

        assert_eq!(best, Ok(Some(2)));
        assert_eq!(
            queue.next_ready(|_| Ok::<_, Infallible>(true)),
            Ok(NextReady::Ready(1))
        );
        assert_eq!(queue.len(), 2);
    }

    #[test]
    fn skip_blocked_cascades_and_keeps_independent_items() {
        // a failed; b needs a; c needs b; d needs x (not failed); e needs d.
        let required = HashMap::from([
            (name("b"), names(&["a"])),
            (name("c"), names(&["b"])),
            (name("d"), names(&["x"])),
            (name("e"), names(&["d"])),
        ]);
        let mut queue = BuildQueue::new(["b", "c", "d", "e"].map(name));
        let mut failed = names(&["a"]);

        let skipped = skip_blocked(&mut queue, &mut failed, |n| (n, required.get(n)));

        assert_eq!(
            skipped,
            vec![(name("b"), name("a")), (name("c"), name("b"))]
        );
        assert_eq!(failed, names(&["a", "b", "c"]));
        assert_eq!(queue.len(), 2);
    }

    #[test]
    fn best_ready_returns_none_when_nothing_is_ready() {
        let mut queue = BuildQueue::new([1, 2]);

        assert_eq!(
            queue.best_ready(|_| Ok::<_, Infallible>(false), |_| 1),
            Ok(None)
        );
        assert_eq!(queue.len(), 2);
    }

    #[test]
    fn skip_blocked_reports_the_alphabetically_first_cause() {
        let required = HashMap::from([(name("c"), names(&["b", "a"]))]);
        let mut queue = BuildQueue::new([name("c")]);
        let mut failed = names(&["a", "b"]);

        let skipped = skip_blocked(&mut queue, &mut failed, |n| (n, required.get(n)));

        assert_eq!(skipped, vec![(name("c"), name("a"))]);
    }

    #[test]
    fn closure_follows_run_dependencies_and_excludes_itself() {
        // pkg needs lib at build time; lib runs with util; util runs with lib and pkg.
        let run = HashMap::from([
            (name("lib"), vec![name("util")]),
            (name("util"), vec![name("lib"), name("pkg")]),
        ]);

        let closure = closure_without(vec![name("lib")], &run, &name("pkg"));

        assert_eq!(closure, names(&["lib", "util"]));
    }

    #[test]
    fn count_dependents_is_transitive_and_ignores_unplanned_names() {
        // base <- lib <- app; tool needs nothing planned; cycle a <-> b.
        let counts = count_dependents([
            (name("base"), vec![name("external")]),
            (name("lib"), vec![name("base")]),
            (name("app"), vec![name("lib"), name("base")]),
            (name("tool"), vec![name("external")]),
            (name("a"), vec![name("b")]),
            (name("b"), vec![name("a")]),
        ]);

        assert_eq!(counts[&name("base")], 2);
        assert_eq!(counts[&name("lib")], 1);
        assert_eq!(counts[&name("app")], 0);
        assert_eq!(counts[&name("tool")], 0);
        assert_eq!(counts[&name("a")], 1);
        assert_eq!(counts[&name("b")], 1);
        assert!(!counts.contains_key(&name("external")));
    }
}
