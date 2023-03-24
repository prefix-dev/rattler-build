use crate::metadata::Output;

use rattler_conda_types::package::{AboutJson, FileMode, PathType, PathsEntry};
use rattler_conda_types::package::{IndexJson, PathsJson};
use rattler_conda_types::{NoArchType, Version};
use rattler_package_streaming::write::{write_tar_bz2_package, CompressionLevel};

use anyhow::Ok;
use anyhow::Result;

use tempdir::TempDir;
use walkdir::WalkDir;

use fs::File;

use std::fs;
use std::io::{Read, Write};
use std::str::FromStr;
use std::time::SystemTime;
use std::time::UNIX_EPOCH;

use super::hash::sha256_digest;
use std::collections::HashSet;

use std::path::PathBuf;

fn create_paths_json(paths: &HashSet<PathBuf>, prefix: &PathBuf) -> Result<String> {
    let mut paths_json: PathsJson = PathsJson {
        paths: Vec::new(),
        paths_version: 1,
    };

    for p in itertools::sorted(paths) {
        let meta = fs::metadata(p)?;

        let relative_path = p.strip_prefix(prefix)?.to_path_buf();

        if meta.is_dir() {
            // check if dir is empty, and only then add it to paths.json
            let mut entries = fs::read_dir(p)?;
            if entries.next().is_none() {
                let path_entry = PathsEntry {
                    sha256: None,
                    relative_path,
                    path_type: PathType::Directory,
                    // TODO put this away?
                    file_mode: FileMode::Binary,
                    prefix_placeholder: None,
                    no_link: false,
                    size_in_bytes: None,
                };
                paths_json.paths.push(path_entry);
            }
        } else if meta.is_file() {
            // read first 1024 bytes to determine file type
            let mut file = File::open(p)?;
            let mut buffer = [0; 1024];
            let n = file.read(&mut buffer)?;
            let buffer = &buffer[..n];

            let content_type = content_inspector::inspect(buffer);
            let file_type = if content_type.is_text() {
                FileMode::Text
            } else {
                FileMode::Binary
            };

            paths_json.paths.push(PathsEntry {
                sha256: Some(sha256_digest(p)),
                relative_path,
                path_type: PathType::HardLink,
                file_mode: file_type,
                prefix_placeholder: None,
                no_link: false,
                size_in_bytes: Some(meta.len()),
            });
        } else if meta.file_type().is_symlink() {
            paths_json.paths.push(PathsEntry {
                sha256: None,
                relative_path,
                path_type: PathType::SoftLink,
                file_mode: FileMode::Binary,
                prefix_placeholder: None,
                no_link: false,
                size_in_bytes: None,
            });
        }
    }
    Ok(serde_json::to_string_pretty(&paths_json)?)
}

fn create_index_json(recipe: &Output) -> Result<String> {
    // TODO use global timestamp?
    let now = SystemTime::now();
    let since_the_epoch = now.duration_since(UNIX_EPOCH).expect("Time went backwards");
    let since_the_epoch = since_the_epoch.as_millis() as u64;

    let subdir = recipe.build_configuration.target_platform.clone();
    let (platform, arch) = if subdir == "noarch" {
        (None, None)
    } else {
        let parts: Vec<&str> = subdir.split('-').collect();
        (Some(String::from(parts[0])), Some(String::from(parts[1])))
    };

    let index_json = IndexJson {
        name: recipe.name.clone(),
        version: Version::from_str(&recipe.version).expect("Could not parse version"),
        build: recipe.build_configuration.hash.clone(),
        build_number: recipe.build.number,
        arch,
        platform,
        subdir: Some(recipe.build_configuration.target_platform.clone()),
        license: recipe.about.license.clone(),
        license_family: recipe.about.license_family.clone(),
        timestamp: Some(since_the_epoch),
        depends: recipe.requirements.run.clone(),
        constrains: recipe.requirements.constrains.clone(),
        noarch: NoArchType::none(),
        track_features: vec![],
        features: None,
    };

    Ok(serde_json::to_string_pretty(&index_json)?)
}

fn create_about_json(recipe: &Output) -> Result<String> {
    let about_json = AboutJson {
        home: recipe.about.home.clone(),
        license: recipe.about.license.clone(),
        license_family: recipe.about.license_family.clone(),
        summary: recipe.about.summary.clone(),
        description: recipe.about.description.clone(),
        doc_url: recipe.about.doc_url.clone(),
        dev_url: recipe.about.dev_url.clone(),
        source_url: None,
        channels: vec![], // TODO
    };

    Ok(serde_json::to_string_pretty(&about_json)?)
}

pub fn record_files(directory: &PathBuf) -> Result<HashSet<PathBuf>> {
    let mut res = HashSet::new();

    for entry in WalkDir::new(directory) {
        let entry = entry?.path().to_owned();
        println!("{:?}", &entry);
        res.insert(entry);
    }

    Ok(res)
}

pub fn package_conda(
    output: &Output,
    new_files: &HashSet<PathBuf>,
    prefix: &PathBuf,
) -> Result<()> {
    let tmp_dir = TempDir::new(&output.name)?;

    for f in new_files {
        let f_rel = f.strip_prefix(prefix)?;
        let dest = tmp_dir.path().join(f_rel);
        if fs::metadata(dest.parent().expect("parent")).is_err() {
            fs::create_dir_all(dest.parent().unwrap())?;
        }

        let meta = fs::metadata(f)?;
        if meta.is_dir() {
            continue;
        };

        println!("Copying {:?} to {:?}", f, dest);
        fs::copy(f, dest).expect("Could not copy to dest");
    }

    tracing::info!("Copying done!");

    let info_folder = tmp_dir.path().join("info");
    fs::create_dir(&info_folder)?;

    let mut paths_json = File::create(info_folder.join("paths.json"))?;
    paths_json.write_all(create_paths_json(new_files, prefix)?.as_bytes())?;

    let mut index_json = File::create(info_folder.join("index.json"))?;
    index_json.write_all(create_index_json(output)?.as_bytes())?;

    let mut about_json = File::create(info_folder.join("about.json"))?;
    about_json.write_all(create_about_json(output)?.as_bytes())?;

    // TODO get proper hash
    let file = tmp_dir.path().join(format!(
        "{}-{}-{}.tar.bz2",
        output.name, output.version, output.build_configuration.hash
    ));
    let file = File::create(file)?;
    let new_files = new_files.iter().cloned().collect::<Vec<_>>();
    write_tar_bz2_package(file, tmp_dir.path(), &new_files, CompressionLevel::Default)?;

    Ok(())
}
