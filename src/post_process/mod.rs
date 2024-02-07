use std::{
    collections::{HashMap, HashSet},
    fmt,
    path::PathBuf,
};

use crate::{
    linux::link::SharedObject, macos::link::Dylib, post_process::package_nature::PackageNature,
};
use rattler_conda_types::{PackageName, PrefixRecord};

use crate::metadata::Output;

pub mod package_nature;
pub mod python;
pub mod relink;

#[derive(thiserror::Error, Debug)]
pub enum LinkingCheckError {
    #[error("Error reading file: {0}")]
    Io(#[from] std::io::Error),

    #[error("Linux relink error: {0}")]
    LinuxRelink(#[from] crate::linux::link::RelinkError),

    #[error("macOS relink error: {0}")]
    MacOSRelink(#[from] crate::macos::link::RelinkError),

    #[error("Overlinking against: {package} (file: {file:?})")]
    Overlinking { package: PathBuf, file: PathBuf },

    #[error("Overdepending against: {package} (file: {file:?})")]
    Overdepending { package: PathBuf, file: PathBuf },
}

#[derive(Debug)]
struct PackageFile {
    pub file: PathBuf,
    pub linked_dsos: HashMap<PathBuf, PackageName>,
    pub shared_libraries: Vec<PathBuf>,
}

impl fmt::Display for PackageFile {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "[{}] links against:\n", self.file.display())?;
        for (library, package) in &self.linked_dsos {
            write!(
                f,
                " -> {} (from {})\n",
                library.display(),
                package.as_source()
            )?;
        }
        Ok(())
    }
}

pub fn linking_checks(
    output: &Output,
    new_files: &HashSet<PathBuf>,
) -> Result<(), LinkingCheckError> {
    let dynamic_linking = output.recipe.build().dynamic_linking();

    // collect all json files in prefix / conda-meta
    let conda_meta = output
        .build_configuration
        .directories
        .host_prefix
        .join("conda-meta");

    if !conda_meta.exists() {
        return Ok(());
    }

    let resolved_run_dependencies: Vec<String> = output
        .finalized_dependencies
        .clone()
        .unwrap()
        .run
        .depends
        .iter()
        .flat_map(|v| v.spec().name.to_owned().map(|v| v.as_source().to_owned()))
        .collect();

    let mut package_to_nature_map = HashMap::new();
    let mut path_to_package_map = HashMap::new();
    for entry in conda_meta.read_dir()? {
        let entry = entry?;
        let path = entry.path();
        if path.extension().and_then(|v| v.to_str()) == Some("json") {
            let record = PrefixRecord::from_path(path)?;
            let package_nature = package_nature::PackageNature::from_prefix_record(&record);
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

    tracing::trace!("Path-package map: {:#?}", path_to_package_map);
    tracing::trace!("Package-nature map: {:#?}", package_to_nature_map);

    // check all DSOs and what they are linking
    let mut package_files = Vec::new();
    for file in new_files.iter() {
        // Parse the DSO to get the list of libraries it links to
        if output.build_configuration.target_platform.is_osx() {
            if !Dylib::test_file(file)? {
                continue;
            }
            let dylib = Dylib::new(file)?;
            let mut file_dsos = Vec::new();
            for lib in &dylib.libraries {
                let lib = match lib.strip_prefix("@rpath/").ok() {
                    Some(suffix) => host_prefix.join("lib").join(suffix),
                    None => lib.to_path_buf(),
                };
                if let Some(package) = path_to_package_map.get(&lib) {
                    if let Some(nature) = package_to_nature_map.get(package) {
                        // Only take shared libraries into account.
                        if nature == &PackageNature::DSOLibrary {
                            file_dsos.push((lib, package.clone()));
                        }
                    }
                }
            }
            package_files.push(PackageFile {
                file: file.clone(),
                linked_dsos: file_dsos.into_iter().collect(),
                shared_libraries: dylib.libraries.clone().iter().map(PathBuf::from).collect(),
            });
        } else {
            if !SharedObject::test_file(file)? {
                continue;
            }
            let so = SharedObject::new(file)?;
            let mut file_dsos = Vec::new();
            for lib in so.libraries.iter().map(PathBuf::from) {
                let libpath = PathBuf::from("lib").join(&lib);
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
                file: file.clone(),
                linked_dsos: file_dsos.into_iter().collect(),
                shared_libraries: so.libraries.iter().map(PathBuf::from).collect(),
            });
        }
    }

    tracing::trace!("Package files: {:#?}", package_files);
    tracing::trace!(
        "Resolved run dependencies: {:#?}",
        resolved_run_dependencies
    );

    for package in package_files.iter() {
        println!("\n{}\n", package);

        // If the package that we are linking against does not exist in run
        // dependencies then it is "overlinking".
        for shared_library in package.shared_libraries.iter() {
            if package.linked_dsos.get(shared_library).is_some() {
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
