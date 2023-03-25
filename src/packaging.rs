use crate::metadata::Output;

use rattler_conda_types::package::{AboutJson, FileMode, PathType, PathsEntry, RunExportsJson};
use rattler_conda_types::package::{IndexJson, PathsJson};
use rattler_conda_types::{NoArchType, Version};
use rattler_digest::compute_file_digest;
use rattler_package_streaming::write::{write_tar_bz2_package, CompressionLevel};

use anyhow::Ok;
use anyhow::Result;

use tempdir::TempDir;
use walkdir::WalkDir;

use fs::File;

use std::fs;
use std::io::{BufReader, Read, Write};
use std::os::unix::prelude::OsStrExt;
use std::str::FromStr;
use std::time::SystemTime;
use std::time::UNIX_EPOCH;

use std::collections::HashSet;

use std::path::{Path, PathBuf};

fn contains_prefix_binary(file_path: &Path, prefix: &Path) -> Result<bool> {
    // Convert the prefix to a Vec<u8> for binary comparison
    // TODO on Windows check both ascii and utf-8 / 16?
    let prefix_bytes = prefix.as_os_str().as_bytes().to_vec();

    // Open the file
    let file = File::open(file_path)?;
    let mut buf_reader = BufReader::new(file);

    // Read the file's content
    let mut content = Vec::new();
    buf_reader.read_to_end(&mut content)?;

    // Check if the content contains the prefix bytes
    let contains_prefix = content
        .windows(prefix_bytes.len())
        .any(|window| window == prefix_bytes.as_slice());

    Ok(contains_prefix)
}

fn contains_prefix_text(file_path: &Path, prefix: &Path) -> Result<bool> {
    // Open the file
    let file = File::open(file_path)?;
    let mut buf_reader = BufReader::new(file);

    // Read the file's content
    let mut content = String::new();
    buf_reader.read_to_string(&mut content)?;

    // Check if the content contains the prefix
    let contains_prefix = content.contains(prefix.to_str().unwrap());

    Ok(contains_prefix)
}

struct PathMetadata {
    file_type: FileMode,
    has_prefix: Option<PathBuf>,
}

impl PathMetadata {
    fn from_path(file_path: &Path, prefix: &Path) -> Result<PathMetadata> {
        // read first 1024 bytes to determine file type
        let mut file = File::open(file_path)?;
        let mut buffer = [0; 1024];
        let n = file.read(&mut buffer)?;
        let buffer = &buffer[..n];

        let content_type = content_inspector::inspect(buffer);
        let file_type = if content_type.is_text() {
            FileMode::Text
        } else {
            FileMode::Binary
        };

        let mut has_prefix = None;
        if file_type == FileMode::Binary {
            if contains_prefix_binary(file_path, prefix)? {
                has_prefix = Some(prefix.to_path_buf());
            }
        } else if contains_prefix_text(file_path, prefix)? {
            has_prefix = Some(prefix.to_path_buf());
        }

        Ok(PathMetadata {
            file_type,
            has_prefix,
        })
    }
}

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
            // TODO figure out under which conditions we should add empty dirs to paths.json
            // let mut entries = fs::read_dir(p)?;
            // if entries.next().is_none() {
            //     let path_entry = PathsEntry {
            //         sha256: None,
            //         relative_path,
            //         path_type: PathType::Directory,
            //         // TODO put this away?
            //         file_mode: FileMode::Binary,
            //         prefix_placeholder: None,
            //         no_link: false,
            //         size_in_bytes: None,
            //     };
            //     paths_json.paths.push(path_entry);
            // }
        } else if meta.is_file() {
            let metadata = PathMetadata::from_path(p, prefix)?;

            let digest = compute_file_digest::<sha2::Sha256>(p)?;

            paths_json.paths.push(PathsEntry {
                sha256: Some(hex::encode(digest)),
                relative_path,
                path_type: PathType::HardLink,
                file_mode: metadata.file_type,
                prefix_placeholder: metadata.has_prefix.map(|p| p.to_str().unwrap().to_string()),
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
        home: recipe.about.home.clone().unwrap_or_default(),
        license: recipe.about.license.clone(),
        license_family: recipe.about.license_family.clone(),
        summary: recipe.about.summary.clone(),
        description: recipe.about.description.clone(),
        doc_url: recipe.about.doc_url.clone().unwrap_or_default(),
        dev_url: recipe.about.dev_url.clone().unwrap_or_default(),
        source_url: None,
        channels: vec![], // TODO
    };

    Ok(serde_json::to_string_pretty(&about_json)?)
}

fn create_run_exports_json(recipe: &Output) -> Result<Option<String>> {
    if let Some(run_exports) = &recipe.build.run_exports {
        let run_exports_json = RunExportsJson {
            strong: run_exports.strong.clone(),
            weak: run_exports.weak.clone(),
            strong_constrains: run_exports.strong_constrains.clone(),
            weak_constrains: run_exports.weak_constrains.clone(),
            noarch: run_exports.noarch.clone(),
        };

        Ok(Some(serde_json::to_string_pretty(&run_exports_json)?))
    } else {
        Ok(None)
    }
}

// This function returns a HashSet of (recursively) all the files in the given directory.
pub fn record_files(directory: &PathBuf) -> Result<HashSet<PathBuf>> {
    let mut res = HashSet::new();
    for entry in WalkDir::new(directory) {
        res.insert(entry?.path().to_owned());
    }
    Ok(res)
}

pub fn package_conda(
    output: &Output,
    new_files: &HashSet<PathBuf>,
    prefix: &PathBuf,
    local_channel_dir: &PathBuf,
) -> Result<()> {
    let tmp_dir = TempDir::new(&output.name)?;

    let mut tmp_files = Vec::new();
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

        fs::copy(f, &dest).expect("Could not copy to dest");
        tmp_files.push(dest.to_path_buf());
    }

    tracing::info!("Copying done!");

    let info_folder = tmp_dir.path().join("info");
    fs::create_dir(&info_folder)?;

    let mut paths_json = File::create(info_folder.join("paths.json"))?;
    paths_json.write_all(create_paths_json(new_files, prefix)?.as_bytes())?;
    tmp_files.push(info_folder.join("paths.json"));

    let mut index_json = File::create(info_folder.join("index.json"))?;
    index_json.write_all(create_index_json(output)?.as_bytes())?;
    tmp_files.push(info_folder.join("index.json"));

    let mut about_json = File::create(info_folder.join("about.json"))?;
    about_json.write_all(create_about_json(output)?.as_bytes())?;
    tmp_files.push(info_folder.join("about.json"));

    let mut run_exports_json = File::create(info_folder.join("run_exports.json"))?;
    if let Some(run_exports) = create_run_exports_json(output)? {
        run_exports_json.write_all(run_exports.as_bytes())?;
        tmp_files.push(info_folder.join("run_exports.json"));
    }

    let output_folder = local_channel_dir.join(&output.build_configuration.target_platform);
    // make dirs
    fs::create_dir_all(&output_folder)?;

    // TODO get proper hash
    let file = format!(
        "{}-{}-{}.tar.bz2",
        output.name, output.version, output.build_configuration.hash
    );

    let file = File::create(output_folder.join(file))?;
    write_tar_bz2_package(file, tmp_dir.path(), &tmp_files, CompressionLevel::Default)?;

    Ok(())
}
