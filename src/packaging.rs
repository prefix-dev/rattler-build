use crate::metadata::Output;

use rattler_conda_types::package::{FileMode, PathType, PathsEntry};
use rattler_conda_types::package::{IndexJson, PathsJson};
use rattler_conda_types::{Version, NoArchType};

use anyhow::Ok;
use anyhow::Result;

use tempdir::TempDir;
use walkdir::WalkDir;

use fs::File;

use std::io::{Read, Write};
use std::str::FromStr;
use std::time::SystemTime;
use std::time::UNIX_EPOCH;
use std::{env, fs};

use bzip2::read::BzEncoder;
use bzip2::Compression;

use super::hash::sha256_digest;
use std::collections::HashSet;

use std::path::{Path, PathBuf};

pub fn copy_all<U: AsRef<Path>, V: AsRef<Path>>(from: U, to: V) -> Result<Vec<PathBuf>> {
    let mut stack = Vec::new();
    let mut paths: Vec<PathBuf> = Vec::new();
    stack.push(PathBuf::from(from.as_ref()));

    let output_root = PathBuf::from(to.as_ref());
    let input_root = PathBuf::from(from.as_ref()).components().count();
    while let Some(working_path) = stack.pop() {
        println!("process: {:?}", &working_path);

        // Generate a relative path
        let src: PathBuf = working_path.components().skip(input_root).collect();

        // Create a destination if missing
        let dest = if src.components().count() == 0 {
            output_root.clone()
        } else {
            output_root.join(&src)
        };
        if fs::metadata(&dest).is_err() {
            println!(" mkdir: {:?}", dest);
            fs::create_dir_all(&dest)?;
        }

        for entry in fs::read_dir(working_path)? {
            let entry = entry?;
            let path = entry.path();
            if path.is_dir() {
                stack.push(path);
            } else {
                match path.file_name() {
                    Some(filename) => {
                        let dest_path = dest.join(filename);
                        println!("  copy: {:?} -> {:?}", &path, &dest_path);
                        fs::copy(&path, &dest_path)?;
                        paths.push(dest_path);
                    }
                    None => {
                        println!("failed: {:?}", path);
                    }
                }
            }
        }
    }

    Ok(paths)
}

fn compress_tarbz2(
    source_directory: &Path,
    filename: &String,
) -> Result<BzEncoder<File>, std::io::Error> {
    let tar_bz2 = File::create(filename)?;

    env::set_current_dir(source_directory).expect("OK");

    let enc = BzEncoder::new(tar_bz2, Compression::default());

    let mut ar = tar::Builder::new(enc);
    ar.append_dir_all(".", source_directory).unwrap();

    ar.into_inner()
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

    let subdir = String::from("osx-arm64");
    let (platform, arch) = if subdir == "noarch" {
        (None, None)
    } else {
        let parts: Vec<&str> = subdir.split('-').collect();
        (Some(String::from(parts[0])), Some(String::from(parts[1])))
    };

    let index_json = IndexJson {
        name: recipe.name.clone(),
        version: Version::from_str(&recipe.version).expect("Could not parse version"),
        build: String::from("hash_0"),
        build_number: 0,
        arch,
        platform,
        subdir: Some(String::from("osx-arm64")),
        license: Some(String::from("BSD-3-Clause")),
        license_family: Some(String::from("BSD")),
        timestamp: Some(since_the_epoch),
        depends: recipe.requirements.run.clone(),
        constrains: recipe.requirements.constrains.clone(),
        noarch: NoArchType::none(),
        track_features: vec![],
        features: None,
    };

    Ok(serde_json::to_string_pretty(&index_json)?)
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

    println!("Copying done!");

    let info_folder = tmp_dir.path().join("info");
    fs::create_dir(&info_folder)?;

    let mut paths_json = File::create(info_folder.join("paths.json"))?;
    paths_json.write_all(create_paths_json(new_files, prefix)?.as_bytes())?;

    let mut index_json = File::create(info_folder.join("index.json"))?;
    index_json.write_all(create_index_json(output)?.as_bytes())?;

    // TODO get proper hash
    compress_tarbz2(
        tmp_dir.path(),
        &format!("{}-{}-{}.tar.bz2", output.name, output.version, "hash_0"),
    )?;

    Ok(())
}
