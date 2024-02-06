use std::{
    collections::{HashMap, HashSet},
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

    #[error("Underlinking against: {package_name} (file: {file:?})")]
    Underlinking { package_name: String, file: PathBuf },

    #[error("Overlinking against: {packages} (file: {file:?})")]
    Overlinking { packages: String, file: PathBuf },
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

    let run_dependencies = output
        .recipe
        .requirements
        .run()
        .iter()
        .flat_map(|v| v.name())
        .collect::<Vec<String>>();
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

    // check all DSOs and what they are linking
    let mut file_to_dso_map = HashMap::<&PathBuf, Vec<&PackageName>>::new();
    for file in new_files.iter() {
        // Parse the DSO to get the list of libraries it links to
        if output.build_configuration.target_platform.is_osx() {
            if !Dylib::test_file(file)? {
                continue;
            }
            let dylib = Dylib::new(file)?;
            let mut dsos = Vec::new();
            for lib in dylib.libraries {
                let lib = match lib.strip_prefix("@rpath/").ok() {
                    Some(suffix) => host_prefix.join("lib").join(suffix),
                    None => lib,
                };
                if let Some(package) = path_to_package_map.get(&lib) {
                    if let Some(nature) = package_to_nature_map.get(package) {
                        // Only take shared libraries into account.
                        if nature == &PackageNature::DSOLibrary {
                            dsos.push(package);
                        }
                    }
                }
            }
            file_to_dso_map.insert(file, dsos);
        } else {
            if !SharedObject::test_file(file)? {
                continue;
            }
            let so = SharedObject::new(file)?;
            let mut dsos = Vec::new();
            for lib in so.libraries {
                let libpath = PathBuf::from("lib").join(lib);
                if let Some(package) = path_to_package_map.get(&libpath) {
                    if let Some(nature) = package_to_nature_map.get(package) {
                        // Only take shared libraries into account.
                        if nature == &PackageNature::DSOLibrary {
                            dsos.push(package);
                        }
                    }
                }
            }
            file_to_dso_map.insert(file, dsos);
        }
    }

    for (file, dsos) in file_to_dso_map {
        let mut run_dependencies = run_dependencies.clone();
        for dso in &dsos {
            let package_name = dso.as_normalized().to_string();
            // If the package that we are linking against does not exist in run
            // dependencies then it is "underlinking".
            if let Some(package_pos) = run_dependencies
                .iter()
                .position(|v| v == &package_name.trim_start_matches("lib"))
            {
                run_dependencies.remove(package_pos);
            } else if dynamic_linking
                .missing_dso_allowlist()
                .map(|v| v.is_match(&package_name))
                .unwrap_or(false)
            {
                tracing::warn!(
                    "{package_name} is missing in run dependencies for {:?}, \
            yet it is included in the allow list. Skipping...",
                    file
                );
            } else if dynamic_linking.error_on_overdepending() {
                return Err(LinkingCheckError::Underlinking {
                    package_name,
                    file: file.clone(),
                });
            } else {
                tracing::warn!("Underlinking against {package_name} for {:?}", file);
            }
        }

        // If there are any unused run/host dependencies then it is "overlinking".
        if !run_dependencies.is_empty() {
            if dynamic_linking.error_on_overlinking() {
                return Err(LinkingCheckError::Overlinking {
                    packages: run_dependencies.join(","),
                    file: file.clone(),
                });
            } else {
                tracing::warn!(
                    "Overlinking against {} for {:?}",
                    run_dependencies.join(","),
                    file
                );
            }
        }
    }

    Ok(())
}
