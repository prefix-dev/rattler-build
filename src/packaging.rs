use fs_err as fs;
use fs_err::os::unix::fs::symlink;
use fs_err::File;
use std::collections::HashSet;
use std::io::Write;
use std::path::{Component, Path, PathBuf};

use itertools::Itertools;
use tempfile::TempDir;
use walkdir::WalkDir;

use rattler_conda_types::package::ArchiveType;
use rattler_conda_types::package::PathsJson;
use rattler_conda_types::{NoArchType, Platform};
use rattler_package_streaming::write::{
    write_conda_package, write_tar_bz2_package, CompressionLevel,
};

mod metadata;
pub use metadata::{create_prefix_placeholder, to_forward_slash_lossy};

use crate::linux;
use crate::macos;
use crate::metadata::{Output, PackagingSettings};
use crate::package_test::write_test_files;
use crate::post_process;

#[derive(Debug, thiserror::Error)]
pub enum PackagingError {
    #[error("Serde error: {0}")]
    SerdeError(#[from] serde_yaml::Error),

    #[error("Failed to build glob from pattern")]
    GlobError(#[from] globset::Error),

    #[error("Build String is not yet set")]
    BuildStringNotSet,

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

    #[error("Failed to parse version {0}")]
    VersionParseError(#[from] rattler_conda_types::ParseVersionError),

    #[error("Failed to relink ELF file: {0}")]
    LinuxRelinkError(#[from] linux::link::RelinkError),

    #[error("Failed to relink MachO file: {0}")]
    MacOSRelinkError(#[from] macos::link::RelinkError),

    #[error("Relink error: {0}")]
    RelinkError(#[from] crate::post_process::relink::RelinkError),

    #[error(transparent)]
    SourceError(#[from] crate::source::SourceError),

    #[error("could not create python entry point: {0}")]
    CannotCreateEntryPoint(String),

    #[error("Linking check error: {0}")]
    LinkingCheckError(#[from] crate::post_process::LinkingCheckError),

    #[error("Failed to compile Python bytecode: {0}")]
    PythonCompileError(String),
}

/// This function returns a HashSet of (recursively) all the files in the given directory.
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
/// * For `noarch: python` packages, furthermore `bin` is replaced with `python-scripts`, and
///   `Scripts` is replaced with `python-scripts` (on Windows only). All other files are included
///   as-is.
/// * Absolute symlinks are made relative so that they are easily relocatable.
fn write_to_dest(
    path: &Path,
    prefix: &Path,
    dest_folder: &Path,
    target_platform: &Platform,
    noarch_type: &NoArchType,
) -> Result<Option<PathBuf>, PackagingError> {
    let path_rel = path.strip_prefix(prefix)?;
    let mut dest_path = dest_folder.join(path_rel);

    // skip the share/info/dir file because multiple packages would write
    // to the same index file
    if path_rel == Path::new("share/info/dir") {
        return Ok(None);
    }

    let ext = path.extension().unwrap_or_default();
    // pyo considered harmful: https://www.python.org/dev/peps/pep-0488/
    if ext == "pyo" {
        return Ok(None); // skip .pyo files
    }

    if ext == "py" || ext == "pyc" {
        // if we have a .so file of the same name, skip this path
        let so_path = path.with_extension("so");
        let pyd_path = path.with_extension("pyd");
        if so_path.exists() || pyd_path.exists() {
            return Ok(None);
        }
    }

    if noarch_type.is_python() {
        // skip .pyc or .pyo or .egg-info files
        if ["pyc", "egg-info", "pyo"].iter().any(|s| ext.eq(*s)) {
            return Ok(None); // skip .pyc files
        }

        // if any part of the path is __pycache__ skip it
        if path_rel
            .components()
            .any(|c| c == Component::Normal("__pycache__".as_ref()))
        {
            return Ok(None);
        }

        if path_rel
            .components()
            .any(|c| c == Component::Normal("site-packages".as_ref()))
        {
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

            dest_path = dest_folder.join(PathBuf::from_iter(new_parts));
        } else if path_rel.starts_with("bin") || path_rel.starts_with("Scripts") {
            // Replace bin with python-scripts. These should really be encoded
            // as entrypoints but sometimes recipe authors forget or don't know
            // how to do that. Maybe sometimes it's also not actually an
            // entrypoint. The reason for this is that on Windows, the
            // entrypoints are in `Scripts/...` folder, and on Unix they are in
            // the `bin/...` folder. So we need to make sure that the
            // entrypoints are in the right place.
            let mut new_parts = path_rel.components().collect::<Vec<_>>();
            new_parts[0] = Component::Normal("python-scripts".as_ref());

            // on Windows, if the file ends with -script.py, remove the -script.py suffix
            if let Some(Component::Normal(name)) = new_parts.last_mut() {
                if let Some(name_str) = name.to_str() {
                    if target_platform.is_windows() {
                        if let Some(stripped_suffix) = name_str.strip_suffix("-script.py") {
                            *name = stripped_suffix.as_ref();
                        }
                    }
                }
            }

            dest_path = dest_folder.join(PathBuf::from_iter(new_parts));
        } else {
            // keep everything else as-is
            dest_path = dest_folder.join(path_rel);
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
        if target_platform.is_windows() {
            tracing::warn!("Symlinks need administrator privileges on Windows");
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

/// This function copies the license files to the info/licenses folder.
fn copy_license_files(
    output: &Output,
    tmp_dir_path: &Path,
) -> Result<Option<Vec<PathBuf>>, PackagingError> {
    if output.recipe.about().license_file.is_empty() {
        Ok(None)
    } else {
        let license_globs = output.recipe.about().license_file.clone();

        let licenses_folder = tmp_dir_path.join("info/licenses/");
        fs::create_dir_all(&licenses_folder)?;

        let copy_dir = crate::source::copy_dir::CopyDir::new(
            &output.build_configuration.directories.recipe_dir,
            &licenses_folder,
        )
        .with_parse_globs(license_globs.iter().map(AsRef::as_ref))
        .use_gitignore(false)
        .run()?;

        let copied_files_recipe_dir = copy_dir.copied_paths();
        let any_include_matched_recipe_dir = copy_dir.any_include_glob_matched();

        let copy_dir = crate::source::copy_dir::CopyDir::new(
            &output.build_configuration.directories.work_dir,
            &licenses_folder,
        )
        .with_parse_globs(license_globs.iter().map(AsRef::as_ref))
        .use_gitignore(false)
        .run()?;

        let copied_files_work_dir = copy_dir.copied_paths();
        let any_include_matched_work_dir = copy_dir.any_include_glob_matched();

        let copied_files = copied_files_recipe_dir
            .iter()
            .chain(copied_files_work_dir)
            .map(PathBuf::from)
            .collect::<Vec<PathBuf>>();

        if !any_include_matched_work_dir && !any_include_matched_recipe_dir {
            tracing::warn!("No include glob matched for copying license files");
        }

        if copied_files.is_empty() {
            tracing::warn!("No license files were copied");
        }

        Ok(Some(copied_files))
    }
}

/// We check that each `pyc` file in the package is also present as a `py` file.
/// This is a temporary measure to avoid packaging `pyc` files that are not
/// generated by the build process.
fn filter_pyc(path: &Path, new_files: &HashSet<PathBuf>) -> bool {
    if let (Some(ext), Some(parent)) = (path.extension(), path.parent()) {
        if ext == "pyc" {
            let has_pycache = parent.ends_with("__pycache__");
            let pyfile = if has_pycache {
                // a pyc file with a pycache parent should be removed
                // replace two last dots with .py
                // these paths look like .../__pycache__/file_dependency.cpython-311.pyc
                // where the `file_dependency.py` path would be found in the parent directory from __pycache__
                let stem = path
                    .file_name()
                    .expect("unreachable as extension doesn't exist without filename")
                    .to_string_lossy()
                    .to_string();
                let py_stem = stem.rsplitn(3, '.').last().unwrap_or_default();
                if let Some(pp) = parent.parent() {
                    pp.join(format!("{}.py", py_stem))
                } else {
                    return true;
                }
            } else {
                path.with_extension("py")
            };

            if !new_files.contains(&pyfile) {
                return true;
            }
        }
    }
    false
}

fn write_recipe_folder(
    output: &Output,
    tmp_dir_path: &Path,
) -> Result<Vec<PathBuf>, PackagingError> {
    let recipe_folder = tmp_dir_path.join("info/recipe/");
    let recipe_dir = &output.build_configuration.directories.recipe_dir;

    let copy_result = crate::source::copy_dir::CopyDir::new(recipe_dir, &recipe_folder).run()?;

    let mut files = Vec::from(copy_result.copied_paths());
    // write the variant config to the appropriate file
    let variant_config_file = recipe_folder.join("variant_config.yaml");
    let mut variant_config = File::create(&variant_config_file)?;
    variant_config
        .write_all(serde_yaml::to_string(&output.build_configuration.variant)?.as_bytes())?;
    files.push(variant_config_file);

    // TODO(recipe): define how we want to render it exactly!
    let rendered_recipe_file = recipe_folder.join("rendered_recipe.yaml");
    let mut rendered_recipe = File::create(&rendered_recipe_file)?;
    rendered_recipe.write_all(serde_yaml::to_string(&output)?.as_bytes())?;
    files.push(rendered_recipe_file);

    Ok(files)
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
    packaging_settings: &PackagingSettings,
) -> Result<(PathBuf, PathsJson), PackagingError> {
    if output.finalized_dependencies.is_none() {
        return Err(PackagingError::DependenciesNotFinalized);
    }

    let tmp_dir = TempDir::with_prefix(output.name().as_normalized())?;
    let tmp_dir_path = tmp_dir.path();

    let mut tmp_files = HashSet::new();
    for f in new_files {
        let stripped = f.strip_prefix(prefix)?;
        // temporary measure to remove pyc files that are not supposed to be there
        if filter_pyc(f, new_files) {
            continue;
        }

        if output.recipe.build().noarch().is_python() {
            // we need to remove files in bin/ that are registered as entry points
            if stripped.starts_with("bin") {
                if let Some(name) = stripped.file_name() {
                    if output
                        .recipe
                        .build()
                        .python()
                        .entry_points
                        .iter()
                        .any(|ep| ep.command == name.to_string_lossy())
                    {
                        continue;
                    }
                }
            }
            // Windows
            else if stripped.starts_with("Scripts") {
                if let Some(name) = stripped.file_name() {
                    if output
                        .recipe
                        .build()
                        .python()
                        .entry_points
                        .iter()
                        .any(|ep| {
                            format!("{}.exe", ep.command) == name.to_string_lossy()
                                || format!("{}-script.py", ep.command) == name.to_string_lossy()
                        })
                    {
                        continue;
                    }
                }
            }
        }

        if let Some(dest_file) = write_to_dest(
            f,
            prefix,
            tmp_dir_path,
            &output.build_configuration.target_platform,
            output.recipe.build().noarch(),
        )? {
            tmp_files.insert(dest_file);
        }
    }

    tracing::info!("Copying done!");

    let dynamic_linking = output
        .recipe
        .build()
        .dynamic_linking()
        .cloned()
        .unwrap_or_default();
    let relocation_config = dynamic_linking.binary_relocation().unwrap_or_default();

    if output.build_configuration.target_platform != Platform::NoArch
        && !relocation_config.no_relocation()
    {
        let rpath_allowlist = dynamic_linking.rpath_allowlist();
        let mut binaries = tmp_files.clone();
        if let Some(globs) = relocation_config.relocate_paths() {
            binaries.retain(|v| globs.is_match(v));
        }

        post_process::relink::relink(
            &binaries,
            tmp_dir_path,
            prefix,
            &output.build_configuration.target_platform,
            &dynamic_linking.rpaths(),
            rpath_allowlist,
        )?;

        post_process::linking_checks(
            output,
            &binaries,
            dynamic_linking.missing_dso_allowlist(),
            dynamic_linking.error_on_overlinking(),
            dynamic_linking.error_on_overdepending(),
        )?;
    }

    tmp_files.extend(post_process::python::python(
        output,
        &tmp_files,
        tmp_dir_path,
    )?);

    tracing::info!("Relink done!");

    let info_folder = tmp_dir_path.join("info");
    fs::create_dir_all(&info_folder)?;

    let paths_json = File::create(info_folder.join("paths.json"))?;
    let paths_json_struct = output.paths_json(&tmp_files, tmp_dir_path, prefix)?;
    serde_json::to_writer_pretty(paths_json, &paths_json_struct)?;
    tmp_files.insert(info_folder.join("paths.json"));

    let index_json = File::create(info_folder.join("index.json"))?;
    serde_json::to_writer_pretty(index_json, &output.index_json()?)?;
    tmp_files.insert(info_folder.join("index.json"));

    let hash_input_json = File::create(info_folder.join("hash_input.json"))?;
    serde_json::to_writer_pretty(hash_input_json, &output.build_configuration.hash.hash_input)?;
    tmp_files.insert(info_folder.join("hash_input.json"));

    let about_json = File::create(info_folder.join("about.json"))?;
    serde_json::to_writer_pretty(about_json, &output.about_json())?;
    tmp_files.insert(info_folder.join("about.json"));

    if let Some(run_exports) = output.run_exports_json()? {
        let run_exports_json = File::create(info_folder.join("run_exports.json"))?;
        serde_json::to_writer_pretty(run_exports_json, &run_exports)?;
        tmp_files.insert(info_folder.join("run_exports.json"));
    }

    if let Some(license_files) = copy_license_files(output, tmp_dir_path)? {
        tmp_files.extend(license_files);
    }

    let mut variant_config = File::create(info_folder.join("hash_input.json"))?;
    variant_config
        .write_all(serde_json::to_string_pretty(&output.build_configuration.variant)?.as_bytes())?;

    if output.build_configuration.store_recipe {
        let recipe_files = write_recipe_folder(output, tmp_dir_path)?;
        tmp_files.extend(recipe_files);
    }

    let test_files = write_test_files(output, tmp_dir_path)?;
    tmp_files.extend(test_files);

    // create any entry points or link.json for noarch packages
    if output.recipe.build().noarch().is_python() {
        let link_json = File::create(info_folder.join("link.json"))?;
        serde_json::to_writer_pretty(link_json, &output.link_json()?)?;
        tmp_files.insert(info_folder.join("link.json"));
    } else {
        let entry_points = post_process::python::create_entry_points(output, tmp_dir_path)?;
        tmp_files.extend(entry_points);
    }

    // print sorted files
    tracing::info!("\nFiles in package:\n");
    tmp_files
        .iter()
        .map(|x| x.strip_prefix(tmp_dir_path))
        .collect::<Result<Vec<_>, _>>()?
        .iter()
        .sorted()
        .for_each(|f| tracing::info!("  - {}", f.to_string_lossy()));

    let output_folder =
        local_channel_dir.join(output.build_configuration.target_platform.to_string());
    tracing::info!("Creating target folder {:?}", output_folder);

    fs::create_dir_all(&output_folder)?;

    let identifier = output
        .identifier()
        .ok_or(PackagingError::BuildStringNotSet)?;
    let out_path = output_folder.join(format!(
        "{}{}",
        identifier,
        packaging_settings.archive_type.extension()
    ));
    let file = File::create(&out_path)?;

    match packaging_settings.archive_type {
        ArchiveType::TarBz2 => {
            write_tar_bz2_package(
                file,
                tmp_dir_path,
                &tmp_files.into_iter().collect::<Vec<_>>(),
                CompressionLevel::Numeric(packaging_settings.compression_level),
                Some(&output.build_configuration.timestamp),
            )?;
        }
        ArchiveType::Conda => {
            // This is safe because we're just putting it together before
            write_conda_package(
                file,
                tmp_dir_path,
                &tmp_files.into_iter().collect::<Vec<_>>(),
                CompressionLevel::Numeric(packaging_settings.compression_level),
                packaging_settings.compression_threads,
                &identifier,
                Some(&output.build_configuration.timestamp),
            )?;
        }
    }

    Ok((out_path, paths_json_struct))
}

#[cfg(test)]
mod test {
    use super::metadata::create_prefix_placeholder;

    #[test]
    fn detect_prefix() {
        let test_data = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("test-data/binary_files/binary_file_fallback");
        let prefix = std::path::Path::new(env!("CARGO_MANIFEST_DIR"));

        create_prefix_placeholder(&test_data, prefix).unwrap();
    }
}
