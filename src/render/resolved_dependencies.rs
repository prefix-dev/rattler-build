use std::{
    collections::HashMap,
    fmt::{Display, Formatter},
    fs,
    path::Path,
    str::FromStr,
};

use crate::{
    metadata::{BuildConfiguration, Output},
    tool_configuration,
};
use indicatif::HumanBytes;
use rattler::package_cache::CacheKey;
use rattler_conda_types::{
    package::{PackageFile, RunExportsJson},
    MatchSpec, PackageName, ParseStrictness, Platform, RepoDataRecord, StringMatcher, Version,
    VersionSpec,
};
use rattler_conda_types::{version_spec::ParseVersionSpecError, PackageRecord};
use serde::{Deserialize, Serialize};
use thiserror::Error;

use super::{pin::PinError, solver::create_environment};
use crate::recipe::parser::Dependency;
use crate::render::solver::install_packages;
use serde_with::{serde_as, DisplayFromStr};

/// A enum to keep track of where a given Dependency comes from
#[serde_as]
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(untagged)]
pub enum DependencyInfo {
    /// The dependency is a direct dependency of the package, with a variant applied
    /// from the variant config
    Variant(VariantDependency),

    /// This is a special compiler dependency (e.g. `{{ compiler('c') }}`
    Compiler(CompilerDependency),

    /// This is a special pin dependency (e.g. `{{ pin_subpackage('foo', exact=True) }}`
    PinSubpackage(PinSubpackageDependency),

    /// This is a special run_exports dependency (e.g. `{{ pin_compatible('foo') }}`
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

/// This is a special compiler dependency (e.g. `{{ compiler('c') }}`
#[serde_as]
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct CompilerDependency {
    /// The language in the `{{ compiler('c') }}` call.
    #[serde(rename = "compiler")]
    pub language: String,

    /// The resolved compiler spec
    #[serde_as(as = "DisplayFromStr")]
    pub spec: MatchSpec,
}

impl From<CompilerDependency> for DependencyInfo {
    fn from(value: CompilerDependency) -> Self {
        DependencyInfo::Compiler(value)
    }
}

/// This is a special pin dependency (e.g. `{{ pin_subpackage('foo', exact=True) }}`
#[serde_as]
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct PinSubpackageDependency {
    #[serde(rename = "pin_subpackage")]
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
            DependencyInfo::Compiler(spec) => &spec.spec,
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
                DependencyInfo::Compiler(spec) => format!("{} (C)", &spec.spec),
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
                DependencyInfo::Compiler(spec) => format!("{} (from compiler)", &spec.spec),
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

    pub fn as_compiler(&self) -> Option<&CompilerDependency> {
        match self {
            DependencyInfo::Compiler(spec) => Some(spec),
            _ => None,
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct FinalizedRunDependencies {
    pub depends: Vec<DependencyInfo>,
    pub constrains: Vec<DependencyInfo>,
    pub run_exports: Option<RunExportsJson>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ResolvedDependencies {
    pub specs: Vec<DependencyInfo>,
    pub resolved: Vec<RepoDataRecord>,
    pub run_exports: HashMap<PackageName, RunExportsJson>,
}

fn short_channel(channel: &str) -> String {
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
                let spec = self
                    .specs
                    .iter()
                    .find(|s| s.spec().name.as_ref() == Some(&r.package_record.name));

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
            table.add_row(vec![
                record.package_record.name.as_normalized().to_string(),
                dep_info
                    .expect("partition contains only values with Some")
                    .render(long),
                record.package_record.version.to_string(),
                record.package_record.build.to_string(),
                short_channel(&record.channel),
                record
                    .package_record
                    .size
                    .map(|s| HumanBytes(s).to_string())
                    .unwrap_or_default(),
            ]);
        }
        for (record, _) in &transient {
            table.add_row(vec![
                record.package_record.name.as_normalized().to_string(),
                "".to_string(),
                record.package_record.version.to_string(),
                record.package_record.build.to_string(),
                short_channel(&record.channel),
                record
                    .package_record
                    .size
                    .map(|s| HumanBytes(s).to_string())
                    .unwrap_or_default(),
            ]);
        }
        table
    }
}

impl FinalizedRunDependencies {
    pub fn to_table(&self, table: comfy_table::Table, long: bool) -> comfy_table::Table {
        let mut table = table;
        table
            .set_content_arrangement(comfy_table::ContentArrangement::Dynamic)
            .set_header(vec!["Name", "Spec"]);

        if !self.depends.is_empty() {
            let mut row = comfy_table::Row::new();
            row.add_cell(
                comfy_table::Cell::new("Depends").add_attribute(comfy_table::Attribute::Bold),
            );
            table.add_row(row);

            self.depends.iter().for_each(|d| {
                let rendered = d.render(long);
                table.add_row(rendered.splitn(2, ' ').collect::<Vec<&str>>());
            });
        }

        if !self.constrains.is_empty() {
            let mut row = comfy_table::Row::new();
            row.add_cell(
                comfy_table::Cell::new("Constrains").add_attribute(comfy_table::Attribute::Bold),
            );
            table.add_row(row);

            self.constrains.iter().for_each(|d| {
                let rendered = d.render(long);
                table.add_row(rendered.splitn(2, ' ').collect::<Vec<&str>>());
            });
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

    #[error("Could not collect run exports: {0}")]
    CouldNotCollectRunExports(std::io::Error),

    #[error("Could not parse version spec: {0}")]
    VersionSpecParseError(#[from] ParseVersionSpecError),

    #[error("Could not parse version: {0}")]
    VersionParseError(#[from] rattler_conda_types::ParseVersionError),

    #[error("Could not parse match spec: {0}")]
    MatchSpecParseError(#[from] rattler_conda_types::ParseMatchSpecError),

    #[error("Could not parse build string matcher: {0}")]
    StringMatcherParseError(#[from] rattler_conda_types::StringMatcherParseError),

    #[error("Could not apply pin: {0}")]
    PinApplyError(#[from] PinError),

    #[error("Could not apply pin. The following subpackage is not available: {0:?}")]
    SubpackageNotFound(PackageName),

    #[error("Compiler configuration error: {0}")]
    CompilerError(String),

    #[error("Could not reindex channels: {0}")]
    RefreshChannelError(std::io::Error),
}

/// Apply a variant to a dependency list and resolve all pin_subpackage and compiler
/// dependencies
pub fn apply_variant(
    raw_specs: &[Dependency],
    build_configuration: &BuildConfiguration,
    compatibility_specs: &HashMap<PackageName, PackageRecord>,
) -> Result<Vec<DependencyInfo>, ResolveError> {
    let variant = &build_configuration.variant;
    let subpackages = &build_configuration.subpackages;
    let target_platform = &build_configuration.target_platform;

    raw_specs
        .iter()
        .map(|s| {
            match s {
                Dependency::Spec(m) => {
                    let m = m.clone();
                    if m.version.is_none() && m.build.is_none() {
                        if let Some(name) = &m.name {
                            if let Some(version) = variant.get(name.as_normalized()) {
                                // if the variant starts with an alphanumeric character,
                                // we have to add a '=' to the version spec
                                let mut spec = version.clone();

                                // check if all characters are alphanumeric or ., in that case add
                                // a '=' to get "startswith" behavior
                                if spec.chars().all(|c| c.is_alphanumeric() || c == '.') {
                                    spec = format!("={}", version);
                                } else {
                                    spec = version.clone();
                                }

                                // we split at whitespace to separate into version and build
                                let mut splitter = spec.split_whitespace();
                                let version_spec = splitter.next().map(|v| VersionSpec::from_str(v, ParseStrictness::Strict)).transpose()?;
                                let build_spec = splitter.next().map(StringMatcher::from_str).transpose()?;
                                let final_spec = MatchSpec {
                                    version: version_spec,
                                    build: build_spec,
                                    ..m
                                };
                                return Ok(VariantDependency {
                                    spec: final_spec,
                                    variant: version.clone(),
                                }.into());
                            }
                        }
                    }
                    Ok(SourceDependency { spec: m }.into())
                }
                Dependency::PinSubpackage(pin) => {
                    let name = &pin.pin_value().name;
                    let subpackage = subpackages.get(name).ok_or(ResolveError::SubpackageNotFound(name.to_owned()))?;
                    let pinned = pin
                        .pin_value()
                        .apply(
                            &Version::from_str(&subpackage.version)?,
                            &subpackage.build_string,
                        )?;
                    Ok(PinSubpackageDependency { spec: pinned }.into())
                }
                Dependency::PinCompatible(pin) => {
                    let name = &pin.pin_value().name;
                    let pin_package = compatibility_specs.get(name)
                        .ok_or(ResolveError::SubpackageNotFound(name.to_owned()))?;

                    let pinned = pin
                        .pin_value()
                        .apply(
                            &pin_package.version,
                            &pin_package.build,
                        )?;
                    Ok(PinCompatibleDependency { spec: pinned }.into())
                }
                Dependency::Compiler(compiler) => {
                    if target_platform == &Platform::NoArch {
                        return Err(ResolveError::CompilerError("Noarch packages cannot have compilers".to_string()))
                    }

                    let compiler_variant = format!("{}_compiler", compiler.language());
                    let compiler_name = variant
                        .get(&compiler_variant)
                        .map(|s| s.to_string())
                        .unwrap_or_else(|| {
                            if target_platform.is_linux() {
                                let default_compiler = match compiler.language() {
                                    "c" => "gcc".to_string(),
                                    "cxx" => "gxx".to_string(),
                                    "fortran" => "gfortran".to_string(),
                                    "rust" => "rust".to_string(),
                                    _ => "".to_string()
                                };
                                default_compiler
                            } else if target_platform.is_osx() {
                                let default_compiler = match compiler.language() {
                                    "c" => "clang".to_string(),
                                    "cxx" => "clangxx".to_string(),
                                    "fortran" => "gfortran".to_string(),
                                    "rust" => "rust".to_string(),
                                    _ => "".to_string()
                                };
                                default_compiler
                            } else if target_platform.is_windows() {
                                let default_compiler = match compiler.language() {
                                    // note with conda-build, these are dependent on the python version
                                    // we could also check the variant for the python version here!
                                    "c" => "vs2017".to_string(),
                                    "cxx" => "vs2017".to_string(),
                                    "fortran" => "gfortran".to_string(),
                                    "rust" => "rust".to_string(),
                                    _ => "".to_string()
                                };
                                default_compiler
                            } else {
                                "".to_string()
                            }
                        });

                    if compiler_name.is_empty() {
                        return Err(ResolveError::CompilerError(
                            format!("Could not find compiler for {}. Configure {}_compiler in your variant config file for {target_platform}.", compiler.language(), compiler.language())));
                    }

                    let compiler_version_variant = format!("{}_version", compiler_variant);
                    let compiler_version = variant.get(&compiler_version_variant);

                    let final_compiler = if let Some(compiler_version) = compiler_version {
                        format!(
                            "{}_{} ={}",
                            compiler_name, target_platform, compiler_version
                        )
                    } else {
                        format!("{}_{}", compiler_name, target_platform)
                    };

                    Ok(CompilerDependency {
                        language: compiler_name,
                        spec: MatchSpec::from_str(&final_compiler, ParseStrictness::Strict)?,
                    }.into())
                }
            }
        })
        .collect()
}

fn collect_run_exports_from_env(
    env: &[RepoDataRecord],
    cache_dir: &Path,
    filter: impl Fn(&RepoDataRecord) -> bool,
) -> Result<HashMap<PackageName, RunExportsJson>, std::io::Error> {
    let mut run_exports = HashMap::new();
    for pkg in env {
        if !filter(pkg) {
            continue;
        }

        let cache_key: CacheKey = Into::into(&pkg.package_record);
        let pkc = cache_dir.join(cache_key.to_string());
        let rex = RunExportsJson::from_package_directory(pkc).ok();
        if let Some(rex) = rex {
            run_exports.insert(pkg.package_record.name.clone(), rex);
        }
    }
    Ok(run_exports)
}

pub async fn install_environments(
    output: &Output,
    tool_configuration: &tool_configuration::Configuration,
) -> Result<(), ResolveError> {
    let cache_dir = rattler::default_cache_dir().expect("Could not get default cache dir");

    let dependencies = output
        .finalized_dependencies
        .as_ref()
        .ok_or(ResolveError::FinalizedDependencyNotFound)?;

    if let Some(build_deps) = dependencies.build.as_ref() {
        install_packages(
            &build_deps.resolved,
            &output.build_configuration.build_platform,
            &output.build_configuration.directories.build_prefix,
            &cache_dir,
            tool_configuration,
        )
        .await?;
    }

    if let Some(host_deps) = dependencies.host.as_ref() {
        install_packages(
            &host_deps.resolved,
            &output.build_configuration.host_platform,
            &output.build_configuration.directories.host_prefix,
            &cache_dir,
            tool_configuration,
        )
        .await?;
    }

    Ok(())
}

/// This function resolves the dependencies of a recipe.
/// To do this, we have to run a couple of steps:
///
/// 1. Apply the variants to the dependencies, and compiler & pin_subpackage specs
/// 2. Extend the dependencies with the run exports of the dependencies "above"
/// 3. Resolve the dependencies
/// 4. Download the packages
/// 5. Extract the run exports from the downloaded packages (for the next environment)
#[allow(clippy::for_kv_map)]
async fn resolve_dependencies(
    output: &Output,
    channels: &[String],
    tool_configuration: &tool_configuration::Configuration,
) -> Result<FinalizedDependencies, ResolveError> {
    let merge_build_host = output.recipe.build().merge_build_and_host_envs();

    let cache_dir = rattler::default_cache_dir().expect("Could not get default cache dir");
    let pkgs_dir = cache_dir.join("pkgs");

    let reqs = &output.recipe.requirements();
    let mut compatibility_specs = HashMap::new();

    let build_env = if !reqs.build.is_empty() && !merge_build_host {
        let specs = apply_variant(
            reqs.build(),
            &output.build_configuration,
            &compatibility_specs,
        )?;

        let match_specs = specs.iter().map(|s| s.spec().clone()).collect::<Vec<_>>();

        let env = create_environment(
            &match_specs,
            &output.build_configuration.build_platform,
            &output.build_configuration.directories.build_prefix,
            channels,
            tool_configuration,
        )
        .await
        .map_err(ResolveError::from)?;

        let run_exports = collect_run_exports_from_env(&env, &pkgs_dir, |rec| {
            let res = match_specs
                .iter()
                .any(|m| Some(&rec.package_record.name) == m.name.as_ref());

            let ignore_run_exports_from = output
                .recipe
                .requirements()
                .ignore_run_exports()
                .from_package();

            res && !ignore_run_exports_from.contains(&rec.package_record.name)
        })
        .map_err(ResolveError::CouldNotCollectRunExports)?;

        env.iter().for_each(|r| {
            compatibility_specs.insert(r.package_record.name.clone(), r.package_record.clone());
        });

        Some(ResolvedDependencies {
            specs,
            resolved: env,
            run_exports,
        })
    } else {
        fs::create_dir_all(&output.build_configuration.directories.build_prefix)
            .expect("Could not create build prefix");
        None
    };

    // host env
    let mut specs = apply_variant(
        reqs.host(),
        &output.build_configuration,
        &compatibility_specs,
    )?;

    let clone_specs = |name: &PackageName,
                       env: &str,
                       specs: &[String]|
     -> Result<Vec<DependencyInfo>, ResolveError> {
        let mut cloned = Vec::new();
        for spec in specs {
            let spec = MatchSpec::from_str(spec, ParseStrictness::Strict)?;
            let in_ignore_run_exports = |pkg| {
                output
                    .recipe
                    .requirements()
                    .ignore_run_exports()
                    .by_name()
                    .contains(pkg)
            };
            if spec
                .name
                .as_ref()
                .map(in_ignore_run_exports)
                .unwrap_or_default()
            {
                continue;
            }

            let dep = RunExportDependency {
                spec,
                from: env.to_string(),
                source_package: name.as_normalized().to_string(),
            };
            cloned.push(dep.into());
        }
        Ok(cloned)
    };

    // add the run exports of the build environment
    if let Some(build_env) = &build_env {
        for (name, rex) in &build_env.run_exports {
            specs.extend(clone_specs(name, "build", &rex.strong)?);
        }
    }

    let mut match_specs = specs.iter().map(|s| s.spec().clone()).collect::<Vec<_>>();
    if merge_build_host {
        // add the reqs of build to host
        let specs = apply_variant(
            reqs.build(),
            &output.build_configuration,
            &compatibility_specs,
        )?;
        match_specs.extend(specs.iter().map(|s| s.spec().clone()));
    }

    let host_env = if !match_specs.is_empty() {
        let env = create_environment(
            &match_specs,
            &output.build_configuration.host_platform,
            &output.build_configuration.directories.host_prefix,
            channels,
            tool_configuration,
        )
        .await
        .map_err(ResolveError::from)?;

        let run_exports = collect_run_exports_from_env(&env, &pkgs_dir, |rec| {
            let res = match_specs
                .iter()
                .any(|m| Some(&rec.package_record.name) == m.name.as_ref());

            let ignore_run_exports_from = output
                .recipe
                .requirements()
                .ignore_run_exports()
                .from_package();

            res && !ignore_run_exports_from.contains(&rec.package_record.name)
        })
        .map_err(ResolveError::CouldNotCollectRunExports)?;

        env.iter().for_each(|r| {
            compatibility_specs.insert(r.package_record.name.clone(), r.package_record.clone());
        });

        Some(ResolvedDependencies {
            specs,
            resolved: env,
            run_exports,
        })
    } else {
        fs::create_dir_all(&output.build_configuration.directories.host_prefix)
            .expect("Could not create host prefix");
        None
    };

    let depends = apply_variant(&reqs.run, &output.build_configuration, &compatibility_specs)?;

    let constrains = apply_variant(
        &reqs.run_constraints,
        &output.build_configuration,
        &compatibility_specs,
    )?;

    let render_run_exports = |run_export: &[Dependency]| -> Result<Vec<String>, ResolveError> {
        let rendered = apply_variant(
            run_export,
            &output.build_configuration,
            &compatibility_specs,
        )?;
        Ok(rendered
            .iter()
            .map(|dep| dep.spec().to_string())
            .collect::<Vec<_>>())
    };

    let run_exports = output.recipe.requirements().run_exports();

    let run_exports = if !run_exports.is_empty() {
        Some(RunExportsJson {
            strong: render_run_exports(run_exports.strong())?,
            weak: render_run_exports(run_exports.weak())?,
            noarch: render_run_exports(run_exports.noarch())?,
            strong_constrains: render_run_exports(run_exports.strong_constraints())?,
            weak_constrains: render_run_exports(run_exports.weak_constraints())?,
        })
    } else {
        None
    };

    let mut run_specs = FinalizedRunDependencies {
        depends,
        constrains,
        run_exports,
    };

    // Propagate run exports from host env to run env
    if let Some(host_env) = &host_env {
        match output.build_configuration.target_platform {
            Platform::NoArch => {
                for (name, rex) in &host_env.run_exports {
                    run_specs
                        .depends
                        .extend(clone_specs(name, "host", &rex.noarch)?);
                }
            }
            _ => {
                for (name, rex) in &host_env.run_exports {
                    run_specs
                        .depends
                        .extend(clone_specs(name, "host", &rex.strong)?);
                    run_specs
                        .depends
                        .extend(clone_specs(name, "host", &rex.weak)?);
                    run_specs
                        .constrains
                        .extend(clone_specs(name, "host", &rex.strong_constrains)?);
                    run_specs
                        .constrains
                        .extend(clone_specs(name, "host", &rex.weak_constrains)?);
                }
            }
        }
    }

    // We also have to propagate the _strong_ run exports of the build environment to the run environment
    if let Some(build_env) = &build_env {
        match output.build_configuration.target_platform {
            Platform::NoArch => {}
            _ => {
                for (name, rex) in &build_env.run_exports {
                    run_specs
                        .depends
                        .extend(clone_specs(name, "build", &rex.strong)?);
                    run_specs.constrains.extend(clone_specs(
                        name,
                        "build",
                        &rex.strong_constrains,
                    )?);
                }
            }
        }
    }

    // log a table of the rendered run dependencies
    if run_specs.depends.is_empty() && run_specs.constrains.is_empty() {
        tracing::info!("\nFinalized run dependencies: this output has no run dependencies");
    } else {
        tracing::info!("\nFinalized run dependencies:\n{}", run_specs);
    }

    Ok(FinalizedDependencies {
        // build_env is empty now!
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
    ) -> Result<Output, ResolveError> {
        let span = tracing::info_span!("Resolving environments");
        let _enter = span.enter();

        let output = if self.finalized_dependencies.is_some() {
            tracing::info!("Using finalized dependencies");

            // The output already has the finalized dependencies, so we can just use it as-is
            install_environments(&self, tool_configuration).await?;
            self.clone()
        } else {
            let channels = self
                .reindex_channels()
                .map_err(ResolveError::RefreshChannelError)?;
            let finalized_dependencies =
                resolve_dependencies(&self, &channels, tool_configuration).await?;

            // The output with the resolved dependencies
            Output {
                finalized_dependencies: Some(finalized_dependencies),
                ..self.clone()
            }
        };
        Ok(output)
    }
}

#[cfg(test)]
mod tests {
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
            CompilerDependency {
                language: "c".to_string(),
                spec: MatchSpec::from_str("foo", ParseStrictness::Strict).unwrap(),
            }
            .into(),
            PinSubpackageDependency {
                spec: MatchSpec::from_str("baz", ParseStrictness::Strict).unwrap(),
            }
            .into(),
            PinCompatibleDependency {
                spec: MatchSpec::from_str("bat", ParseStrictness::Strict).unwrap(),
            }
            .into(),
        ];
        let yaml_str = serde_yaml::to_string(&dep_info).unwrap();
        insta::assert_snapshot!(yaml_str);

        // test deserialize
        let dep_info: Vec<DependencyInfo> = serde_yaml::from_str(&yaml_str).unwrap();
        assert_eq!(dep_info.len(), 5);
        assert!(matches!(dep_info[0], DependencyInfo::Source(_)));
        assert!(matches!(dep_info[1], DependencyInfo::Variant(_)));
        assert!(matches!(dep_info[2], DependencyInfo::Compiler(_)));
        assert!(matches!(dep_info[3], DependencyInfo::PinSubpackage(_)));
        assert!(matches!(dep_info[4], DependencyInfo::PinCompatible(_)));
    }
}
