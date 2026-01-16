use rayon::prelude::*;
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
use text_stub_library::TbdVersionedRecord;
use walkdir::WalkDir;

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
            if let Some(rattler_conda_types::PackageNameMatcher::Exact(exact_name)) =
                &dep.spec().name
            {
                if let Some(nature) = package_to_nature_map.get(exact_name)
                    && nature != &PackageNature::DSOLibrary
                {
                    return None;
                }
                Some(exact_name.as_source().to_owned())
            } else {
                None
            }
        })
        .collect()
}

/// Extract install names from .tbd files in the given sysroot directory.
/// This parses the text-based stub files that macOS SDKs use to represent
/// dynamic libraries, extracting the actual runtime library paths.
fn extract_tbd_install_names(sysroot: &Path) -> Vec<String> {
    let mut install_names = Vec::new();

    for entry in WalkDir::new(sysroot)
        .into_iter()
        .filter_map(|e| e.ok())
        .filter(|e| e.path().extension().is_some_and(|ext| ext == "tbd"))
    {
        let Ok(content) = fs_err::read_to_string(entry.path()) else {
            continue;
        };

        let Ok(records) = text_stub_library::parse_str(&content) else {
            tracing::warn!("Failed to parse .tbd file at {}", entry.path().display());
            continue;
        };

        for record in records {
            let install_name = match &record {
                TbdVersionedRecord::V1(r) => &r.install_name,
                TbdVersionedRecord::V2(r) => &r.install_name,
                TbdVersionedRecord::V3(r) => &r.install_name,
                TbdVersionedRecord::V4(r) => &r.install_name,
            };

            if !install_name.is_empty() {
                install_names.push(install_name.clone());
            }
        }
    }

    install_names
}

fn add_osx_system_libs(
    output: &Output,
    system_libs: &mut GlobSetBuilder,
) -> Result<(), globset::Error> {
    // If CONDA_BUILD_SYSROOT is set, parse .tbd files to extract install names.
    // This matches conda-build's behavior of reading actual runtime library
    // paths from the SDK's text-based stub files.
    if let Some(sysroot) = output
        .build_configuration
        .variant
        .get(&"CONDA_BUILD_SYSROOT".into())
    {
        let sysroot_path = PathBuf::from(sysroot.to_string());
        if sysroot_path.exists() {
            for name in extract_tbd_install_names(&sysroot_path) {
                system_libs.add(Glob::new(&name)?);
            }
            return Ok(());
        }
    }

    // Fallback: match conda-build behavior - allow any library in sysroot directories
    // https://github.com/conda/conda-build/blob/61e9bb24588d8b353321c11de5452d57aa2f85ca/conda_build/post.py#L1371-L1384
    const DEFAULT_SYSROOT_PATTERNS: &[&str] = &[
        "/usr/lib/**/*",
        "/opt/X11/**/*.dylib",
        // e.g. /System/Library/Frameworks/AGL.framework/*
        "/System/Library/Frameworks/*.framework/*",
    ];

    for pattern in DEFAULT_SYSROOT_PATTERNS {
        system_libs.add(Glob::new(pattern)?);
    }

    Ok(())
}

fn add_windows_system_libs(system_libs: &mut GlobSetBuilder) -> Result<(), globset::Error> {
    for pattern in WIN_ALLOWLIST {
        system_libs.add(Glob::new(pattern)?);
    }
    Ok(())
}

fn add_linux_system_libs(
    output: &Output,
    system_libs: &mut GlobSetBuilder,
) -> Result<(), globset::Error> {
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
            if let Some(file_name) = file.file_name()
                && so_glob.is_match(file_name)
            {
                system_libs.add(Glob::new(&file_name.to_string_lossy())?);
            }
        }
    }
    Ok(())
}

/// Returns the system libraries found in sysroot.
fn find_system_libs(output: &Output) -> Result<GlobSet, globset::Error> {
    let mut system_libs = GlobSetBuilder::new();
    let platform = &output.build_configuration.target_platform;

    if platform.is_osx() {
        add_osx_system_libs(output, &mut system_libs)?;
    } else if platform.is_windows() {
        add_windows_system_libs(&mut system_libs)?;
    } else {
        add_linux_system_libs(output, &mut system_libs)?;
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

    // Parallel processing of DSO files
    let package_files: Vec<PackageFile> = new_files
        .par_iter()
        .filter_map(|file| {
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
                        if let Ok(libpath) = lib.strip_prefix(host_prefix)
                            && let Some(package) = prefix_info
                                .path_to_package
                                .get(&libpath.to_path_buf().into())
                            && let Some(nature) = prefix_info.package_to_nature.get(package)
                        {
                            // Only take shared libraries into account.
                            if nature == &PackageNature::DSOLibrary {
                                file_dsos.push((libpath.to_path_buf(), package.clone()));
                            }
                        }
                    }

                    Some(PackageFile {
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
                    })
                }
                Err(RelinkError::UnknownFileFormat) => None,
                Err(e) => {
                    tracing::error!("Failed to get relinker for file {}: {}", file.display(), e);
                    None
                }
            }
        })
        .collect();
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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_extract_tbd_install_names() {
        let test_sysroot = Path::new(env!("CARGO_MANIFEST_DIR")).join("test-data/tbd_files");

        let install_names = extract_tbd_install_names(&test_sysroot);

        // Should extract install names from both .tbd files
        assert!(
            install_names.contains(&"/usr/lib/libSystem.B.dylib".to_string()),
            "Should contain libSystem.B.dylib, got: {:?}",
            install_names
        );
        assert!(
            install_names.contains(&"/usr/lib/libz.1.dylib".to_string()),
            "Should contain libz.1.dylib, got: {:?}",
            install_names
        );
    }
}
