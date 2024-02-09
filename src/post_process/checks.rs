use std::{
    collections::{HashMap, HashSet},
    path::{Path, PathBuf},
};

use crate::post_process::{package_nature::PackageNature, relink};
use crate::{metadata::Output, post_process::relink::RelinkError};

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

    #[error("Overdepending against: {package} (file: {file:?})")]
    Overdepending { package: PathBuf, file: PathBuf },

    #[error("failed to build glob from pattern")]
    GlobError(#[from] globset::Error),
}

#[derive(Debug)]
struct PackageFile {
    pub file: PathBuf,
    pub linked_dsos: HashMap<PathBuf, PackageName>,
    pub shared_libraries: HashSet<PathBuf>,
    pub system_libs: HashSet<PathBuf>,
}

impl PackageFile {
    fn pretty_print(&self) {
        let mut linked_libraries = Vec::new();
        self.shared_libraries.iter().for_each(|shared_library| {
            linked_libraries.push((shared_library, self.linked_dsos.get(shared_library)));
        });
        if linked_libraries.is_empty() {
            return;
        }
        println!(
            "\n[{}] links against:",
            console::style(self.file.display()).white().bold()
        );
        for (i, (library, package)) in linked_libraries.iter().enumerate() {
            println!(
                " {} {}",
                if i != linked_libraries.len() - 1 {
                    "├─"
                } else {
                    "└─"
                },
                if let Some(package) = package {
                    format!(
                        "{} (from {})",
                        console::style(library.display()).green(),
                        console::style(package.as_normalized()).italic()
                    )
                } else if self
                    .system_libs
                    .iter()
                    .any(|v| v.file_name() == library.file_name())
                {
                    console::style(library.display())
                        .black()
                        .bright()
                        .to_string()
                } else {
                    console::style(library.display()).red().to_string()
                },
            );
        }
        println!();
    }
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
    let resolved_run_dependencies: Vec<String> = output
        .finalized_dependencies
        .clone()
        .expect("failed to get the finalized dependencies")
        .run
        .depends
        .iter()
        .flat_map(|v| v.spec().name.to_owned().map(|v| v.as_source().to_owned()))
        .collect();

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

    let host_prefix = &output.build_configuration.directories.host_prefix;
    tracing::trace!("Package-nature map: {:#?}", package_to_nature_map);

    // check all DSOs and what they are linking
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
                    system_libs: system_libs.clone(),
                });
            }
            Err(RelinkError::UnknownFileFormat) => {
                continue;
            }
            Err(e) => return Err(LinkingCheckError::SharedObject(e.to_string())),
        }
    }

    tracing::trace!("Package files: {:#?}", package_files);
    tracing::trace!(
        "Resolved run dependencies: {:#?}",
        resolved_run_dependencies
    );
    tracing::trace!("System libraries: {:#?}", system_libs);

    for package in package_files.iter() {
        package.pretty_print();
        // If the package that we are linking against does not exist in run
        // dependencies then it is "overlinking".
        for shared_library in package.shared_libraries.iter() {
            if package.linked_dsos.get(shared_library).is_some()
                || system_libs
                    .iter()
                    .any(|v| v.file_name() == shared_library.file_name())
            {
                continue;
            } else if dynamic_linking
                .missing_dso_allowlist()
                .map(|v| v.is_match(shared_library))
                .unwrap_or(false)
            {
                tracing::warn!(
                    "{shared_library:?} is missing in run dependencies for {:?}, \
                    yet it is included in the allow list. Skipping...",
                    package.file
                );
            } else if dynamic_linking.error_on_overlinking() {
                return Err(LinkingCheckError::Overlinking {
                    package: shared_library.clone(),
                    file: package.file.clone(),
                });
            } else {
                tracing::warn!(
                    "Overlinking against {shared_library:?} for {:?}",
                    package.file
                );
            }
        }
    }

    // If there are any unused run dependencies then it is "overdepending".
    for run_dependency in resolved_run_dependencies.iter() {
        let linked_libraries: Vec<(PathBuf, String)> = package_files
            .iter()
            .map(|package| {
                (
                    package.file.clone(),
                    package
                        .linked_dsos
                        .values()
                        .map(|v| v.as_source().to_string())
                        .collect(),
                )
            })
            .collect();
        if let Some((file, _)) = linked_libraries
            .into_iter()
            .find(|(_, l)| !l.contains(run_dependency))
        {
            if dynamic_linking.error_on_overdepending() {
                return Err(LinkingCheckError::Overdepending {
                    package: PathBuf::from(run_dependency),
                    file,
                });
            } else {
                tracing::warn!("Overdepending against {run_dependency} for {file:?}");
            }
        }
    }

    Ok(())
}
