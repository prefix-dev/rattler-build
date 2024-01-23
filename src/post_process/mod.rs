use std::{
    collections::{HashMap, HashSet},
    path::PathBuf,
};

use crate::{
    linux::link::SharedObject, macos::link::Dylib, post_process::package_nature::PackageNature,
};
use rattler_conda_types::PrefixRecord;

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
}

pub fn linking_checks(
    output: &Output,
    new_files: &HashSet<PathBuf>,
) -> Result<(), LinkingCheckError> {
    // collect all json files in prefix / conda-meta
    let conda_meta = output
        .build_configuration
        .directories
        .host_prefix
        .join("conda-meta");

    if !conda_meta.exists() {
        return Ok(());
    }

    let mut run_dependencies = output
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
    for file in new_files.iter() {
        println!("file: {}", file.display());
        // Parse the DSO to get the list of libraries it links to
        if output.build_configuration.target_platform.is_osx() {
            if Dylib::test_file(file)? {
                println!("dylib");
            } else {
                println!("not dylib");
                continue;
            }

            let dylib = Dylib::new(file)?;
            println!("dylib: {:?}", dylib);
            for lib in dylib.libraries {
                println!("lib: {:?}", lib);

                let lib = match lib.strip_prefix("@rpath/").ok() {
                    Some(suffix) => host_prefix.join("lib").join(suffix),
                    None => lib,
                };

                if let Some(package) = path_to_package_map.get(&lib) {
                    println!("package: {:?}", package);
                    if let Some(nature) = package_to_nature_map.get(package) {
                        println!("nature: {:?}", nature);
                    }
                }
            }
        } else {
            let so = SharedObject::new(file)?;
            println!("so: {:?}", so);
            for lib in so.libraries {
                println!("lib: {:?}", lib);
                let libpath = PathBuf::from("lib").join(lib);
                if let Some(package) = path_to_package_map.get(&libpath) {
                    println!("package: {:?}", package);
                    if let Some(nature) = package_to_nature_map.get(package) {
                        println!("nature: {:?}", nature);
                        if nature == &PackageNature::DSOLibrary {
                            let package_name = package.as_normalized().to_string();
                            if let Some(package_pos) =
                                run_dependencies.iter().position(|v| v == &package_name)
                            {
                                run_dependencies.remove(package_pos);
                            } else {
                                tracing::warn!("Underlinking against {}", package_name)
                            }
                        }
                    }
                }
            }
            if !run_dependencies.is_empty() {
                tracing::warn!("Overlinking against {}", run_dependencies.join(","))
            }
        }
    }

    Ok(())
}
