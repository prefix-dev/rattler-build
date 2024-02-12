use std::{
    collections::{HashMap, HashSet},
    fmt,
    path::{Path, PathBuf},
};

use crate::{metadata::Output, post_process::relink::RelinkError};
use crate::{
    post_process::{package_nature::PackageNature, relink},
    render::resolved_dependencies::DependencyInfo,
};

use globset::Glob;
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
            "[{}] links against:",
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
                write!(
                    f,
                    "{} (system)",
                    console::style(self.name.display()).black().bright()
                )
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
            if let DependencyInfo::RunExport { from, .. } = dep {
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
fn find_system_libs(output: &Output) -> Result<HashSet<PathBuf>, LinkingCheckError> {
    let mut system_libs = HashSet::new();
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
        let sysroot_path = output
            .build_configuration
            .directories
            .build_prefix
            .join("conda-meta")
            .join(sysroot_package.file_name.replace("conda", "json"));
        let record = PrefixRecord::from_path(sysroot_path)?;
        let so_glob = Glob::new("*.so*")?.compile_matcher();
        for file in record.files {
            if let Some(file_name) = file.file_name() {
                if so_glob.is_match(file_name) {
                    system_libs.insert(file);
                }
            }
        }
    }
    Ok(system_libs)
}

pub fn perform_linking_checks(
    output: &Output,
    new_files: &HashSet<PathBuf>,
    tmp_prefix: &Path,
) -> Result<(), LinkingCheckError> {
    let dynamic_linking = output.recipe.build().dynamic_linking();
    let system_libs = find_system_libs(output)?;
    let conda_meta = output
        .build_configuration
        .directories
        .host_prefix
        .join("conda-meta");

    if !conda_meta.exists() {
        return Ok(());
    }

    let mut package_to_nature_map = HashMap::new();
    let mut path_to_package_map = HashMap::new();
    for entry in conda_meta.read_dir()? {
        let entry = entry?;
        let path = entry.path();
        if path.extension().and_then(|v| v.to_str()) == Some("json") {
            let record = PrefixRecord::from_path(path)?;
            let package_nature = PackageNature::from_prefix_record(&record);
            package_to_nature_map.insert(
                record.repodata_record.package_record.name.clone(),
                package_nature,
            );
            for file in record.files {
                path_to_package_map
                    .insert(file, record.repodata_record.package_record.name.clone());
            }
        }
    }
    tracing::trace!("Package-nature map: {package_to_nature_map:#?}");

    let resolved_run_dependencies = resolved_run_dependencies(output, &package_to_nature_map);
    tracing::trace!("Resolved run dependencies: {resolved_run_dependencies:#?}",);

    // check all DSOs and what they are linking
    let host_prefix = &output.build_configuration.directories.host_prefix;
    let mut package_files = Vec::new();
    for file in new_files.iter() {
        // Parse the DSO to get the list of libraries it links to
        match relink::get_relinker(output.build_configuration.target_platform, file) {
            Ok(relinker) => {
                let mut file_dsos = Vec::new();
                for lib in relinker.libraries().iter().map(PathBuf::from) {
                    let libpath = if output.build_configuration.target_platform.is_osx() {
                        match lib.strip_prefix("@rpath/").ok() {
                            Some(suffix) => host_prefix.join("lib").join(suffix),
                            None => lib.to_path_buf(),
                        }
                    } else {
                        PathBuf::from("lib").join(&lib)
                    };
                    if let Some(package) = path_to_package_map.get(&libpath) {
                        if let Some(nature) = package_to_nature_map.get(package) {
                            // Only take shared libraries into account.
                            if nature == &PackageNature::DSOLibrary {
                                file_dsos.push((lib, package.clone()));
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
                    shared_libraries: relinker.libraries(),
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
            //  Check if the package has the library linked.
            if let Some(package) = package.linked_dsos.get(lib) {
                link_info.linked_packages.push(LinkedPackage {
                    name: lib.clone(),
                    link_origin: LinkOrigin::ForeignPackage(package.as_normalized().to_string()),
                });
                continue;
            // Check if the library is one of the system libraries (i.e. comes from sysroot).
            } else if system_libs.iter().any(|v| v.file_name() == lib.file_name()) {
                link_info.linked_packages.push(LinkedPackage {
                    name: lib.clone(),
                    link_origin: LinkOrigin::System,
                });
                continue;
            // Check if the package itself has the shared library.
            } else if package_files.iter().any(|package| {
                lib.file_name()
                    .and_then(|shared_library| {
                        package.file.file_name().map(|v| {
                            v.to_string_lossy()
                                .contains(&*shared_library.to_string_lossy())
                        })
                    })
                    .unwrap_or_default()
            }) {
                link_info.linked_packages.push(LinkedPackage {
                    name: lib.clone(),
                    link_origin: LinkOrigin::PackageItself,
                });
                continue;
            // Check if we allow overlinking.
            } else if dynamic_linking
                .missing_dso_allowlist()
                .map(|v| v.is_match(lib))
                .unwrap_or(false)
            {
                tracing::warn!(
                    "{lib:?} is missing in run dependencies for {:?}, \
                    yet it is included in the allow list. Skipping...",
                    package.file
                );
            // Error on overlinking.
            } else if dynamic_linking.error_on_overlinking() {
                link_info.linked_packages.push(LinkedPackage {
                    name: lib.clone(),
                    link_origin: LinkOrigin::NotFound,
                });
                linked_packages.push(link_info);
                linked_packages.iter().for_each(|linked_package| {
                    println!("\n{linked_package}");
                });
                return Err(LinkingCheckError::Overlinking {
                    package: lib.clone(),
                    file: package.file.clone(),
                });
            } else {
                tracing::warn!("Overlinking against {lib:?} for {:?}", package.file);
            }
            link_info.linked_packages.push(LinkedPackage {
                name: lib.clone(),
                link_origin: LinkOrigin::NotFound,
            });
        }
        linked_packages.push(link_info);
    }

    println!();
    linked_packages.iter().for_each(|linked_package| {
        println!("{linked_package}");
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
            } else {
                tracing::warn!("Overdepending against {run_dependency}");
            }
        }
    }

    Ok(())
}
