use std::{
    collections::HashMap,
    fmt::{Display, Formatter},
    fs,
    path::Path,
    str::FromStr,
};

use crate::metadata::{BuildConfiguration, Output};
use indicatif::HumanBytes;
use rattler::package_cache::CacheKey;
use rattler_conda_types::{
    package::{PackageFile, RunExportsJson},
    MatchSpec, Platform, RepoDataRecord, Version, VersionSpec,
};
use thiserror::Error;

use super::{
    dependency_list::{Dependency, DependencyList},
    solver::create_environment,
};

/// A enum to keep track of where a given Dependency comes from
#[allow(dead_code)]
#[derive(Debug, Clone)]
pub enum DependencyInfo {
    /// The dependency is a direct dependency of the package, with a variant applied
    /// from the variant config
    Variant { spec: MatchSpec, variant: String },
    /// This is a special compiler dependency (e.g. `{{ compiler('c') }}`
    Compiler { spec: MatchSpec },
    /// This is a special pin dependency (e.g. `{{ pin_subpackage('foo', exact=True) }}`
    PinSubpackage { spec: MatchSpec },
    /// This is a special run_exports dependency (e.g. `{{ pin_compatible('foo') }}`
    PinCompatible { spec: MatchSpec },
    /// This is a special run_exports dependency from another package
    RunExports {
        spec: MatchSpec,
        from: String,
        source_package: String,
    },
    /// This is a regular dependency of the package without any modifications
    Raw { spec: MatchSpec },
    /// This is a transient dependency of the package, which is not a direct dependency
    /// of the package, but is a dependency of a dependency
    Transient,
}

impl DependencyInfo {
    /// Get the matchspec from a dependency info
    pub fn spec(&self) -> &MatchSpec {
        match self {
            DependencyInfo::Variant { spec, .. } => spec,
            DependencyInfo::Compiler { spec } => spec,
            DependencyInfo::PinSubpackage { spec } => spec,
            DependencyInfo::PinCompatible { spec } => spec,
            DependencyInfo::RunExports { spec, .. } => spec,
            DependencyInfo::Raw { spec } => spec,
            DependencyInfo::Transient => panic!("Cannot get spec from transient dependency"),
        }
    }

    pub fn render(&self) -> String {
        match self {
            DependencyInfo::Variant { spec, .. } => format!("{} (V)", spec),
            DependencyInfo::Compiler { spec } => format!("{} (C)", spec),
            DependencyInfo::PinSubpackage { spec } => format!("{} (PS)", spec),
            DependencyInfo::PinCompatible { spec } => format!("{} (PC)", spec),
            DependencyInfo::RunExports {
                spec,
                from,
                source_package,
            } => format!("{} (RE of [{}: {}])", spec, from, source_package),
            DependencyInfo::Raw { spec } => spec.to_string(),
            DependencyInfo::Transient => "transient".to_string(),
        }
    }
}

#[derive(Debug, Clone)]
pub struct FinalizedRunDependencies {
    pub depends: Vec<DependencyInfo>,
    pub constrains: Vec<DependencyInfo>,
    pub run_exports: Option<RunExportsJson>,
}

#[allow(dead_code)]
#[derive(Debug, Clone)]
pub struct ResolvedDependencies {
    specs: Vec<DependencyInfo>,
    resolved: Vec<RepoDataRecord>,
    run_exports: HashMap<String, RunExportsJson>,
}

fn short_channel(channel: &str) -> String {
    if channel.contains('/') {
        channel
            .rsplit('/')
            .find(|s| !s.is_empty())
            .unwrap()
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
                    (r, s)
                } else {
                    (r, &DependencyInfo::Transient)
                }
            })
            .collect::<Vec<_>>();

        let (mut explicit, mut transient): (Vec<_>, Vec<_>) = resolved_w_specs
            .into_iter()
            .partition(|(_, s)| !matches!(s, DependencyInfo::Transient));

        explicit.sort_by(|(a, _), (b, _)| a.package_record.name.cmp(&b.package_record.name));
        transient.sort_by(|(a, _), (b, _)| a.package_record.name.cmp(&b.package_record.name));

        for (record, dep_info) in &explicit {
            table.add_row(vec![
                record.package_record.name.clone(),
                dep_info.render(),
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
                record.package_record.name.clone(),
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

#[derive(Debug, Clone)]
pub struct FinalizedDependencies {
    pub build: Option<ResolvedDependencies>,
    pub host: Option<ResolvedDependencies>,
    pub run: FinalizedRunDependencies,
}

#[derive(Error, Debug)]
pub enum ResolveError {
    #[error("Failed to resolve dependencies {0}")]
    DependencyResolutionError(#[from] anyhow::Error),
}

/// Apply a variant to a dependency list and resolve all pin_subpackage and compiler
/// dependencies
pub fn apply_variant(
    raw_specs: &DependencyList,
    build_configuration: &BuildConfiguration,
) -> Result<Vec<DependencyInfo>, ResolveError> {
    let variant = &build_configuration.variant;
    let subpackages = &build_configuration.subpackages;
    let target_platform = &build_configuration.target_platform;

    let applied = raw_specs
        .iter()
        .map(|s| {
            match s {
                Dependency::Spec(m) => {
                    let m = m.clone();
                    if m.version.is_none() && m.build.is_none() {
                        if let Some(name) = &m.name {
                            if let Some(version) = variant.get(name) {
                                let final_spec = MatchSpec {
                                    version: Some(
                                        VersionSpec::from_str(&format!("={}", version))
                                            .expect("Invalid version spec"),
                                    ),
                                    ..m
                                };
                                return DependencyInfo::Variant {
                                    spec: final_spec,
                                    variant: version.clone(),
                                };
                            }
                        }
                    }
                    DependencyInfo::Raw { spec: m }
                }
                Dependency::PinSubpackage(pin) => {
                    let name = &pin.pin_subpackage.name;
                    let subpackage = subpackages.get(name).expect("Invalid subpackage");
                    let pinned = pin
                        .pin_subpackage
                        .apply(
                            &Version::from_str(&subpackage.version)
                                .expect("could not parse version"),
                            &subpackage.build_string,
                        )
                        .expect("could not apply pin");
                    DependencyInfo::PinSubpackage { spec: pinned }
                }
                Dependency::Compiler(compiler) => {
                    if target_platform == &Platform::NoArch {
                        panic!("Noarch packages cannot have compilers");
                    }

                    let compiler_variant = format!("{}_compiler", compiler.compiler);
                    let compiler_name = variant
                        .get(&compiler_variant)
                        .map(|s| s.to_string())
                        .unwrap_or_else(|| {
                            // defaults
                            if target_platform.is_linux() {
                                let default_compiler = match compiler.compiler.as_str() {
                                    "c" => "gcc".to_string(),
                                    "cxx" => "gxx".to_string(),
                                    "fortran" => "gfortran".to_string(),
                                    "rust" => "rust".to_string(),
                                    _ => {
                                        panic!(
                                            "No default value for compiler: {}",
                                            compiler.compiler
                                        )
                                    }
                                };
                                default_compiler
                            } else if target_platform.is_osx() {
                                let default_compiler = match compiler.compiler.as_str() {
                                    "c" => "clang".to_string(),
                                    "cxx" => "clangxx".to_string(),
                                    "fortran" => "gfortran".to_string(),
                                    "rust" => "rust".to_string(),
                                    _ => {
                                        panic!(
                                            "No default value for compiler: {}",
                                            compiler.compiler
                                        )
                                    }
                                };
                                default_compiler
                            } else if target_platform.is_windows() {
                                let default_compiler = match compiler.compiler.as_str() {
                                    // note with conda-build, these are dependent on the python version
                                    // we could also check the variant for the python version here!
                                    "c" => "vs2017".to_string(),
                                    "cxx" => "vs2017".to_string(),
                                    "fortran" => "gfortran".to_string(),
                                    "rust" => "rust".to_string(),
                                    _ => {
                                        panic!(
                                            "No default value for compiler: {}",
                                            compiler.compiler
                                        )
                                    }
                                };
                                default_compiler
                            } else {
                                panic!("Unknown target platform: {}", target_platform);
                            }
                        });

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

                    DependencyInfo::Compiler {
                        spec: MatchSpec::from_str(&final_compiler)
                            .expect("Could not parse compiler"),
                    }
                }
            }
        })
        .collect::<Vec<_>>();

    Ok(applied)
}

fn collect_run_exports_from_env(
    env: &[RepoDataRecord],
    cache_dir: &Path,
    filter: impl Fn(&RepoDataRecord) -> bool,
) -> Result<HashMap<String, RunExportsJson>, std::io::Error> {
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
) -> Result<FinalizedDependencies, ResolveError> {
    let cache_dir = rattler::default_cache_dir().expect("Could not get default cache dir");
    let pkgs_dir = cache_dir.join("pkgs");

    let reqs = &output.recipe.requirements;

    let build_env = if !reqs.build.is_empty() {
        let specs = apply_variant(&reqs.build, &output.build_configuration)?;

        let match_specs = specs.iter().map(|s| s.spec().clone()).collect::<Vec<_>>();

        let env = create_environment(
            &match_specs,
            &output.build_configuration.build_platform,
            &output.build_configuration.directories.build_prefix,
            channels,
        )
        .await
        .map_err(ResolveError::from)?;

        let run_exports = collect_run_exports_from_env(&env, &pkgs_dir, |rec| {
            let res = match_specs
                .iter()
                .any(|m| Some(&rec.package_record.name) == m.name.as_ref());

            if let Some(ignore_run_exports_from) = &output.recipe.build.ignore_run_exports_from {
                res && !ignore_run_exports_from.contains(&rec.package_record.name)
            } else {
                res
            }
        })
        .expect("Could not find run exports");

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
    let mut specs = apply_variant(&reqs.host, &output.build_configuration)?;

    let clone_specs =
        |name: &str, env: &str, specs: &[String]| -> Result<Vec<DependencyInfo>, ResolveError> {
            let mut cloned = Vec::new();
            for spec in specs {
                let spec = MatchSpec::from_str(spec).expect("...");
                let dep = DependencyInfo::RunExports {
                    spec,
                    from: env.to_string(),
                    source_package: name.to_string(),
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
        )
        .await
        .map_err(ResolveError::from)?;

        let run_exports = collect_run_exports_from_env(&env, &pkgs_dir, |rec| {
            match_specs
                .iter()
                .any(|m| Some(&rec.package_record.name) == m.name.as_ref())
        })
        .expect("Could not find run exports");

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

    let run_depends = apply_variant(&reqs.run, &output.build_configuration)?;

    let run_constrains = apply_variant(&reqs.run_constrained, &output.build_configuration)?;

    let render_run_exports = |run_export: &DependencyList| -> Vec<String> {
        let rendered = apply_variant(run_export, &output.build_configuration).unwrap();
        rendered
            .iter()
            .map(|dep| dep.spec().to_string())
            .collect::<Vec<_>>()
    };

    let run_exports = &output
        .recipe
        .build
        .run_exports
        .as_ref()
        .map(|run_exports| RunExportsJson {
            strong: render_run_exports(&run_exports.strong),
            weak: render_run_exports(&run_exports.weak),
            noarch: render_run_exports(&run_exports.noarch),
            strong_constrains: render_run_exports(&run_exports.strong_constrains),
            weak_constrains: render_run_exports(&run_exports.weak_constrains),
        });

    let mut run_specs = FinalizedRunDependencies {
        depends: run_depends,
        constrains: run_constrains,
        run_exports: run_exports.clone(),
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
