use crate::metadata::Output;

use self::package_metadata::PathRecord;
use self::package_metadata::Paths;
use anyhow::Ok;
use anyhow::Result;
use tempdir::TempDir;
use walkdir::WalkDir;

use fs::File;
use std::io::Write;
use std::time::SystemTime;
use std::time::UNIX_EPOCH;
use std::{env, fs};

use bzip2::read::BzEncoder;
use bzip2::Compression;

use super::hash::sha256_digest;
use std::collections::HashSet;
use std::os::unix::fs::MetadataExt;
use std::path::{Path, PathBuf};

pub mod package_metadata;

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
    let mut paths_json: Paths = Paths::default();
    let mut paths: Vec<PathBuf> = paths.clone().into_iter().collect();

    // Sort paths to get "reproducible" metadata
    paths.sort();

    for p in paths {
        let meta = fs::metadata(&p)?;
        if meta.is_dir() {
            continue;
        };

        paths_json.paths.push(PathRecord {
            sha256: sha256_digest(&p),
            path_type: String::from("hardlink"),
            size_in_bytes: meta.size(),
            path: p.strip_prefix(prefix)?.into(),
        })
    }
    Ok(serde_json::to_string_pretty(&paths_json)?)
}

// {
//     "arch": "arm64",
//     "build": "hffc8910_0",
//     "build_number": 0,
//     "depends": [
//       "libcxx >=14.0.4"
//     ],
//     "license": "BSD-3-Clause",
//     "license_family": "BSD",
//     "name": "xsimd",
//     "platform": "osx",
//     "subdir": "osx-arm64",
//     "timestamp": 1661428002610,
//     "version": "9.0.1"
//   }
fn create_index_json(recipe: &Output) -> Result<String> {
    // TODO use global timestamp?
    let now = SystemTime::now();
    let since_the_epoch = now.duration_since(UNIX_EPOCH).expect("Time went backwards");

    let meta: package_metadata::MetaIndex = package_metadata::MetaIndex {
        name: recipe.name.clone(),
        version: recipe.version.clone(),
        build: String::from("hash_0"),
        build_number: 0,
        arch: String::from("arm64"),
        platform: String::from("osx"),
        subdir: String::from("osx-arm64"),
        license: String::from("BSD-3-Clause"),
        license_family: String::from("BSD"),
        timestamp: since_the_epoch.as_millis(),
        depends: recipe.requirements.run.clone(),
        constrains: recipe.requirements.constrains.clone(),
    };
    Ok(serde_json::to_string_pretty(&meta)?)
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

    let mut paths_json = File::create(&info_folder.join("paths.json"))?;
    paths_json.write_all(create_paths_json(new_files, prefix)?.as_bytes())?;

    let mut index_json = File::create(&info_folder.join("index.json"))?;
    index_json.write_all(create_index_json(output)?.as_bytes())?;

    // TODO get proper hash
    compress_tarbz2(
        tmp_dir.path(),
        &format!("{}-{}-{}.tar.bz2", output.name, output.version, "hash_0"),
    )?;

    Ok(())
}
