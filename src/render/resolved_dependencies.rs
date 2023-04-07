use std::{
    collections::{BTreeMap, HashMap},
    fs,
    path::{Path, PathBuf},
    str::FromStr,
};

use crate::metadata::{Output, PlatformOrNoarch, RunExports};
use rattler::package_cache::CacheKey;
use rattler_conda_types::{
    package::{PackageFile, RunExportsJson},
    MatchSpec, RepoDataRecord, VersionSpec,
};
use thiserror::Error;

use super::{
    dependency_list::{Dependency, DependencyList},
    solver::create_environment,
};

pub enum DependencyInfo {
    FromVariant { spec: MatchSpec, variant: String },
    Raw { spec: MatchSpec },
}

#[derive(Debug, Clone)]
pub struct FinalizedRunDependencies {
    pub depends: Vec<MatchSpec>,
    pub constrains: Vec<MatchSpec>,
}

pub struct ResolvedDependency {
    // source: Some(DependencyInfo),
    resolved: RepoDataRecord,
    cache_fn: PathBuf,
}

#[derive(Debug, Clone)]
pub struct ResolvedDependencies {
    raw: DependencyList,
    // resolved: Vec<ResolvedDependency>,
    resolved: Vec<RepoDataRecord>,
    run_exports: HashMap<String, RunExports>,
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



pub fn apply_variant(
    raw_specs: &DependencyList,
    variant: &BTreeMap<String, String>,
    target_platform: &PlatformOrNoarch,
) -> Result<Vec<(Dependency, MatchSpec)>, ResolveError> {
    let applied = raw_specs
        .iter()
        .map(|s| {
            match s {
                Dependency::Spec(m) => {
                    let m = m.clone();
                    if m.version.is_none() && m.build.is_none() {
                        if let Some(name) = &m.name {
                            if let Some(version) = variant.get(name) {
                                return (
                                    s.clone(),
                                    MatchSpec {
                                        version: Some(
                                            VersionSpec::from_str(version)
                                                .expect("Invalid version spec"),
                                        ),
                                        ..m
                                    },
                                );
                            }
                        }
                    }
                    (s.clone(), m)
                }
                Dependency::PinSubpackage(_) => todo!(),
                Dependency::Compiler(compiler) => {
                    let target_platform =
                        if let PlatformOrNoarch::Platform(target_platform) = target_platform {
                            target_platform
                        } else {
                            panic!("Noarch packages cannot have compilers");
                        };

                    let compiler_variant = format!("compiler_{}", compiler.compiler);
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
                        format!("{}_{} {}", compiler_name, target_platform, compiler_version)
                    } else {
                        format!("{}_{}", compiler_name, target_platform)
                    };

                    (
                        s.clone(),
                        MatchSpec::from_str(&final_compiler).expect("Invalid compiler spec"),
                    )
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
) -> Result<HashMap<String, RunExports>, std::io::Error> {
    let mut run_exports = HashMap::new();
    for pkg in env {
        if !filter(pkg) {
            continue;
        }

        let cache_key: CacheKey = Into::into(&pkg.package_record);
        let pkc = cache_dir.join(cache_key.to_string());
        let rex = RunExportsJson::from_package_directory(pkc).ok();
        if let Some(rex) = rex {
            let rex = RunExports {
                strong: rex.strong,
                weak: rex.weak,
                strong_constrains: rex.strong_constrains,
                weak_constrains: rex.weak_constrains,
                noarch: rex.noarch,
            };
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
/// 5. Extract the run exports from the downloaded packages (for the next environent)
#[allow(clippy::for_kv_map)]
pub async fn resolve_dependencies(output: &Output) -> Result<FinalizedDependencies, ResolveError> {
    let cache_dir = rattler::default_cache_dir().expect("Could not get default cache dir");
    let pkgs_dir = cache_dir.join("pkgs");

    let reqs = &output.recipe.requirements;

    let build_env = if !reqs.build.is_empty() {
        let specs = apply_variant(
            &reqs.build,
            &output.build_configuration.variant,
            &output.build_configuration.target_platform,
        )?;

        let match_specs = specs.iter().map(|(_, s)| s).cloned().collect::<Vec<_>>();

        let env = create_environment(
            match_specs.clone(),
            &output.build_configuration.build_platform,
            &output.build_configuration.directories.build_prefix,
            &output.build_configuration.channels,
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
            raw: reqs.build.clone(),
            resolved: env,
            run_exports,
        })
    } else {
        fs::create_dir_all(&output.build_configuration.directories.build_prefix)
            .expect("Could not create build prefix");
        None
    };

    // host env
    let specs = apply_variant(
        &reqs.host,
        &output.build_configuration.variant,
        &output.build_configuration.target_platform,
    )?;

    let mut match_specs = specs.iter().map(|(_, s)| s).cloned().collect::<Vec<_>>();

    tracing::info!("Resolving host specs: {:?}", match_specs);

    // add the run exports of the build environment
    if let Some(build_env) = &build_env {
        for (_, rex) in &build_env.run_exports {
            for spec in &rex.strong {
                match_specs.push(MatchSpec::from_str(spec).expect("Invalid match spec"));
            }
            for spec in &rex.weak {
                match_specs.push(MatchSpec::from_str(spec).expect("Invalid match spec"));
            }
        }
    }

    let host_env = if !match_specs.is_empty() {
        let env = create_environment(
            match_specs.clone(),
            &output.build_configuration.host_platform,
            &output.build_configuration.directories.host_prefix,
            &output.build_configuration.channels,
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
            raw: reqs.host.clone(),
            resolved: env,
            run_exports,
        })
    } else {
        fs::create_dir_all(&output.build_configuration.directories.host_prefix)
            .expect("Could not create host prefix");
        None
    };

    let run_depends = apply_variant(
        &reqs.run,
        &output.build_configuration.variant,
        &output.build_configuration.target_platform,
    )?;

    let run_constrains = apply_variant(
        &reqs.constrains,
        &output.build_configuration.variant,
        &output.build_configuration.target_platform,
    )?;

    let mut run_specs = FinalizedRunDependencies {
        depends: run_depends.iter().map(|(_, s)| s).cloned().collect(),
        constrains: run_constrains.iter().map(|(_, s)| s).cloned().collect(),
    };

    if let Some(host_env) = &host_env {
        match output.build_configuration.target_platform {
            PlatformOrNoarch::Platform(_) => {
                for (_, rex) in &host_env.run_exports {
                    for spec in &rex.strong {
                        run_specs
                            .depends
                            .push(MatchSpec::from_str(spec).expect("Invalid match spec"));
                    }
                    for spec in &rex.weak {
                        run_specs
                            .depends
                            .push(MatchSpec::from_str(spec).expect("Invalid match spec"));
                    }
                    for spec in &rex.strong_constrains {
                        run_specs
                            .constrains
                            .push(MatchSpec::from_str(spec).expect("Invalid match spec"));
                    }
                    for spec in &rex.weak_constrains {
                        run_specs
                            .constrains
                            .push(MatchSpec::from_str(spec).expect("Invalid match spec"));
                    }
                }
            }
            PlatformOrNoarch::Noarch(_) => {
                for (_, rex) in &host_env.run_exports {
                    for spec in &rex.noarch {
                        run_specs
                            .depends
                            .push(MatchSpec::from_str(spec).expect("Invalid match spec"));
                    }
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
