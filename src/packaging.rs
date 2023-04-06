use crate::linux;
use crate::macos;
use crate::metadata::{Output, PlatformOrNoarch};

use rattler_conda_types::package::{
    AboutJson, FileMode, LinkJson, NoArchLinks, PathType, PathsEntry, PrefixPlaceholder,
    PythonEntryPoints, RunExportsJson,
};
use rattler_conda_types::package::{IndexJson, PathsJson};
use rattler_conda_types::Version;
use rattler_digest::compute_file_digest;
use rattler_package_streaming::write::{write_tar_bz2_package, CompressionLevel};

use anyhow::Ok;
use anyhow::Result;

use tempdir::TempDir;
use walkdir::WalkDir;

use fs::File;

use std::fs;
use std::io::{BufReader, Read, Write};

#[cfg(target_family = "unix")]
use std::os::unix::prelude::OsStrExt;

#[cfg(target_family = "unix")]
use std::os::unix::fs::symlink;

use std::str::FromStr;
use std::time::SystemTime;
use std::time::UNIX_EPOCH;

use std::collections::HashSet;

use std::path::{Component, Path, PathBuf};

#[allow(unused_variables)]
fn contains_prefix_binary(file_path: &Path, prefix: &Path) -> Result<bool> {
    // Convert the prefix to a Vec<u8> for binary comparison
    // TODO on Windows check both ascii and utf-8 / 16?
    #[cfg(target_family = "windows")]
    {
        tracing::warn!("Windows is not supported yet for binary prefix checking.");
        return Ok(false);
    }

    #[cfg(target_family = "unix")]
    {
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

fn create_prefix_placeholder(file_path: &Path, prefix: &Path) -> Result<Option<PrefixPlaceholder>> {
    // read first 1024 bytes to determine file type
    let mut file = File::open(file_path)?;
    let mut buffer = [0; 1024];
    let n = file.read(&mut buffer)?;
    let buffer = &buffer[..n];

    let content_type = content_inspector::inspect(buffer);
    let mut has_prefix = None;

    let file_mode = if content_type.is_text() {
        if contains_prefix_text(file_path, prefix)? {
            has_prefix = Some(prefix.to_path_buf());
        }
        FileMode::Text
    } else {
        if contains_prefix_binary(file_path, prefix)? {
            has_prefix = Some(prefix.to_path_buf());
        }
        FileMode::Binary
    };

    if let Some(prefix_placeholder) = has_prefix {
        Ok(Some(PrefixPlaceholder {
            file_mode,
            placeholder: prefix_placeholder.to_string_lossy().to_string(),
        }))
    } else {
        Ok(None)
    }
}

/// Create a `paths.json` file for the given paths.
fn create_paths_json(
    paths: &HashSet<PathBuf>,
    path_prefix: &Path,
    encoded_prefix: &Path,
) -> Result<String> {
    let mut paths_json: PathsJson = PathsJson {
        paths: Vec::new(),
        paths_version: 1,
    };

    for p in itertools::sorted(paths) {
        let meta = fs::symlink_metadata(p)?;

        let relative_path = p.strip_prefix(path_prefix)?.to_path_buf();

        tracing::info!("Adding {:?}", &relative_path);
        if !p.exists() {
            if p.is_symlink() {
                tracing::warn!(
                    "Symlink target does not exist: {:?} -> {:?}",
                    &p,
                    fs::read_link(p)?
                );
                continue;
            }
            tracing::warn!("File does not exist: {:?} (TODO)", &p);
            continue;
        }

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
            let prefix_placeholder = create_prefix_placeholder(p, encoded_prefix)?;

            let digest = compute_file_digest::<sha2::Sha256>(p)?;

            paths_json.paths.push(PathsEntry {
                sha256: Some(digest),
                relative_path,
                path_type: PathType::HardLink,
                prefix_placeholder,
                no_link: false,
                size_in_bytes: Some(meta.len()),
            });
        } else if meta.file_type().is_symlink() {
            let digest = compute_file_digest::<sha2::Sha256>(p)?;

            paths_json.paths.push(PathsEntry {
                sha256: Some(digest),
                relative_path,
                path_type: PathType::SoftLink,
                prefix_placeholder: None,
                no_link: false,
                size_in_bytes: Some(meta.len()),
            });
        }
    }
    Ok(serde_json::to_string_pretty(&paths_json)?)
}

/// Create the index.json file for the given output.
fn create_index_json(output: &Output) -> Result<String> {
    // TODO use global timestamp?
    let now = SystemTime::now();
    let since_the_epoch = now.duration_since(UNIX_EPOCH).expect("Time went backwards");
    let since_the_epoch = since_the_epoch.as_millis() as u64;

    let recipe = &output.recipe;

    let (platform, arch) = match output.build_configuration.target_platform {
        PlatformOrNoarch::Platform(p) => {
            // todo add better functions in rattler for this
            let pstring = p.to_string();
            let parts: Vec<&str> = pstring.split('-').collect();
            (Some(String::from(parts[0])), Some(String::from(parts[1])))
        }
        PlatformOrNoarch::Noarch(_) => (None, None),
    };

    let index_json = IndexJson {
        name: output.name().to_string(),
        version: Version::from_str(output.version()).expect("Could not parse version"),
        build: output.build_configuration.hash.clone(),
        build_number: recipe.build.number,
        arch,
        platform,
        subdir: Some(output.build_configuration.target_platform.to_string()),
        license: recipe.about.license.clone(),
        license_family: recipe.about.license_family.clone(),
        timestamp: Some(since_the_epoch),
        depends: output
            .finalized_dependencies
            .clone()
            .unwrap()
            .run
            .depends
            .iter()
            .map(|d| d.to_string())
            .collect(),
        constrains: output
            .finalized_dependencies
            .clone()
            .unwrap()
            .run
            .constrains
            .iter()
            .map(|d| d.to_string())
            .collect(),
        noarch: recipe.build.noarch,
        track_features: vec![],
        features: None,
    };

    Ok(serde_json::to_string_pretty(&index_json)?)
}

fn create_about_json(output: &Output) -> Result<String> {
    let recipe = &output.recipe;
    let about_json = AboutJson {
        home: recipe.about.home.clone().unwrap_or_default(),
        license: recipe.about.license.clone(),
        license_family: recipe.about.license_family.clone(),
        summary: recipe.about.summary.clone(),
        description: recipe.about.description.clone(),
        doc_url: recipe.about.doc_url.clone().unwrap_or_default(),
        dev_url: recipe.about.dev_url.clone().unwrap_or_default(),
        // TODO ?
        source_url: None,
        channels: output.build_configuration.channels.clone(),
    };

    Ok(serde_json::to_string_pretty(&about_json)?)
}

fn create_run_exports_json(recipe: &Output) -> Result<Option<String>> {
    if let Some(run_exports) = &recipe.recipe.build.run_exports {
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

fn write_to_dest(
    path: &Path,
    prefix: &Path,
    dest_folder: &Path,
    target_platform: &PlatformOrNoarch,
) -> Result<Option<PathBuf>> {
    let path_rel = path.strip_prefix(prefix)?;
    let mut dest_path = dest_folder.join(path_rel);

    if let PlatformOrNoarch::Noarch(t) = target_platform {
        if t.is_python() {
            if path.ends_with(".pyc") || path.ends_with(".pyo") {
                return Ok(None); // skip .pyc files
            }
            // if any part of the path is __pycache__ skip it
            if path_rel
                .components()
                .any(|c| c == Component::Normal("__pycache__".as_ref()))
            {
                return Ok(None);
            }

            // check if site-packages is in the path and strip everything before it
            let pat = std::path::Component::Normal("site-packages".as_ref());
            let parts = path_rel.components();
            let mut new_parts = Vec::new();
            let mut found = false;
            for part in parts {
                if part == pat {
                    found = true;
                }
                if found {
                    new_parts.push(part);
                }
            }

            if !found {
                // skip files that are not in site-packages
                // TODO we need data files?
                return Ok(None);
            }

            dest_path = dest_folder.join(PathBuf::from_iter(new_parts));
        }
    }

    if fs::metadata(dest_path.parent().expect("parent")).is_err() {
        fs::create_dir_all(dest_path.parent().unwrap())?;
    }

    let metadata = fs::symlink_metadata(path)?;

    // make absolute symlinks relative
    if metadata.is_symlink() {
        if let PlatformOrNoarch::Platform(p) = target_platform {
            if p.is_windows() {
                tracing::warn!("Symlinks need administrator privileges on Windows");
            }
        }

        if let Result::Ok(link) = fs::read_link(path) {
            tracing::trace!("Copying link: {:?} -> {:?}", path, link);
        } else {
            tracing::warn!("Could not read link at {:?}", path);
        }

        #[cfg(target_family = "unix")]
        fs::read_link(path)
            .and_then(|target| {
                if target.is_absolute() && target.starts_with(prefix) {
                    let rel_target = pathdiff::diff_paths(
                        target,
                        path.parent().expect("Could not get parent directory"),
                    )
                    .expect("Could not make path relative");

                    tracing::trace!(
                        "Making symlink relative {:?} -> {:?}",
                        dest_path,
                        rel_target
                    );
                    symlink(&rel_target, &dest_path)
                        .map_err(|e| {
                            tracing::error!(
                                "Could not create symlink from {:?} to {:?}: {:?}",
                                rel_target,
                                dest_path,
                                e
                            );
                            e
                        })
                        .expect("Could not create symlink");
                } else {
                    if target.is_absolute() {
                        tracing::warn!("Symlink {:?} points outside of the prefix", path);
                    }
                    symlink(&target, &dest_path)
                        .map_err(|e| {
                            tracing::error!(
                                "Could not create symlink from {:?} to {:?}: {:?}",
                                target,
                                dest_path,
                                e
                            );
                            e
                        })
                        .expect("Could not create symlink");
                }
                Result::Ok(())
            })
            .expect("Could not read link!");
        Ok(Some(dest_path))
    } else if metadata.is_dir() {
        // skip directories for now
        Ok(None)
    } else {
        tracing::trace!("Copying file {:?} to {:?}", path, dest_path);
        fs::copy(path, &dest_path).expect("Could not copy file to dest");
        Ok(Some(dest_path))
        // TODO add relink stuff here?
    }
}

fn create_link_json(output: &Output) -> Result<Option<String>> {
    let noarch_links = PythonEntryPoints {
        entry_points: output.recipe.build.entry_points.clone(),
    };

    let link_json = LinkJson {
        noarch: NoArchLinks::Python(noarch_links),
        package_metadata_version: 1,
    };

    Ok(Some(serde_json::to_string_pretty(&link_json)?))
}

pub fn package_conda(
    output: &Output,
    new_files: &HashSet<PathBuf>,
    prefix: &Path,
    local_channel_dir: &Path,
) -> Result<()> {
    if output.finalized_dependencies.is_none() {
        return Err(anyhow::anyhow!("Dependencies have not been finalized yet!"));
    }

    let tmp_dir = TempDir::new(output.name())?;
    let tmp_dir_path = tmp_dir.path();

    let mut tmp_files = HashSet::new();
    for f in new_files {
        if let Some(dest_file) = write_to_dest(
            f,
            prefix,
            tmp_dir_path,
            &output.build_configuration.target_platform,
        )? {
            tmp_files.insert(dest_file);
        }
    }

    tracing::info!("Copying done!");

    if let PlatformOrNoarch::Platform(p) = &output.build_configuration.target_platform {
        if p.is_linux() {
            linux::relink::relink_paths(&tmp_files, tmp_dir_path, prefix)
                .expect("Could not relink paths");
        } else if p.is_osx() {
            macos::relink::relink_paths(&tmp_files, tmp_dir_path, prefix)
                .expect("Could not relink paths");
        }
    }

    tracing::info!("Relink done!");

    let info_folder = tmp_dir_path.join("info");
    fs::create_dir_all(&info_folder)?;

    let mut paths_json = File::create(info_folder.join("paths.json"))?;
    paths_json.write_all(create_paths_json(&tmp_files, tmp_dir_path, prefix)?.as_bytes())?;
    tmp_files.insert(info_folder.join("paths.json"));

    let mut index_json = File::create(info_folder.join("index.json"))?;
    index_json.write_all(create_index_json(output)?.as_bytes())?;
    tmp_files.insert(info_folder.join("index.json"));

    let mut about_json = File::create(info_folder.join("about.json"))?;
    about_json.write_all(create_about_json(output)?.as_bytes())?;
    tmp_files.insert(info_folder.join("about.json"));

    if let Some(run_exports) = create_run_exports_json(output)? {
        let mut run_exports_json = File::create(info_folder.join("run_exports.json"))?;
        run_exports_json.write_all(run_exports.as_bytes())?;
        tmp_files.insert(info_folder.join("run_exports.json"));
    }

    if let PlatformOrNoarch::Noarch(noarch_type) = output.build_configuration.target_platform {
        if noarch_type.is_python() {
            if let Some(link) = create_link_json(output)? {
                let mut link_json = File::create(info_folder.join("link.json"))?;
                link_json.write_all(link.as_bytes())?;
                tmp_files.insert(info_folder.join("link.json"));
            }
        }
    }

    let output_folder =
        local_channel_dir.join(output.build_configuration.target_platform.to_string());
    tracing::info!("Creating target folder {:?}", output_folder);

    // make dirs
    fs::create_dir_all(&output_folder)?;

    // TODO get proper hash
    let file = format!(
        "{}-{}-{}.tar.bz2",
        output.name(),
        output.version(),
        output.build_configuration.hash
    );

    let file = File::create(output_folder.join(file))?;
    write_tar_bz2_package(
        file,
        tmp_dir_path,
        &tmp_files.into_iter().collect::<Vec<_>>(),
        CompressionLevel::Default,
    )?;

    Ok(())
}
