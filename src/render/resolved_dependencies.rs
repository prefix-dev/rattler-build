use std::{
    borrow::Cow,
    collections::HashMap,
    fmt::{Display, Formatter},
    sync::Arc,
};

use indicatif::{HumanBytes, MultiProgress, ProgressBar, ProgressStyle};
use rattler::install::Placement;
use rattler_conda_types::{
    ChannelUrl, MatchSpec, NamelessMatchSpec, PackageName, PackageRecord, Platform, RepoDataRecord,
    package::RunExportsJson,
};
use rattler_repodata_gateway::{Gateway, RunExportExtractorError, RunExportsReporter};
use serde::{Deserialize, Serialize};
use serde_with::{DisplayFromStr, serde_as};
use thiserror::Error;

use super::pin::PinError;
use crate::{
    metadata::Output,
    metadata::{BuildConfiguration, build_reindexed_channels},
    package_cache_reporter::PackageCacheReporter,
    recipe::parser::{Dependency, Requirements},
    render::{
        pin::PinArgs,
        solver::{install_packages, solve_environment},
    },
    tool_configuration::{self, Configuration},
};

use super::reporters::GatewayReporter;

/// A enum to keep track of where a given Dependency comes from
#[serde_as]
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(untagged)]
pub enum DependencyInfo {
    /// The dependency is a direct dependency of the package, with a variant
    /// applied from the variant config
    Variant(VariantDependency),

    /// This is a special pin dependency (e.g. `{{ pin_subpackage('foo',
    /// exact=True) }}`
    PinSubpackage(PinSubpackageDependency),

    /// This is a special run_exports dependency (e.g. `{{ pin_compatible('foo')
    /// }}`
    PinCompatible(PinCompatibleDependency),

    /// This is a special run_exports dependency from another package
    RunExport(RunExportDependency),

    /// This is a regular dependency of the package without any modifications
    Source(SourceDependency),
}

/// The dependency is a direct dependency of the package, with a variant applied
/// from the variant config
#[serde_as]
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct VariantDependency {
    /// The key in the config file.
    pub variant: String,

    /// The spec from the config file
    #[serde_as(as = "DisplayFromStr")]
    pub spec: MatchSpec,
}

impl From<VariantDependency> for DependencyInfo {
    fn from(value: VariantDependency) -> Self {
        DependencyInfo::Variant(value)
    }
}

/// This is a special pin dependency (e.g. `{{ pin_subpackage('foo', exact=True)
/// }}`
#[serde_as]
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct PinSubpackageDependency {
    #[serde(rename = "pin_subpackage")]
    pub name: String,

    #[serde(flatten)]
    pub args: PinArgs,

    #[serde_as(as = "DisplayFromStr")]
    pub spec: MatchSpec,
}

impl From<PinSubpackageDependency> for DependencyInfo {
    fn from(value: PinSubpackageDependency) -> Self {
        DependencyInfo::PinSubpackage(value)
    }
}

/// This is a special run_exports dependency (e.g. `{{ pin_compatible('foo') }}`
#[serde_as]
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct PinCompatibleDependency {
    #[serde(rename = "pin_compatible")]
    pub name: String,

    #[serde(flatten)]
    pub args: PinArgs,

    #[serde_as(as = "DisplayFromStr")]
    pub spec: MatchSpec,
}

impl From<PinCompatibleDependency> for DependencyInfo {
    fn from(value: PinCompatibleDependency) -> Self {
        DependencyInfo::PinCompatible(value)
    }
}

/// This is a special run_exports dependency from another package
#[serde_as]
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct RunExportDependency {
    #[serde_as(as = "DisplayFromStr")]
    pub spec: MatchSpec,
    pub from: String,
    #[serde(rename = "run_export")]
    pub source_package: String,
}

impl From<RunExportDependency> for DependencyInfo {
    fn from(value: RunExportDependency) -> Self {
        DependencyInfo::RunExport(value)
    }
}

/// This is a regular dependency of the package without any modifications
#[serde_as]
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct SourceDependency {
    #[serde(rename = "source")]
    #[serde_as(as = "DisplayFromStr")]
    pub spec: MatchSpec,
}

impl From<SourceDependency> for DependencyInfo {
    fn from(value: SourceDependency) -> Self {
        DependencyInfo::Source(value)
    }
}

impl DependencyInfo {
    /// Get the matchspec from a dependency info
    pub fn spec(&self) -> &MatchSpec {
        match self {
            DependencyInfo::Variant(spec) => &spec.spec,
            DependencyInfo::PinSubpackage(spec) => &spec.spec,
            DependencyInfo::PinCompatible(spec) => &spec.spec,
            DependencyInfo::RunExport(spec) => &spec.spec,
            DependencyInfo::Source(spec) => &spec.spec,
        }
    }

    pub fn render(&self, long: bool) -> String {
        if !long {
            match self {
                DependencyInfo::Variant(spec) => format!("{} (V)", &spec.spec),
                DependencyInfo::PinSubpackage(spec) => format!("{} (PS)", &spec.spec),
                DependencyInfo::PinCompatible(spec) => format!("{} (PC)", &spec.spec),
                DependencyInfo::RunExport(spec) => format!(
                    "{} (RE of [{}: {}])",
                    &spec.spec, &spec.from, &spec.source_package
                ),
                DependencyInfo::Source(spec) => spec.spec.to_string(),
            }
        } else {
            match self {
                DependencyInfo::Variant(spec) => format!("{} (from variant config)", &spec.spec),
                DependencyInfo::PinSubpackage(spec) => {
                    format!("{} (from pin subpackage)", &spec.spec)
                }
                DependencyInfo::PinCompatible(spec) => {
                    format!("{} (from pin compatible)", &spec.spec)
                }
                DependencyInfo::RunExport(spec) => format!(
                    "{} (run export by {} in {} env)",
                    &spec.spec, &spec.from, &spec.source_package
                ),
                DependencyInfo::Source(spec) => spec.spec.to_string(),
            }
        }
    }

    pub fn as_variant(&self) -> Option<&VariantDependency> {
        match self {
            DependencyInfo::Variant(spec) => Some(spec),
            _ => None,
        }
    }

    pub fn as_source(&self) -> Option<&SourceDependency> {
        match self {
            DependencyInfo::Source(spec) => Some(spec),
            _ => None,
        }
    }

    pub fn as_run_export(&self) -> Option<&RunExportDependency> {
        match self {
            DependencyInfo::RunExport(spec) => Some(spec),
            _ => None,
        }
    }

    pub fn as_pin_subpackage(&self) -> Option<&PinSubpackageDependency> {
        match self {
            DependencyInfo::PinSubpackage(spec) => Some(spec),
            _ => None,
        }
    }

    pub fn as_pin_compatible(&self) -> Option<&PinCompatibleDependency> {
        match self {
            DependencyInfo::PinCompatible(spec) => Some(spec),
            _ => None,
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct FinalizedRunDependencies {
    #[serde(default)]
    pub depends: Vec<DependencyInfo>,
    #[serde(default)]
    pub constraints: Vec<DependencyInfo>,
    #[serde(default, skip_serializing_if = "RunExportsJson::is_empty")]
    pub run_exports: RunExportsJson,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ResolvedDependencies {
    pub specs: Vec<DependencyInfo>,
    pub resolved: Vec<RepoDataRecord>,
}

fn short_channel(channel: Option<&str>) -> String {
    let channel = channel.unwrap_or_default();
    if channel.contains('/') {
        channel
            .rsplit('/')
            .find(|s| !s.is_empty())
            .unwrap_or_default()
            .to_string()
    } else {
        channel.to_string()
    }
}

impl ResolvedDependencies {
    pub fn to_table(&self, table: comfy_table::Table, long: bool) -> comfy_table::Table {
        let mut table = table;
        table.set_header(vec![
            "Package", "Spec", "Version", "Build", "Channel", "Size",
        ]);
        let column = table.column_mut(5).expect("This should be column two");
        column.set_cell_alignment(comfy_table::CellAlignment::Right);

        let resolved_w_specs = self
            .resolved
            .iter()
            .map(|r| {
                let spec = self.specs.iter().find(|s| {
                    s.spec().name.as_ref()
                        == Some(&rattler_conda_types::PackageNameMatcher::Exact(
                            r.package_record.name.clone(),
                        ))
                });

                if let Some(s) = spec {
                    (r, Some(s))
                } else {
                    (r, None)
                }
            })
            .collect::<Vec<_>>();

        let (mut explicit, mut transient): (Vec<_>, Vec<_>) =
            resolved_w_specs.into_iter().partition(|(_, s)| s.is_some());

        explicit.sort_by(|(a, _), (b, _)| a.package_record.name.cmp(&b.package_record.name));
        transient.sort_by(|(a, _), (b, _)| a.package_record.name.cmp(&b.package_record.name));

        for (record, dep_info) in &explicit {
            table.add_row([
                record.package_record.name.as_normalized().to_string(),
                dep_info
                    .expect("partition contains only values with Some")
                    .render(long),
                record.package_record.version.to_string(),
                record.package_record.build.to_string(),
                short_channel(record.channel.as_deref()),
                record
                    .package_record
                    .size
                    .map(|s| HumanBytes(s).to_string())
                    .unwrap_or_default(),
            ]);
        }
        for (record, _) in &transient {
            table.add_row([
                record.package_record.name.as_normalized().to_string(),
                "".to_string(),
                record.package_record.version.to_string(),
                record.package_record.build.to_string(),
                short_channel(record.channel.as_deref()),
                record
                    .package_record
                    .size
                    .map(|s| HumanBytes(s).to_string())
                    .unwrap_or_default(),
            ]);
        }
        table
    }

    /// Collect run exports from this environment
    /// If `direct_only` is set to true, only the run exports of the direct
    /// dependencies are collected
    fn run_exports(&self, direct_only: bool) -> HashMap<PackageName, RunExportsJson> {
        let mut result = HashMap::new();
        for record in &self.resolved {
            // If there are no run exports, we don't need to do anything.
            let Some(run_exports) = &record.package_record.run_exports else {
                continue;
            };

            // If the specific package is a transitive dependency we ignore the run exports
            if direct_only
                && !self
                    .specs
                    .iter()
                    // Run export dependencies are not direct dependencies
                    .filter(|s| !matches!(s, DependencyInfo::RunExport(_)))
                    .any(|s| {
                        s.spec().name.as_ref()
                            == Some(&rattler_conda_types::PackageNameMatcher::Exact(
                                record.package_record.name.clone(),
                            ))
                    })
            {
                continue;
            }

            result.insert(record.package_record.name.clone(), run_exports.clone());
        }
        result
    }
}
impl Display for ResolvedDependencies {
    fn fmt(&self, f: &mut Formatter<'_>) -> std::fmt::Result {
        let mut table = comfy_table::Table::new();
        table
            .load_preset(comfy_table::presets::UTF8_FULL_CONDENSED)
            .apply_modifier(comfy_table::modifiers::UTF8_ROUND_CORNERS);
        write!(f, "{}", self.to_table(table, false))
    }
}

/// Render dependencies as (name, rest) pairs, sorted by name.
/// When multiple dependencies have the same name, they will be grouped together.
/// Empty specs are shown as "*" to indicate "any version".
fn render_grouped_dependencies(deps: &[DependencyInfo], long: bool) -> Vec<(String, String)> {
    // Collect all dependencies as (name, rest) pairs
    // The rendered string format is "name spec (annotation)" so we split on first space
    let mut items: Vec<(String, String)> = deps
        .iter()
        .map(|d| {
            let rendered = d.render(long);
            // Split on first space to separate name from the rest
            if let Some((name, rest)) = rendered.split_once(' ') {
                (name.to_string(), rest.to_string())
            } else {
                // No space means just a name with no version spec
                (rendered.clone(), String::new())
            }
        })
        .collect();

    // Replace empty specs with "*" to indicate "any version"
    for (_, rest) in &mut items {
        if rest.is_empty() {
            *rest = "*".to_string();
        }
    }

    // Sort alphabetically by name
    items.sort_by(|a, b| a.0.cmp(&b.0));

    items
}

impl FinalizedRunDependencies {
    pub fn to_table(&self, table: comfy_table::Table, long: bool) -> comfy_table::Table {
        let mut table = table;
        table
            .set_content_arrangement(comfy_table::ContentArrangement::Dynamic)
            .set_header(vec!["Name", "Spec"]);

        // Helper function to add a section header
        fn add_section_header(table: &mut comfy_table::Table, section_name: &str) {
            let mut row = comfy_table::Row::new();
            row.add_cell(
                comfy_table::Cell::new(section_name).add_attribute(comfy_table::Attribute::Bold),
            );
            table.add_row(row);
        }

        // Helper function to add grouped dependencies
        // When multiple deps have the same name, only the first shows the name
        fn add_grouped_items(table: &mut comfy_table::Table, items: &[(String, String)]) {
            let mut prev_name: Option<&str> = None;
            for (name, rest) in items {
                // Only show name if different from previous
                let display_name = if prev_name == Some(name.as_str()) {
                    ""
                } else {
                    prev_name = Some(name.as_str());
                    name.as_str()
                };

                table.add_row(vec![display_name, rest.as_str()]);
            }
        }

        // Helper function to add simple string items
        fn add_simple_items(table: &mut comfy_table::Table, items: &[String]) {
            for item in items {
                table.add_row(item.splitn(2, ' ').collect::<Vec<&str>>());
            }
        }

        let mut has_previous_section = false;

        // Add dependencies section (grouped by name)
        let depends_rendered = render_grouped_dependencies(&self.depends, long);
        if !depends_rendered.is_empty() {
            add_section_header(&mut table, "Run dependencies");
            add_grouped_items(&mut table, &depends_rendered);
            has_previous_section = true;
        }

        // Add constraints section (grouped by name)
        let constraints_rendered = render_grouped_dependencies(&self.constraints, long);
        if !constraints_rendered.is_empty() {
            if has_previous_section {
                table.add_row(vec!["", ""]);
            }
            add_section_header(&mut table, "Run constraints");
            add_grouped_items(&mut table, &constraints_rendered);
            has_previous_section = true;
        }

        // Add run exports sections if not empty
        if !self.run_exports.is_empty() {
            let sections = [
                ("Weak", &self.run_exports.weak),
                ("Strong", &self.run_exports.strong),
                ("Noarch", &self.run_exports.noarch),
                ("Weak constrains", &self.run_exports.weak_constrains),
                ("Strong constrains", &self.run_exports.strong_constrains),
            ];

            for (name, exports) in sections {
                if !exports.is_empty() {
                    if has_previous_section {
                        table.add_row(vec!["", ""]);
                    }
                    add_section_header(&mut table, &format!("Run exports ({name})"));
                    add_simple_items(&mut table, exports);
                    has_previous_section = true;
                }
            }
        }

        table
    }
}

impl Display for FinalizedRunDependencies {
    fn fmt(&self, f: &mut Formatter<'_>) -> std::fmt::Result {
        let mut table = comfy_table::Table::new();
        table
            .load_preset(comfy_table::presets::UTF8_FULL_CONDENSED)
            .apply_modifier(comfy_table::modifiers::UTF8_ROUND_CORNERS);
        write!(f, "{}", self.to_table(table, false))
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct FinalizedDependencies {
    pub build: Option<ResolvedDependencies>,
    pub host: Option<ResolvedDependencies>,
    pub run: FinalizedRunDependencies,
}

#[derive(Error, Debug)]
pub enum ResolveError {
    #[error("Failed to get finalized dependencies")]
    FinalizedDependencyNotFound,

    #[error("Failed to resolve dependencies: {0}")]
    DependencyResolutionError(#[from] anyhow::Error),

    #[error("Could not collect run exports")]
    CouldNotCollectRunExports(#[from] RunExportExtractorError),

    #[error("Could not parse match spec: {0}")]
    MatchSpecParseError(#[from] rattler_conda_types::ParseMatchSpecError),

    #[error("Could not parse version spec for variant key {0}: {1}")]
    VariantSpecParseError(String, rattler_conda_types::ParseMatchSpecError),

    #[error("Could not apply pin: {0}")]
    PinApplyError(#[from] PinError),

    #[error("Could not apply pin_subpackage. The following subpackage is not available: {}", .0.as_normalized())]
    PinSubpackageNotFound(PackageName),

    #[error("Could not apply pin_compatible. The following package is not part of the solution: {}", .0.as_normalized())]
    PinCompatibleNotFound(PackageName),

    #[error("Compiler configuration error: {0}")]
    CompilerError(String),

    #[error("Could not reindex channels: {0}")]
    RefreshChannelError(std::io::Error),
}

/// Controls whether to download missing run exports during dependency resolution
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum RunExportsDownload {
    /// Download packages to extract run exports when they are missing
    DownloadMissing,
    /// Skip downloading packages for run exports extraction
    SkipDownload,
}

/// Apply a variant to a dependency list and resolve all pin_subpackage and
/// compiler dependencies
pub fn apply_variant(
    raw_specs: &[Dependency],
    build_configuration: &BuildConfiguration,
    compatibility_specs: &HashMap<PackageName, PackageRecord>,
    build_time: bool,
) -> Result<Vec<DependencyInfo>, ResolveError> {
    let variant = &build_configuration.variant;
    let subpackages = &build_configuration.subpackages;

    raw_specs
        .iter()
        .map(|s| {
            match s {
                Dependency::Spec(m) => {
                    let m = m.clone();
                    if build_time
                        && m.version.is_none()
                        && m.build.is_none()
                        && let Some(name) = &m.name
                        && let Some(version) = variant.get(&name.to_string().into())
                    {
                        // if the variant starts with an alphanumeric character,
                        // we have to add a '=' to the version spec
                        let mut spec = version.to_string();

                        // check if all characters are alphanumeric or ., in that case add
                        // a '=' to get "startswith" behavior
                        if spec.chars().all(|c| c.is_alphanumeric() || c == '.') {
                            spec = format!("={spec}");
                        }

                        let variant = name.to_string();
                        let spec: NamelessMatchSpec = spec
                            .parse()
                            .map_err(|e| ResolveError::VariantSpecParseError(variant.clone(), e))?;

                        let spec = MatchSpec::from_nameless(spec, Some(name.clone()));

                        return Ok(VariantDependency { spec, variant }.into());
                    }
                    Ok(SourceDependency { spec: m }.into())
                }
                Dependency::PinSubpackage(pin) => {
                    let name = &pin.pin_value().name;
                    let subpackage = subpackages
                        .get(name)
                        .ok_or(ResolveError::PinSubpackageNotFound(name.clone()))?;
                    let pinned = pin
                        .pin_value()
                        .apply(&subpackage.version, &subpackage.build_string)?;
                    Ok(PinSubpackageDependency {
                        spec: pinned,
                        name: name.as_normalized().to_string(),
                        args: pin.pin_value().args.clone(),
                    }
                    .into())
                }
                Dependency::PinCompatible(pin) => {
                    let name = &pin.pin_value().name;
                    let pin_package = compatibility_specs
                        .get(name)
                        .ok_or(ResolveError::PinCompatibleNotFound(name.clone()))?;

                    let pinned = pin
                        .pin_value()
                        .apply(&pin_package.version, &pin_package.build)?;
                    Ok(PinCompatibleDependency {
                        spec: pinned,
                        name: name.as_normalized().to_string(),
                        args: pin.pin_value().args.clone(),
                    }
                    .into())
                }
            }
        })
        .collect()
}

use rattler::package_cache::CacheReporter;
use rattler_repodata_gateway::DownloadReporter;

struct RunExportsProgressReporter {
    repodata_reporter: GatewayReporter,
    package_cache_reporter: PackageCacheReporter,
}

impl RunExportsProgressReporter {
    fn new(
        repodata_reporter: GatewayReporter,
        package_cache_reporter: PackageCacheReporter,
    ) -> Self {
        Self {
            repodata_reporter,
            package_cache_reporter,
        }
    }
}

impl RunExportsReporter for RunExportsProgressReporter {
    fn download_reporter(&self) -> Option<&dyn DownloadReporter> {
        Some(&self.repodata_reporter)
    }

    fn create_package_download_reporter(
        &self,
        repo_data_record: &RepoDataRecord,
    ) -> Option<Box<dyn CacheReporter>> {
        let mut reporter = self.package_cache_reporter.clone();
        let entry = reporter.add(repo_data_record);
        Some(Box::new(entry) as Box<dyn CacheReporter>)
    }
}

/// Collect run exports from the package cache and add them to the package
/// records.
async fn ensure_run_exports(
    records: &mut [RepoDataRecord],
    gateway: &Gateway,
    multi_progress: MultiProgress,
    progress_prefix: impl Into<Cow<'static, str>>,
    top_level_pb: Option<ProgressBar>,
    progress_style: ProgressStyle,
    finish_style: ProgressStyle,
) -> Result<(), RunExportExtractorError> {
    let progress_prefix: Cow<'static, str> = progress_prefix.into();
    let placement = top_level_pb
        .as_ref()
        .map(|pb| Placement::After(pb.clone()))
        .unwrap_or(Placement::End);

    let repodata_reporter = GatewayReporter::builder()
        .with_multi_progress(multi_progress.clone())
        .with_progress_template(progress_style.clone())
        .with_finish_template(finish_style.clone())
        .with_placement(placement.clone())
        .finish();

    let package_cache_reporter =
        PackageCacheReporter::new(multi_progress, placement).with_prefix(progress_prefix);

    let reporter: Arc<dyn RunExportsReporter> = Arc::new(RunExportsProgressReporter::new(
        repodata_reporter,
        package_cache_reporter,
    ));

    gateway
        .ensure_run_exports(records.iter_mut(), Some(reporter))
        .await
}

pub async fn install_environments(
    output: &Output,
    dependencies: &FinalizedDependencies,
    tool_configuration: &tool_configuration::Configuration,
) -> Result<(), ResolveError> {
    const EMPTY_RECORDS: Vec<RepoDataRecord> = Vec::new();
    install_packages(
        "build",
        dependencies
            .build
            .as_ref()
            .map(|deps| &deps.resolved)
            .unwrap_or(&EMPTY_RECORDS),
        output.build_configuration.build_platform.platform,
        &output.build_configuration.directories.build_prefix,
        tool_configuration,
    )
    .await?;

    install_packages(
        "host",
        dependencies
            .host
            .as_ref()
            .map(|deps| &deps.resolved)
            .unwrap_or(&EMPTY_RECORDS),
        output.build_configuration.host_platform.platform,
        &output.build_configuration.directories.host_prefix,
        tool_configuration,
    )
    .await?;

    Ok(())
}

/// This function renders the run exports into `RunExportsJson` format
/// This function applies any variant information or `pin_subpackage`
/// specifications to the run exports.
fn render_run_exports(
    output: &Output,
    compatibility_specs: &HashMap<PackageName, PackageRecord>,
) -> Result<RunExportsJson, ResolveError> {
    let render_run_exports = |run_export: &[Dependency]| -> Result<Vec<String>, ResolveError> {
        let rendered = apply_variant(
            run_export,
            &output.build_configuration,
            compatibility_specs,
            false,
        )?;
        Ok(rendered
            .iter()
            .map(|dep| dep.spec().to_string())
            .collect::<Vec<_>>())
    };

    let run_exports = output.recipe.requirements().run_exports();

    if !run_exports.is_empty() {
        Ok(RunExportsJson {
            strong: render_run_exports(run_exports.strong())?,
            weak: render_run_exports(run_exports.weak())?,
            noarch: render_run_exports(run_exports.noarch())?,
            strong_constrains: render_run_exports(run_exports.strong_constraints())?,
            weak_constrains: render_run_exports(run_exports.weak_constraints())?,
        })
    } else {
        Ok(RunExportsJson::default())
    }
}

/// This function resolves the dependencies of a recipe.
/// To do this, we have to run a couple of steps:
///
/// 1. Apply the variants to the dependencies, and compiler & pin_subpackage
///    specs
/// 2. Extend the dependencies with the run exports of the dependencies "above"
/// 3. Resolve the dependencies
/// 4. Download the packages
/// 5. Extract the run exports from the downloaded packages (for the next
///    environment)
pub(crate) async fn resolve_dependencies(
    requirements: &Requirements,
    output: &Output,
    channels: &[ChannelUrl],
    tool_configuration: &tool_configuration::Configuration,
    download_missing_run_exports: RunExportsDownload,
) -> Result<FinalizedDependencies, ResolveError> {
    let merge_build_host = output.recipe.build().merge_build_and_host_envs();

    let mut compatibility_specs = HashMap::new();

    let gateway = if download_missing_run_exports == RunExportsDownload::DownloadMissing {
        let client = tool_configuration.client.get_client().clone();
        let package_cache = tool_configuration.package_cache.clone();
        Some(
            Gateway::builder()
                .with_max_concurrent_requests(50)
                .with_client(client)
                .with_package_cache(package_cache)
                .finish(),
        )
    } else {
        None
    };

    let build_env = if !requirements.build.is_empty() && !merge_build_host {
        let build_env_specs = apply_variant(
            requirements.build(),
            &output.build_configuration,
            &compatibility_specs,
            true,
        )?;

        let match_specs = build_env_specs
            .iter()
            .map(|s| s.spec().clone())
            .collect::<Vec<_>>();

        let mut resolved = solve_environment(
            "build",
            &match_specs,
            &output.build_configuration.build_platform,
            channels,
            tool_configuration,
            output.build_configuration.channel_priority,
            output.build_configuration.solve_strategy,
            output.build_configuration.exclude_newer,
        )
        .await
        .map_err(ResolveError::from)?;

        // Optionally add run exports to records that don't have them yet by
        // downloading packages and extracting run_exports.json
        if download_missing_run_exports == RunExportsDownload::DownloadMissing {
            tool_configuration
                .fancy_log_handler
                .wrap_in_progress_async_with_progress("Collecting run exports", |pb| {
                    let progress_style = tool_configuration.fancy_log_handler.default_bytes_style();
                    let finish_style = tool_configuration
                        .fancy_log_handler
                        .finished_progress_style();
                    ensure_run_exports(
                        &mut resolved,
                        gateway.as_ref().unwrap(),
                        tool_configuration
                            .fancy_log_handler
                            .multi_progress()
                            .clone(),
                        tool_configuration
                            .fancy_log_handler
                            .with_indent_levels("  "),
                        Some(pb),
                        progress_style,
                        finish_style,
                    )
                })
                .await
                .map_err(ResolveError::CouldNotCollectRunExports)?;
        }

        resolved.iter().for_each(|r| {
            compatibility_specs.insert(r.package_record.name.clone(), r.package_record.clone());
        });

        Some(ResolvedDependencies {
            specs: build_env_specs,
            resolved,
        })
    } else {
        None
    };

    // host env
    let mut host_env_specs = apply_variant(
        requirements.host(),
        &output.build_configuration,
        &compatibility_specs,
        true,
    )?;

    // Apply the strong run exports from the build environment to the host
    // environment
    let mut build_run_exports = HashMap::new();
    if let Some(build_env) = &build_env {
        build_run_exports.extend(build_env.run_exports(true));
    }

    let output_ignore_run_exports = requirements.ignore_run_exports(None);
    let mut build_run_exports = output_ignore_run_exports.filter(&build_run_exports, "build")?;

    if let Some(cache) = &output.finalized_cache_dependencies
        && let Some(cache_build_env) = &cache.build
    {
        let cache_build_run_exports = cache_build_env.run_exports(true);
        let filtered = output
            .recipe
            .cache
            .as_ref()
            .expect("recipe should have cache section")
            .requirements
            .ignore_run_exports(Some(&output_ignore_run_exports))
            .filter(&cache_build_run_exports, "cache-build")?;
        build_run_exports.extend(&filtered);
    }

    host_env_specs.extend(build_run_exports.strong.iter().cloned());

    let mut match_specs = host_env_specs
        .iter()
        .map(|s| s.spec().clone())
        .collect::<Vec<_>>();
    if merge_build_host {
        // add the requirements of build to host
        let specs = apply_variant(
            requirements.build(),
            &output.build_configuration,
            &compatibility_specs,
            true,
        )?;
        match_specs.extend(specs.iter().map(|s| s.spec().clone()));
    }

    let host_env = if !match_specs.is_empty() {
        let mut resolved = solve_environment(
            "host",
            &match_specs,
            &output.build_configuration.host_platform,
            channels,
            tool_configuration,
            output.build_configuration.channel_priority,
            output.build_configuration.solve_strategy,
            output.build_configuration.exclude_newer,
        )
        .await
        .map_err(ResolveError::from)?;

        // Optionally add run exports to records that don't have them yet by
        // downloading packages and extracting run_exports.json
        if download_missing_run_exports == RunExportsDownload::DownloadMissing {
            tool_configuration
                .fancy_log_handler
                .wrap_in_progress_async_with_progress("Collecting run exports", |pb| {
                    let progress_style = tool_configuration.fancy_log_handler.default_bytes_style();
                    let finish_style = tool_configuration
                        .fancy_log_handler
                        .finished_progress_style();
                    ensure_run_exports(
                        &mut resolved,
                        gateway.as_ref().unwrap(),
                        tool_configuration
                            .fancy_log_handler
                            .multi_progress()
                            .clone(),
                        tool_configuration
                            .fancy_log_handler
                            .with_indent_levels("  "),
                        Some(pb),
                        progress_style,
                        finish_style,
                    )
                })
                .await
                .map_err(ResolveError::CouldNotCollectRunExports)?;
        }

        resolved.iter().for_each(|r| {
            compatibility_specs.insert(r.package_record.name.clone(), r.package_record.clone());
        });

        Some(ResolvedDependencies {
            specs: host_env_specs,
            resolved,
        })
    } else {
        None
    };

    let mut depends = apply_variant(
        &requirements.run,
        &output.build_configuration,
        &compatibility_specs,
        false,
    )?;

    let mut constraints = apply_variant(
        &requirements.run_constraints,
        &output.build_configuration,
        &compatibility_specs,
        false,
    )?;

    // add in dependencies from the finalized cache
    if let Some(finalized_cache) = &output.finalized_cache_dependencies {
        tracing::info!(
            "Adding dependencies from finalized cache: {:?}",
            finalized_cache.run.depends
        );

        depends = depends
            .iter()
            .chain(finalized_cache.run.depends.iter())
            .filter(|c| !matches!(c, DependencyInfo::RunExport(_)))
            .cloned()
            .collect();

        tracing::info!(
            "Adding constraints from finalized cache: {:?}",
            finalized_cache.run.constraints
        );
        constraints = constraints
            .iter()
            .chain(finalized_cache.run.constraints.iter())
            .filter(|c| !matches!(c, DependencyInfo::RunExport(_)))
            .cloned()
            .collect();
    }

    let rendered_run_exports = render_run_exports(output, &compatibility_specs)?;

    let mut host_run_exports = HashMap::new();

    // Grab the host run exports from the cache
    // Add in the host run exports from the current output
    if let Some(host_env) = &host_env {
        host_run_exports.extend(host_env.run_exports(true));
    }

    // And filter the run exports
    let mut host_run_exports = output_ignore_run_exports.filter(&host_run_exports, "host")?;

    if let Some(cache) = &output.finalized_cache_dependencies
        && let Some(cache_host_env) = &cache.host
    {
        let cache_host_run_exports = cache_host_env.run_exports(true);
        let filtered = output
            .recipe
            .cache
            .as_ref()
            .expect("recipe should have cache section")
            .requirements
            .ignore_run_exports(Some(&output_ignore_run_exports))
            .filter(&cache_host_run_exports, "cache-host")?;
        host_run_exports.extend(&filtered);
    }

    // add the host run exports to the run dependencies
    if output.target_platform() == &Platform::NoArch {
        // ignore build noarch depends
        depends.extend(host_run_exports.noarch.iter().cloned());
    } else {
        depends.extend(build_run_exports.strong.iter().cloned());
        depends.extend(host_run_exports.strong.iter().cloned());
        depends.extend(host_run_exports.weak.iter().cloned());
        // add the constraints
        constraints.extend(build_run_exports.strong_constraints.iter().cloned());
        constraints.extend(host_run_exports.strong_constraints.iter().cloned());
        constraints.extend(host_run_exports.weak_constraints.iter().cloned());
    }

    if let Some(cache) = &output.finalized_cache_dependencies {
        // add in the run exports from the cache
        // filter run dependencies that came from run exports
        let ignore_run_exports = requirements.ignore_run_exports(None);
        // Note: these run exports are already filtered
        let _cache_run_exports = cache.run.depends.iter().filter(|c| match c {
            DependencyInfo::RunExport(run_export) => {
                let source_package: Option<PackageName> = run_export.source_package.parse().ok();
                let spec_name = &run_export.spec.name;

                let by_name = spec_name
                    .as_ref()
                    .and_then(|n| match n {
                        rattler_conda_types::PackageNameMatcher::Exact(name) => {
                            Some(ignore_run_exports.by_name().contains(name))
                        }
                        _ => None,
                    })
                    .unwrap_or(false);
                let by_package = source_package
                    .map(|s| ignore_run_exports.from_package().contains(&s))
                    .unwrap_or(false);

                !by_name && !by_package
            }
            _ => false,
        });
    }

    let run_specs = FinalizedRunDependencies {
        depends,
        constraints,
        run_exports: rendered_run_exports,
    };

    // log a table of the rendered run dependencies
    if run_specs.depends.is_empty() && run_specs.constraints.is_empty() {
        tracing::info!("\nFinalized run dependencies: this output has no run dependencies");
    } else {
        tracing::info!(
            "\nFinalized run dependencies ({}):\n{}",
            output.identifier(),
            run_specs
        );
    }

    Ok(FinalizedDependencies {
        build: build_env,
        host: host_env,
        run: run_specs,
    })
}

impl Output {
    /// Resolve the dependencies for this output
    pub async fn resolve_dependencies(
        self,
        tool_configuration: &tool_configuration::Configuration,
        download_missing_run_exports: RunExportsDownload,
    ) -> Result<Output, ResolveError> {
        let span = tracing::info_span!("Resolving environments");
        let _enter = span.enter();

        if self.finalized_dependencies.is_some() {
            return Ok(self);
        }

        let channels = build_reindexed_channels(&self.build_configuration, tool_configuration)
            .await
            .map_err(ResolveError::RefreshChannelError)?;

        let finalized_dependencies = resolve_dependencies(
            self.recipe.requirements(),
            &self,
            &channels,
            tool_configuration,
            download_missing_run_exports,
        )
        .await?;

        // The output with the resolved dependencies
        Ok(Output {
            finalized_dependencies: Some(finalized_dependencies),
            ..self.clone()
        })
    }

    /// Install the environments of the outputs. Assumes that the dependencies
    /// for the environment have already been resolved.
    pub async fn install_environments(
        &self,
        tool_configuration: &Configuration,
    ) -> Result<(), ResolveError> {
        let dependencies = self
            .finalized_dependencies
            .as_ref()
            .ok_or(ResolveError::FinalizedDependencyNotFound)?;

        if tool_configuration.environments_externally_managed {
            let span = tracing::info_span!(
                "Externally resolved dependencies",
                recipe = self.identifier()
            );
            let _enter = span.enter();
            if let Some(build) = &dependencies.build {
                tracing::info!(
                    "\nResolved build dependencies({}):\n{}",
                    self.identifier(),
                    build
                );
            }
            if let Some(host) = &dependencies.host {
                tracing::info!(
                    "Resolved host dependencies({}):\n{}",
                    self.identifier(),
                    host
                );
            }
            tracing::info!(
                "Resolved run dependencies({}):\n{}",
                self.identifier(),
                dependencies.run
            );
            return Ok(());
        }

        install_environments(self, dependencies, tool_configuration).await
    }
}

#[cfg(test)]
mod tests {
    use rattler_conda_types::ParseStrictness;

    // test rendering of DependencyInfo
    use super::*;

    #[test]
    fn test_dependency_info_render() {
        let dep_info: Vec<DependencyInfo> = vec![
            SourceDependency {
                spec: MatchSpec::from_str("xyz", ParseStrictness::Strict).unwrap(),
            }
            .into(),
            VariantDependency {
                spec: MatchSpec::from_str("foo", ParseStrictness::Strict).unwrap(),
                variant: "bar".to_string(),
            }
            .into(),
            PinSubpackageDependency {
                name: "baz".to_string(),
                spec: MatchSpec::from_str("baz", ParseStrictness::Strict).unwrap(),
                args: PinArgs {
                    upper_bound: Some("x.x".parse().unwrap()),
                    lower_bound: Some("x.x.x".parse().unwrap()),
                    exact: true,
                    ..Default::default()
                },
            }
            .into(),
            PinCompatibleDependency {
                name: "bat".to_string(),
                spec: MatchSpec::from_str("bat", ParseStrictness::Strict).unwrap(),
                args: PinArgs {
                    upper_bound: Some("x.x".parse().unwrap()),
                    lower_bound: Some("x.x.x".parse().unwrap()),
                    exact: true,
                    ..Default::default()
                },
            }
            .into(),
        ];
        let yaml_str = serde_yaml::to_string(&dep_info).unwrap();
        insta::assert_snapshot!(yaml_str);

        // test deserialize
        let dep_info: Vec<DependencyInfo> = serde_yaml::from_str(&yaml_str).unwrap();
        assert_eq!(dep_info.len(), 4);
        assert!(matches!(dep_info[0], DependencyInfo::Source(_)));
        assert!(matches!(dep_info[1], DependencyInfo::Variant(_)));
        assert!(matches!(dep_info[2], DependencyInfo::PinSubpackage(_)));
        assert!(matches!(dep_info[3], DependencyInfo::PinCompatible(_)));
    }
}
