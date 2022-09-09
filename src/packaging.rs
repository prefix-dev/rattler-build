use self::package_metadata::PathRecord;
use anyhow::Ok;
use anyhow::Result;
use tempdir::TempDir;
use walkdir::WalkDir;

use super::metadata::Metadata;
use fs::File;
use std::io::Write;
use std::{env, fs};

use bzip2::read::{BzDecoder, BzEncoder};
use bzip2::Compression;

use std::path::{Path, PathBuf};
use tar::Builder;
use super::hash::sha256_digest;
use std::collections::HashSet;
use std::os::unix::fs::MetadataExt;

use super::metadata;
pub mod package_metadata;
// use package_metadata;

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

    return Ok(paths);
}

fn compress_tarbz2(directory: &Path) -> Result<BzEncoder<File>, std::io::Error> {
    let tar_bz2 = File::create("archive.tar.bz2")?;

    env::set_current_dir(&directory).expect("OK");

    let enc = BzEncoder::new(tar_bz2, Compression::default());

    let mut ar = tar::Builder::new(enc);
    ar.append_dir_all(".", directory).unwrap();

    return ar.into_inner();
}

fn create_paths_json(paths: Vec<PathBuf>) -> Result<String> {
    let mut paths: Vec<PathBuf> = paths.into_iter().collect();
    paths.sort();
    let mut res = Vec::new();
    for p in paths {
        let meta = fs::metadata(&p)?;
        if meta.is_dir() {
            continue;
        };

        res.push(PathRecord {
            sha256: sha256_digest(&p),
            size: meta.size(),
            path: p,
        })
    }
    return Ok(serde_json::to_string_pretty(&res)?);
}

fn create_index_json(recipe: &metadata::Recipe) -> Result<String> {
    let meta: package_metadata::MetaIndex = package_metadata::MetaIndex {
        name: recipe.name.clone(),
        version: recipe.version.clone(),
        build_string: String::from(""),
        build_number: 0,
        dependencies: recipe.requirements.run.clone(),
        constrains: recipe.requirements.constrains.clone(),
    };
    return Ok(serde_json::to_string_pretty(&recipe)?);
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


pub fn package_conda(meta: Metadata) -> Result<()> {
    let tmp_dir = TempDir::new("package")?;
    let info_folder = tmp_dir.path().join("info");
    fs::create_dir(&info_folder);
    let file_path = info_folder.join("my-temporary-note.txt");
    let mut f = File::create(file_path)?;
    writeln!(f, "Hello My Friend")?;

    let include_dir = tmp_dir.path().join("include");
    fs::create_dir(&include_dir);
    let paths = copy_all(
        "/Users/wolfvollprecht/micromamba/pkgs/libmamba-0.25.0-h1c735bf_1/include",
        include_dir,
    );

    for entry in fs::read_dir(&tmp_dir.path()) {
        println!("Entry {:?}", entry);
    }

    create_paths_json(paths.expect("Could not list paths"));
    compress_tarbz2(tmp_dir.path());

    Ok(())
}
