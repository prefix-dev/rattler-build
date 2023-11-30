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
    MatchSpec, PackageName, Platform, RepoDataRecord, StringMatcher, Version, VersionSpec,
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
#[serde(tag = "source", rename_all = "snake_case")]
pub enum DependencyInfo {
    /// The dependency is a direct dependency of the package, with a variant applied
    /// from the variant config
    Variant {
        #[serde_as(as = "DisplayFromStr")]
        spec: MatchSpec,
        variant: String,
    },
    /// This is a special compiler dependency (e.g. `{{ compiler('c') }}`
    Compiler {
        #[serde_as(as = "DisplayFromStr")]
        spec: MatchSpec,
    },
    /// This is a special pin dependency (e.g. `{{ pin_subpackage('foo', exact=True) }}`
    PinSubpackage {
        #[serde_as(as = "DisplayFromStr")]
        spec: MatchSpec,
    },
    /// This is a special run_exports dependency (e.g. `{{ pin_compatible('foo') }}`
    PinCompatible {
        #[serde_as(as = "DisplayFromStr")]
        spec: MatchSpec,
    },
    /// This is a special run_exports dependency from another package
    RunExport {
        #[serde_as(as = "DisplayFromStr")]
        spec: MatchSpec,
        from: String,
        source_package: String,
    },
    /// This is a regular dependency of the package without any modifications
    Raw {
        #[serde_as(as = "DisplayFromStr")]
        spec: MatchSpec,
    },
}

impl DependencyInfo {
    /// Get the matchspec from a dependency info
    pub fn spec(&self) -> &MatchSpec {
        match self {
            DependencyInfo::Variant { spec, .. } => spec,
            DependencyInfo::Compiler { spec } => spec,
            DependencyInfo::PinSubpackage { spec } => spec,
            DependencyInfo::PinCompatible { spec } => spec,
            DependencyInfo::RunExport { spec, .. } => spec,
            DependencyInfo::Raw { spec } => spec,
        }
    }

    pub fn render(&self) -> String {
        match self {
            DependencyInfo::Variant { spec, .. } => format!("{} (V)", spec),
            DependencyInfo::Compiler { spec } => format!("{} (C)", spec),
            DependencyInfo::PinSubpackage { spec } => format!("{} (PS)", spec),
            DependencyInfo::PinCompatible { spec } => format!("{} (PC)", spec),
            DependencyInfo::RunExport {
                spec,
                from,
                source_package,
            } => format!("{} (RE of [{}: {}])", spec, from, source_package),
            DependencyInfo::Raw { spec } => spec.to_string(),
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

impl Display for ResolvedDependencies {
    fn fmt(&self, f: &mut Formatter<'_>) -> std::fmt::Result {
        let mut table = comfy_table::Table::new();
        table
            .load_preset(comfy_table::presets::UTF8_FULL_CONDENSED)
            .apply_modifier(comfy_table::modifiers::UTF8_ROUND_CORNERS)
            .set_header(vec![
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
                    .render(),
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

        write!(f, "{}", table)?;

        Ok(())
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
                                let version_spec = splitter.next().map(VersionSpec::from_str).transpose()?;
                                let build_spec = splitter.next().map(StringMatcher::from_str).transpose()?;
                                let final_spec = MatchSpec {
                                    version: version_spec,
                                    build: build_spec,
                                    ..m
                                };
                                return Ok(DependencyInfo::Variant {
                                    spec: final_spec,
                                    variant: version.clone(),
                                });
                            }
                        }
                    }
                    Ok(DependencyInfo::Raw { spec: m })
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
                    Ok(DependencyInfo::PinSubpackage { spec: pinned })
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
                    Ok(DependencyInfo::PinCompatible { spec: pinned })
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

                    Ok(DependencyInfo::Compiler {
                        spec: MatchSpec::from_str(&final_compiler)?,
                    })
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
    tool_configuration: tool_configuration::Configuration,
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
            &tool_configuration,
        )
        .await?;
    }

    if let Some(host_deps) = dependencies.host.as_ref() {
        install_packages(
            &host_deps.resolved,
            &output.build_configuration.host_platform,
            &output.build_configuration.directories.host_prefix,
            &cache_dir,
            &tool_configuration,
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
pub async fn resolve_dependencies(
    output: &Output,
    channels: &[String],
    tool_configuration: tool_configuration::Configuration,
) -> Result<FinalizedDependencies, ResolveError> {
    let cache_dir = rattler::default_cache_dir().expect("Could not get default cache dir");
    let pkgs_dir = cache_dir.join("pkgs");

    let reqs = &output.recipe.requirements();
    let mut compatibility_specs = HashMap::new();

    let build_env = if !reqs.build.is_empty() {
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
            &tool_configuration,
        )
        .await
        .map_err(ResolveError::from)?;

        let run_exports = collect_run_exports_from_env(&env, &pkgs_dir, |rec| {
            let res = match_specs
                .iter()
                .any(|m| Some(&rec.package_record.name) == m.name.as_ref());

            let ignore_run_exports_from = output.recipe.build().ignore_run_exports_from();
            if !ignore_run_exports_from.is_empty() {
                res && !ignore_run_exports_from.contains(&rec.package_record.name)
            } else {
                res
            }
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
            let spec = MatchSpec::from_str(spec)?;
            let dep = DependencyInfo::RunExport {
                spec,
                from: env.to_string(),
                source_package: name.as_normalized().to_string(),
            };
            cloned.push(dep);
        }
        Ok(cloned)
    };

    // add the run exports of the build environment
    if let Some(build_env) = &build_env {
        for (name, rex) in &build_env.run_exports {
            specs.extend(clone_specs(name, "build", &rex.strong)?);
        }
    }

    let match_specs = specs.iter().map(|s| s.spec().clone()).collect::<Vec<_>>();

    let host_env = if !match_specs.is_empty() {
        let env = create_environment(
            &match_specs,
            &output.build_configuration.host_platform,
            &output.build_configuration.directories.host_prefix,
            channels,
            &tool_configuration,
        )
        .await
        .map_err(ResolveError::from)?;

        let run_exports = collect_run_exports_from_env(&env, &pkgs_dir, |rec| {
            match_specs
                .iter()
                .any(|m| Some(&rec.package_record.name) == m.name.as_ref())
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
        &reqs.run_constrained,
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

    let run_exports = output.recipe.build().run_exports();

    let run_exports = if !run_exports.is_empty() {
        Some(RunExportsJson {
            strong: render_run_exports(run_exports.strong())?,
            weak: render_run_exports(run_exports.weak())?,
            noarch: render_run_exports(run_exports.noarch())?,
            strong_constrains: render_run_exports(run_exports.strong_constrains())?,
            weak_constrains: render_run_exports(run_exports.weak_constrains())?,
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

    Ok(FinalizedDependencies {
        build: build_env,
        host: host_env,
        run: run_specs,
    })
}

#[cfg(test)]
mod tests {
    // test rendering of DependencyInfo
    use super::*;

    #[test]
    fn test_dependency_info_render() {
        let dep_info = vec![
            DependencyInfo::Raw {
                spec: MatchSpec::from_str("xyz").unwrap(),
            },
            DependencyInfo::Variant {
                spec: MatchSpec::from_str("foo").unwrap(),
                variant: "bar".to_string(),
            },
            DependencyInfo::Compiler {
                spec: MatchSpec::from_str("foo").unwrap(),
            },
            DependencyInfo::PinSubpackage {
                spec: MatchSpec::from_str("baz").unwrap(),
            },
            DependencyInfo::PinCompatible {
                spec: MatchSpec::from_str("bat").unwrap(),
            },
        ];
        let yaml_str = serde_yaml::to_string(&dep_info).unwrap();
        insta::assert_snapshot!(yaml_str);

        // test deserialize
        let dep_info: Vec<DependencyInfo> = serde_yaml::from_str(&yaml_str).unwrap();
        assert_eq!(dep_info.len(), 5);
        assert!(matches!(dep_info[0], DependencyInfo::Raw { .. }));
        assert!(matches!(dep_info[1], DependencyInfo::Variant { .. }));
        assert!(matches!(dep_info[2], DependencyInfo::Compiler { .. }));
        assert!(matches!(dep_info[3], DependencyInfo::PinSubpackage { .. }));
        assert!(matches!(dep_info[4], DependencyInfo::PinCompatible { .. }));
    }
}
