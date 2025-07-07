use std::{
    collections::{HashMap, HashSet},
    fmt,
    path::{Path, PathBuf},
};

use crate::windows::link::WIN_ALLOWLIST;
use crate::{
    metadata::Output,
    post_process::relink::RelinkError,
    post_process::{package_nature::PackageNature, relink},
};

use crate::render::resolved_dependencies::{FinalizedDependencies, RunExportDependency};
use globset::{Glob, GlobSet, GlobSetBuilder};
use rattler_conda_types::{PackageName, PrefixRecord};

#[derive(thiserror::Error, Debug)]
pub enum LinkingCheckError {
    #[error("Error reading file: {0}")]
    Io(#[from] std::io::Error),

    #[error("Shared object error: {0}")]
    SharedObject(String),

    #[error("Overlinking against: {package} (file: {file:?})")]
    Overlinking { package: PathBuf, file: PathBuf },

    #[error("Overdepending against: {package}")]
    Overdepending { package: PathBuf },

    #[error("failed to build glob from pattern")]
    GlobError(#[from] globset::Error),
}

#[derive(Debug)]
struct PackageFile {
    pub file: PathBuf,
    pub linked_dsos: HashMap<PathBuf, PackageName>,
    pub shared_libraries: HashSet<PathBuf>,
}

#[derive(Debug)]
struct PackageLinkInfo {
    file: PathBuf,
    linked_packages: Vec<LinkedPackage>,
}

impl fmt::Display for PackageLinkInfo {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        writeln!(
            f,
            "\n[{}] links against:",
            console::style(self.file.display()).white().bold()
        )?;
        for (i, package) in self.linked_packages.iter().enumerate() {
            let connector = if i != self.linked_packages.len() - 1 {
                " ├─"
            } else {
                " └─"
            };
            writeln!(f, "{connector} {package}")?;
        }
        Ok(())
    }
}

#[derive(Debug)]
struct LinkedPackage {
    name: PathBuf,
    link_origin: LinkOrigin,
}

#[derive(Debug)]
enum LinkOrigin {
    System,
    PackageItself,
    ForeignPackage(String),
    NotFound,
}

impl fmt::Display for LinkedPackage {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match &self.link_origin {
            LinkOrigin::System => {
                write!(f, "{} (system)", console::style(self.name.display()).dim())
            }
            LinkOrigin::PackageItself => {
                write!(
                    f,
                    "{} (package)",
                    console::style(self.name.display()).blue()
                )
            }
            LinkOrigin::ForeignPackage(package) => {
                write!(
                    f,
                    "{} ({})",
                    console::style(self.name.display()).green(),
                    console::style(package).italic()
                )
            }
            LinkOrigin::NotFound => {
                write!(f, "{}", console::style(self.name.display()).red())
            }
        }
    }
}

/// Returns the list of resolved run dependencies and a mapping from dependency names to source packages.
fn resolved_run_dependencies_with_sources(
    output: &Output,
    package_to_nature_map: &HashMap<PackageName, PackageNature>,
) -> (Vec<String>, HashMap<String, String>) {
    let mut deps = Vec::new();
    let mut dep_to_source = HashMap::new();

    for dep in output
        .finalized_dependencies
        .clone()
        .expect("failed to get the finalized dependencies")
        .run
        .depends
        .iter()
    {
        // Filter out run exports from build environments
        if let Some(RunExportDependency {
            from,
            source_package,
            ..
        }) = dep.as_run_export()
        {
            if from == &String::from("build") || from == &String::from("cache-build") {
                continue;
            }

            if let Some(package_name) = &dep.spec().name {
                if let Some(nature) = package_to_nature_map.get(package_name) {
                    if nature != &PackageNature::DSOLibrary {
                        continue;
                    }
                }
                let dep_name = package_name.as_source().to_owned();
                deps.push(dep_name.clone());
                // Map the dependency name to its source package
                dep_to_source.insert(dep_name, source_package.clone());
            }
        } else if let Some(package_name) = &dep.spec().name {
            if let Some(nature) = package_to_nature_map.get(package_name) {
                if nature != &PackageNature::DSOLibrary {
                    continue;
                }
            }
            let dep_name = package_name.as_source().to_owned();
            deps.push(dep_name.clone());
            // For non-run-export deps, the source is the same as the dependency
            dep_to_source.insert(dep_name.clone(), dep_name);
        }
    }

    (deps, dep_to_source)
}

/// Returns the system libraries patterns.
fn find_system_libs(output: &Output) -> Result<GlobSet, globset::Error> {
    let mut system_libs = GlobSetBuilder::new();

    if output.build_configuration.target_platform.is_osx() {
        let default_sysroot = vec![
            "*.dylib", // Match any dylib in X11
            "libSystem.B.dylib",
            "libcrypto.0.9.8.dylib",
            "libobjc.A.dylib",
            // e.g. /System/Library/Frameworks/AGL.framework/*
            "*.framework/*", // Match any framework
        ];

        if let Some(sysroot) = output
            .build_configuration
            .variant
            .get(&"CONDA_BUILD_SYSROOT".into())
        {
            system_libs.add(Glob::new(&format!("{}/**/*", sysroot))?);
        } else {
            for v in default_sysroot {
                system_libs.add(Glob::new(v)?);
            }
        }

        return system_libs.build();
    }

    if output.build_configuration.target_platform.is_windows() {
        for v in WIN_ALLOWLIST {
            system_libs.add(Glob::new(v)?);
        }
        return system_libs.build();
    }

    // Try to locate a `sysroot_<platform>` package in either the regular build
    // dependencies *or* the cache build dependencies. The first one found is
    // used to derive the list of system libraries.
    let mut sysroot_package = output
        .finalized_dependencies
        .as_ref()
        .and_then(|deps| deps.build.as_ref())
        .and_then(|deps| {
            deps.resolved.iter().find(|v| {
                v.file_name.starts_with(&format!(
                    "sysroot_{}",
                    output.build_configuration.target_platform
                ))
            })
        })
        .cloned();

    if sysroot_package.is_none() {
        sysroot_package = output
            .finalized_cache_dependencies
            .as_ref()
            .and_then(|deps| deps.build.as_ref())
            .and_then(|deps| {
                deps.resolved.iter().find(|v| {
                    v.file_name.starts_with(&format!(
                        "sysroot_{}",
                        output.build_configuration.target_platform
                    ))
                })
            })
            .cloned();
    }

    if let Some(sysroot_package) = sysroot_package {
        let prefix_record_name = format!(
            "conda-meta/{}-{}-{}.json",
            sysroot_package.package_record.name.as_normalized(),
            sysroot_package.package_record.version,
            sysroot_package.package_record.build
        );

        let sysroot_path = output
            .build_configuration
            .directories
            .build_prefix
            .join(prefix_record_name);

        if let Ok(record) = PrefixRecord::from_path(&sysroot_path) {
            // match all shared objects (e.g. libfoo.so, libfoo.so.1.2)
            let so_glob = Glob::new("*.so*")?.compile_matcher();
            for file in record.files {
                if let Some(file_name) = file.file_name() {
                    if so_glob.is_match(file_name) {
                        system_libs.add(Glob::new(&file_name.to_string_lossy())?);
                    }
                }
            }
        } else {
            // If the prefix record cannot be found, continue without adding patterns.
            tracing::debug!("sysroot prefix record not found at {:?}", sysroot_path);
        }
    }

    // If no sysroot package was found, we fall back to an empty set – in that case every
    // linked library will be validated against the run dependencies.

    system_libs.build()
}

/// Extend the provided `library_mapping` and `package_to_nature` maps with the
/// information contained in a `FinalizedDependencies` instance. This helper
/// avoids duplicating the same merging logic for the regular dependencies and
/// the optional cache dependencies.
fn extend_mappings_from_finalized(
    deps: &Option<FinalizedDependencies>,
    library_mapping: &mut HashMap<String, PackageName>,
    package_to_nature: &mut HashMap<PackageName, PackageNature>,
) {
    let Some(deps) = deps else { return };

    // Only include sysroot_ packages from both host and build environments
    if let Some(host) = &deps.host {
        for (lib, pkg) in &host.library_mapping {
            if pkg.as_normalized().starts_with("sysroot_") {
                library_mapping.insert(lib.clone(), pkg.clone());
            }
        }
        package_to_nature.extend(host.package_nature.clone());
    }

    if let Some(build) = &deps.build {
        for (lib, pkg) in &build.library_mapping {
            if pkg.as_normalized().starts_with("sysroot_") {
                library_mapping.insert(lib.clone(), pkg.clone());
            }
        }
        package_to_nature.extend(build.package_nature.clone());
    }
}

pub fn perform_linking_checks(
    output: &Output,
    new_files: &HashSet<PathBuf>,
    tmp_prefix: &Path,
) -> Result<(), LinkingCheckError> {
    let dynamic_linking = output.recipe.build().dynamic_linking();
    let system_libs = find_system_libs(output)?;

    // Try to load post-process-mappings.json from the cache directory if it exists
    let mut library_mapping = HashMap::new();
    let mut package_to_nature = HashMap::new();
    let mut used_cache_file = false;
    let cache_dir = &output.build_configuration.directories.cache_dir;
    let mappings_path = cache_dir.join("post-process-mappings.json");
    if mappings_path.exists() {
        match fs_err::read_to_string(&mappings_path)
            .ok()
            .and_then(|s| serde_json::from_str::<crate::cache::PostProcessMappings>(&s).ok())
        {
            Some(mappings) => {
                library_mapping = mappings.library_mapping;
                package_to_nature = mappings.package_to_nature;
                used_cache_file = true;
                tracing::info!("Loaded post-process-mappings.json from cache");
            }
            None => {
                tracing::warn!(
                    "Failed to load or parse post-process-mappings.json, falling back to in-memory computation"
                );
            }
        }
    }
    if !used_cache_file {
        // Merge information from the regular dependencies and (if present) from the cache
        if let Some(merged) = output.merged_finalized_dependencies() {
            extend_mappings_from_finalized(
                &Some(merged),
                &mut library_mapping,
                &mut package_to_nature,
            );
        }
    }

    let (resolved_run_dependencies, dep_to_source) =
        resolved_run_dependencies_with_sources(output, &package_to_nature);
    tracing::trace!("Resolved run dependencies: {resolved_run_dependencies:#?}",);
    tracing::trace!("Dependency to source mapping: {dep_to_source:#?}",);

    // check all DSOs and what they are linking
    let target_platform = output.target_platform();
    let host_prefix = output.prefix();
    let mut package_files = Vec::new();
    for file in new_files.iter() {
        // Parse the DSO to get the list of libraries it links to
        match relink::get_relinker(output.build_configuration.target_platform, file) {
            Ok(relinker) => {
                let mut file_dsos = Vec::new();

                let resolved_libraries = relinker.resolve_libraries(tmp_prefix, host_prefix);
                for (lib, resolved) in &resolved_libraries {
                    // filter out @self on macOS
                    if target_platform.is_osx() && lib.to_str() == Some("self") {
                        continue;
                    }

                    let lib = resolved.as_ref().unwrap_or(lib);
                    if let Some(lib_file_name) = lib.file_name().and_then(|n| n.to_str()) {
                        if let Some(package) = library_mapping.get(lib_file_name) {
                            let lib_rel = lib.strip_prefix(host_prefix).unwrap_or(lib);
                            file_dsos.push((lib_rel.to_path_buf(), package.clone()));
                        }
                    }
                }

                package_files.push(PackageFile {
                    file: file
                        .clone()
                        .strip_prefix(tmp_prefix)
                        .unwrap_or(file)
                        .to_path_buf(),
                    linked_dsos: file_dsos.into_iter().collect(),
                    shared_libraries: resolved_libraries
                        .into_iter()
                        .map(|(v, res)| res.unwrap_or(v.to_path_buf()))
                        .collect(),
                });
            }
            Err(RelinkError::UnknownFileFormat) => {
                continue;
            }
            Err(e) => return Err(LinkingCheckError::SharedObject(e.to_string())),
        }
    }
    tracing::trace!("Package files: {package_files:#?}");

    // Track dependencies found in cache builds
    let mut cache_matched_deps: HashSet<String> = HashSet::new();

    let mut linked_packages = Vec::new();
    for package in package_files.iter() {
        let mut link_info = PackageLinkInfo {
            file: package.file.clone(),
            linked_packages: Vec::new(),
        };
        // If the package that we are linking against does not exist in run
        // dependencies then it is "overlinking".
        'library_loop: for lib in &package.shared_libraries {
            let lib = lib.strip_prefix(host_prefix).unwrap_or(lib);

            // skip @self on macOS
            if target_platform.is_osx() && lib.to_str() == Some("self") {
                continue;
            }

            // Check if the package has the library linked.
            if let Some(package) = package.linked_dsos.get(lib) {
                link_info.linked_packages.push(LinkedPackage {
                    name: lib.to_path_buf(),
                    link_origin: LinkOrigin::ForeignPackage(package.as_normalized().to_string()),
                });
                continue;
            }

            // Check if the package itself has the shared library.
            if new_files.iter().any(|file| file.ends_with(lib)) {
                link_info.linked_packages.push(LinkedPackage {
                    name: lib.to_path_buf(),
                    link_origin: LinkOrigin::PackageItself,
                });
                continue;
            }

            // Use library mapping to find which package provides this library
            if let Some(lib_name) = lib.file_name().and_then(|n| n.to_str()) {
                tracing::debug!("Looking up library {} in mapping", lib_name);
                if let Some(package_name) = library_mapping.get(lib_name) {
                    let package_str = package_name.as_normalized();
                    tracing::debug!(
                        "Found library {} belongs to package {}",
                        lib_name,
                        package_str
                    );

                    if package_str.starts_with("sysroot_") {
                        link_info.linked_packages.push(LinkedPackage {
                            name: lib.to_path_buf(),
                            link_origin: LinkOrigin::System,
                        });
                        continue 'library_loop;
                    }

                    let dependency_match = dep_to_source.iter().find(|(dep_name, source_pkg)| {
                        dep_name.as_str() == package_str || source_pkg.as_str() == package_str
                    });

                    if let Some((dep_name, _)) = dependency_match {
                        link_info.linked_packages.push(LinkedPackage {
                            name: lib.to_path_buf(),
                            link_origin: LinkOrigin::ForeignPackage(dep_name.clone()),
                        });

                        // Track that this dependency is used (for cache builds)
                        if output.finalized_cache_dependencies.is_some() {
                            cache_matched_deps.insert(dep_name.clone());
                        }

                        continue 'library_loop;
                    }
                    // If the library is from a conda package but not in run dependencies,
                    // this is overlinking
                    tracing::debug!(
                        "Library {} from package {} not in run dependencies",
                        lib_name,
                        package_str
                    );
                } else {
                    tracing::debug!("Library {} not found in mapping", lib_name);
                }
            }

            // Check if the library is one of the system libraries (i.e. comes from sysroot).
            // We only consider core system libraries that are always available
            // This check comes AFTER library mapping to avoid marking conda-provided libraries as system
            if let Some(file_name) = lib.file_name() {
                let file_name_str = file_name.to_string_lossy();
                tracing::info!("Checking if {} is a system library", file_name_str);

                if system_libs.is_match(file_name) {
                    tracing::info!("{} matched as system library", file_name_str);
                    link_info.linked_packages.push(LinkedPackage {
                        name: lib.to_path_buf(),
                        link_origin: LinkOrigin::System,
                    });
                    continue;
                } else {
                    tracing::info!("{} did not match system library patterns", file_name_str);
                }
            }

            // Check if we allow overlinking.
            if dynamic_linking.missing_dso_allowlist().is_match(lib) {
                tracing::info!(
                    "{lib:?} is missing in run dependencies for {:?}, \
                    yet it is included in the allow list. Skipping...",
                    package.file
                );
            // Error on overlinking.
            } else if dynamic_linking.error_on_overlinking() {
                link_info.linked_packages.push(LinkedPackage {
                    name: lib.to_path_buf(),
                    link_origin: LinkOrigin::NotFound,
                });
                linked_packages.push(link_info);
                linked_packages.iter().for_each(|linked_package| {
                    tracing::info!("\n{linked_package}");
                });

                return Err(LinkingCheckError::Overlinking {
                    package: lib.to_path_buf(),
                    file: package.file.clone(),
                });
            } else {
                let warn_str = format!("Overlinking against {lib:?} for {:?}", package.file);
                tracing::warn!(warn_str);
                output.record_warning(&warn_str);
            }

            link_info.linked_packages.push(LinkedPackage {
                name: lib.to_path_buf(),
                link_origin: LinkOrigin::NotFound,
            });
        }
        linked_packages.push(link_info);
    }

    linked_packages.iter().for_each(|linked_package| {
        tracing::info!("{linked_package}");
    });

    // If there are any unused run dependencies then it is "overdepending".
    for run_dependency in resolved_run_dependencies.iter() {
        // Check if the dependency is used in linked_dsos
        let used_in_linked_dsos = package_files
            .iter()
            .map(|package| {
                package
                    .linked_dsos
                    .values()
                    .map(|v| v.as_source().to_string())
                    .collect::<Vec<String>>()
            })
            .any(|libraries| libraries.contains(run_dependency));

        // Also check if it was matched in cache builds
        let used_in_cache = cache_matched_deps.contains(run_dependency);

        if !used_in_linked_dsos && !used_in_cache {
            if dynamic_linking.error_on_overdepending() {
                return Err(LinkingCheckError::Overdepending {
                    package: PathBuf::from(run_dependency),
                });
            }
            tracing::warn!("Overdepending against {run_dependency}");
            output.record_warning(&format!("Overdepending against {run_dependency}"));
        }
    }

    Ok(())
}
