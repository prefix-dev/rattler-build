use std::{
    collections::{HashMap, HashSet},
    fmt,
    path::{Path, PathBuf},
};

use crate::{
    metadata::Output,
    post_process::{package_nature::PrefixInfo, relink::RelinkError},
};
use crate::{
    post_process::{package_nature::PackageNature, relink},
    windows::link::WIN_ALLOWLIST,
};

use crate::render::resolved_dependencies::RunExportDependency;
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

/// Returns the list of resolved run dependencies.
fn resolved_run_dependencies(
    output: &Output,
    package_to_nature_map: &HashMap<PackageName, PackageNature>,
) -> Vec<String> {
    output
        .finalized_dependencies
        .clone()
        .expect("failed to get the finalized dependencies")
        .run
        .depends
        .iter()
        .filter(|dep| {
            if let Some(RunExportDependency { from, .. }) = dep.as_run_export() {
                from != &String::from("build")
            } else {
                true
            }
        })
        .flat_map(|dep| {
            if let Some(package_name) = &dep.spec().name {
                if let Some(nature) = package_to_nature_map.get(package_name) {
                    if nature != &PackageNature::DSOLibrary {
                        return None;
                    }
                }
                dep.spec().name.to_owned().map(|v| v.as_source().to_owned())
            } else {
                None
            }
        })
        .collect()
}

/// Returns the system libraries found in sysroot.
fn find_system_libs(output: &Output) -> Result<GlobSet, globset::Error> {
    let mut system_libs = GlobSetBuilder::new();
    if output.build_configuration.target_platform.is_osx() {
        let default_sysroot = vec![
            "/opt/X11/**/*.dylib",
            "/usr/lib/libSystem.B.dylib",
            "/usr/lib/libcrypto.0.9.8.dylib",
            "/usr/lib/libobjc.A.dylib",
            // e.g. /System/Library/Frameworks/AGL.framework/*
            "/System/Library/Frameworks/*.framework/*",
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

    if let Some(sysroot_package) = output
        .finalized_dependencies
        .clone()
        .expect("failed to get the finalized dependencies")
        .build
        .and_then(|deps| {
            deps.resolved.into_iter().find(|v| {
                v.file_name.starts_with(&format!(
                    "sysroot_{}",
                    output.build_configuration.target_platform
                ))
            })
        })
    {
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
        let record = PrefixRecord::from_path(sysroot_path).unwrap();
        let so_glob = Glob::new("*.so*")?.compile_matcher();
        for file in record.files {
            if let Some(file_name) = file.file_name() {
                if so_glob.is_match(file_name) {
                    system_libs.add(Glob::new(&file_name.to_string_lossy())?);
                }
            }
        }
    }
    system_libs.build()
}

pub fn perform_linking_checks(
    output: &Output,
    new_files: &HashSet<PathBuf>,
    tmp_prefix: &Path,
) -> Result<(), LinkingCheckError> {
    let dynamic_linking = output.recipe.build().dynamic_linking();
    let system_libs = find_system_libs(output)?;

    let prefix_info = PrefixInfo::from_prefix(output.prefix())?;

    let resolved_run_dependencies =
        resolved_run_dependencies(output, &prefix_info.package_to_nature);
    tracing::trace!("Resolved run dependencies: {resolved_run_dependencies:#?}",);

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
                    if let Ok(libpath) = lib.strip_prefix(host_prefix) {
                        if let Some(package) = prefix_info
                            .path_to_package
                            .get(&libpath.to_path_buf().into())
                        {
                            if let Some(nature) = prefix_info.package_to_nature.get(package) {
                                // Only take shared libraries into account.
                                if nature == &PackageNature::DSOLibrary {
                                    file_dsos.push((libpath.to_path_buf(), package.clone()));
                                }
                            }
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

    let mut linked_packages = Vec::new();
    for package in package_files.iter() {
        let mut link_info = PackageLinkInfo {
            file: package.file.clone(),
            linked_packages: Vec::new(),
        };
        // If the package that we are linking against does not exist in run
        // dependencies then it is "overlinking".
        for lib in &package.shared_libraries {
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

            // Check if the library is one of the system libraries (i.e. comes from sysroot).
            if system_libs.is_match(lib) {
                link_info.linked_packages.push(LinkedPackage {
                    name: lib.to_path_buf(),
                    link_origin: LinkOrigin::System,
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
        if !package_files
            .iter()
            .map(|package| {
                package
                    .linked_dsos
                    .values()
                    .map(|v| v.as_source().to_string())
                    .collect::<Vec<String>>()
            })
            .any(|libraries| libraries.contains(run_dependency))
        {
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
