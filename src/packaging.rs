use std::collections::HashSet;
use std::io::{BufReader, Read, Write};
use std::path::{Component, Path, PathBuf};
use std::str::FromStr;
use std::{fs, fs::File};

#[cfg(target_family = "unix")]
use std::os::unix::prelude::OsStrExt;

#[cfg(target_family = "unix")]
use std::os::unix::fs::symlink;

use tempdir::TempDir;
use walkdir::WalkDir;

use rattler_conda_types::package::{
    AboutJson, FileMode, LinkJson, NoArchLinks, PathType, PathsEntry, PrefixPlaceholder,
    PythonEntryPoints, RunExportsJson,
};
use rattler_conda_types::package::{IndexJson, PathsJson};
use rattler_conda_types::Version;
use rattler_digest::compute_file_digest;
use rattler_package_streaming::write::{write_tar_bz2_package, CompressionLevel};

use crate::linux;
use crate::macos;
use crate::metadata::{Output, PlatformOrNoarch};

#[derive(Debug, thiserror::Error)]
pub enum PackagingError {
    #[error("Dependencies are not yet finalized / resolved")]
    DependenciesNotFinalized,

    #[error("Could not open or create, or write to file")]
    IoError(#[from] std::io::Error),

    #[error("Could not strip a prefix from a Path")]
    StripPrefixError(#[from] std::path::StripPrefixError),

    #[error("Could not serialize JSON: {0}")]
    SerializationError(#[from] serde_json::Error),

    #[error("Could not run walkdir: {0}")]
    WalkDirError(#[from] walkdir::Error),

    #[error("Could not get a relative path from {0} to {1}")]
    RelativePathError(PathBuf, PathBuf),

    #[error("Could not find parent directory of {0}")]
    ParentDirError(PathBuf),

    #[error("Failed to parse version {0}")]
    VersionParseError(#[from] rattler_conda_types::ParseVersionError),

    #[error("Failed to relink ELF file: {0}")]
    LinuxRelinkError(#[from] linux::relink::RelinkError),

    #[error("Failed to relink MachO file: {0}")]
    MacOSRelinkError(#[from] macos::relink::RelinkError),

    #[error("License file not found: {0}")]
    LicenseFileNotFound(String),
}

#[allow(unused_variables)]
fn contains_prefix_binary(file_path: &Path, prefix: &Path) -> Result<bool, PackagingError> {
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

fn contains_prefix_text(file_path: &Path, prefix: &Path) -> Result<bool, PackagingError> {
    // Open the file
    let file = File::open(file_path)?;
    let mut buf_reader = BufReader::new(file);

    // Read the file's content
    let mut content = String::new();
    buf_reader.read_to_string(&mut content)?;

    // Check if the content contains the prefix
    let prefix = prefix.to_string_lossy().to_string();
    let contains_prefix = content.contains(&prefix);

    Ok(contains_prefix)
}

fn create_prefix_placeholder(
    file_path: &Path,
    prefix: &Path,
) -> Result<Option<PrefixPlaceholder>, PackagingError> {
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
/// Paths should be given as absolute paths under the `path_prefix` directory.
/// This function will also determine if the file is binary or text, and if it contains the prefix.
fn create_paths_json(
    paths: &HashSet<PathBuf>,
    path_prefix: &Path,
    encoded_prefix: &Path,
) -> Result<String, PackagingError> {
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
fn create_index_json(output: &Output) -> Result<String, PackagingError> {
    // TODO use global timestamp?
    let timestamp = chrono::Utc::now();
    let recipe = &output.recipe;

    let (platform, arch) = match output.build_configuration.target_platform {
        PlatformOrNoarch::Platform(p) => {
            // TODO add better functions in rattler for this
            let pstring = p.to_string();
            let parts: Vec<&str> = pstring.split('-').collect();
            let (platform, arch) = (String::from(parts[0]), String::from(parts[1]));

            match arch.as_str() {
                "64" => (Some(platform), Some("x86_64".to_string())),
                "32" => (Some(platform), Some("x86".to_string())),
                _ => (Some(platform), Some(arch)),
            }
        }
        PlatformOrNoarch::Noarch(_) => (None, None),
    };

    let index_json = IndexJson {
        name: output.name().to_string(),
        version: Version::from_str(output.version())?,
        build: output.build_configuration.hash.clone(),
        build_number: recipe.build.number,
        arch,
        platform,
        subdir: Some(output.build_configuration.target_platform.to_string()),
        license: recipe.about.license.clone(),
        license_family: recipe.about.license_family.clone(),
        timestamp: Some(timestamp),
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

/// Create the about.json file for the given output.
fn create_about_json(output: &Output) -> Result<String, PackagingError> {
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

/// Create the run_exports.json file for the given output.
fn create_run_exports_json(recipe: &Output) -> Result<Option<String>, PackagingError> {
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
pub fn record_files(directory: &PathBuf) -> Result<HashSet<PathBuf>, PackagingError> {
    let mut res = HashSet::new();
    for entry in WalkDir::new(directory) {
        res.insert(entry?.path().to_owned());
    }
    Ok(res)
}

/// This function copies the given file to the destination folder and
/// transforms it on the way if needed.
///
/// * For `noarch: python` packages, the "lib/pythonX.X" prefix is stripped so that only
///   the "site-packages" part is kept. Additionally, any `__pycache__` directories or
///  `.pyc` files are skipped.
/// * Absolute symlinks are made relative so that they are easily relocatable.
fn write_to_dest(
    path: &Path,
    prefix: &Path,
    dest_folder: &Path,
    target_platform: &PlatformOrNoarch,
) -> Result<Option<PathBuf>, PackagingError> {
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

    match dest_path.parent() {
        Some(parent) => {
            if fs::metadata(parent).is_err() {
                fs::create_dir_all(parent)?;
            }
        }
        None => {
            return Err(PackagingError::IoError(std::io::Error::new(
                std::io::ErrorKind::Other,
                "Could not get parent directory",
            )));
        }
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
        fs::read_link(path).and_then(|target| {
            if target.is_absolute() && target.starts_with(prefix) {
                let rel_target = pathdiff::diff_paths(
                    target,
                    path.parent().ok_or(std::io::Error::new(
                        std::io::ErrorKind::Other,
                        "Could not get parent directory",
                    ))?,
                )
                .ok_or(std::io::Error::new(
                    std::io::ErrorKind::Other,
                    "Could not get relative path",
                ))?;

                tracing::trace!(
                    "Making symlink relative {:?} -> {:?}",
                    dest_path,
                    rel_target
                );
                symlink(&rel_target, &dest_path).map_err(|e| {
                    tracing::error!(
                        "Could not create symlink from {:?} to {:?}: {:?}",
                        rel_target,
                        dest_path,
                        e
                    );
                    e
                })?;
            } else {
                if target.is_absolute() {
                    tracing::warn!("Symlink {:?} points outside of the prefix", path);
                }
                symlink(&target, &dest_path).map_err(|e| {
                    tracing::error!(
                        "Could not create symlink from {:?} to {:?}: {:?}",
                        target,
                        dest_path,
                        e
                    );
                    e
                })?;
            }
            Result::Ok(())
        })?;
        Ok(Some(dest_path))
    } else if metadata.is_dir() {
        // skip directories for now
        Ok(None)
    } else {
        tracing::trace!("Copying file {:?} to {:?}", path, dest_path);
        fs::copy(path, &dest_path)?;
        Ok(Some(dest_path))
    }
}

/// This function creates a link.json file for the given output.
fn create_link_json(output: &Output) -> Result<Option<String>, PackagingError> {
    let noarch_links = PythonEntryPoints {
        entry_points: output.recipe.build.entry_points.clone(),
    };

    let link_json = LinkJson {
        noarch: NoArchLinks::Python(noarch_links),
        package_metadata_version: 1,
    };

    Ok(Some(serde_json::to_string_pretty(&link_json)?))
}

/// This function copies the license files to the info/licenses folder.
fn copy_license_files(
    output: &Output,
    tmp_dir_path: &Path,
) -> Result<Option<Vec<PathBuf>>, PackagingError> {
    if let Some(license_files) = &output.recipe.about.license_file {
        let licenses_folder = tmp_dir_path.join("info/licenses/");
        fs::create_dir_all(&licenses_folder)?;
        let mut copied_files = Vec::new();
        for license in license_files {
            // license file can be found either in the recipe folder or in the source folder
            let candidates = vec![
                output
                    .build_configuration
                    .directories
                    .recipe_dir
                    .join(license),
                output
                    .build_configuration
                    .directories
                    .work_dir
                    .join(license),
            ];

            let found = candidates.iter().find(|c| c.exists());
            if let Some(license_file) = found {
                if license_file.is_dir() {
                    todo!("License file is a directory");
                }
                let dest = licenses_folder.join(license);
                fs::copy(license_file, &dest)?;
                copied_files.push(dest);
            } else {
                return Err(PackagingError::LicenseFileNotFound(license.clone()));
            }
        }
        Ok(Some(copied_files))
    } else {
        Ok(None)
    }
}

/// Given an output and a set of new files, create a conda package.
/// This function will copy all the files to a temporary directory and then
/// create a conda package from that. Note that the output needs to have its
/// dependencies finalized before calling this function.
///
/// The `local_channel_dir` is the path to the local channel / output directory.
pub fn package_conda(
    output: &Output,
    new_files: &HashSet<PathBuf>,
    prefix: &Path,
    local_channel_dir: &Path,
) -> Result<PathBuf, PackagingError> {
    if output.finalized_dependencies.is_none() {
        return Err(PackagingError::DependenciesNotFinalized);
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
            linux::relink::relink_paths(&tmp_files, tmp_dir_path, prefix)?;
        } else if p.is_osx() {
            macos::relink::relink_paths(&tmp_files, tmp_dir_path, prefix)?;
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

    if let Some(license_files) = copy_license_files(output, tmp_dir_path)? {
        tmp_files.extend(license_files);
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

    let out_path = output_folder.join(file);
    let file = File::create(&out_path)?;
    write_tar_bz2_package(
        file,
        tmp_dir_path,
        &tmp_files.into_iter().collect::<Vec<_>>(),
        CompressionLevel::Default,
    )?;

    Ok(out_path)
}
