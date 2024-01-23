use std::{
    collections::{HashMap, HashSet},
    path::PathBuf,
};

use crate::{linux::link::SharedObject, macos::link::Dylib};
use rattler_conda_types::PrefixRecord;

use crate::metadata::Output;

pub mod package_nature;
pub mod python;
pub mod relink;

#[derive(thiserror::Error, Debug)]
pub enum LinkingCheckError {}

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

    let mut package_to_nature_map = HashMap::new();
    let mut path_to_package_map = HashMap::new();
    for entry in conda_meta.read_dir().unwrap() {
        let entry = entry.unwrap();
        let path = entry.path();
        if path.extension().unwrap() == "json" {
            let record = PrefixRecord::from_path(path).unwrap();
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
            if Dylib::test_file(file).unwrap() {
                println!("dylib");
            } else {
                println!("not dylib");
                continue;
            }

            let dylib = Dylib::new(file).unwrap();
            // println!("dylib: {:?}", dylib);
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
            let so = SharedObject::new(file).unwrap();
            // println!("so: {:?}", so);
            for lib in so.libraries {
                println!("lib: {:?}", lib);
                let libpath = PathBuf::from("lib").join(lib);
                if let Some(package) = path_to_package_map.get(&libpath) {
                    println!("package: {:?}", package);
                    if let Some(nature) = package_to_nature_map.get(package) {
                        println!("nature: {:?}", nature);
                    }
                }
            }
        }
    }

    Ok(())
}
