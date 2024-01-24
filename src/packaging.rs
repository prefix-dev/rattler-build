use content_inspector::ContentType;
use fs_err as fs;
use fs_err::File;
use rattler::install::{get_windows_launcher, python_entry_point_template, PythonInfo};
use std::collections::HashSet;
use std::io::{BufReader, Read, Write};
use std::path::{Component, Path, PathBuf};

#[cfg(target_family = "unix")]
use std::os::unix::prelude::OsStrExt;

#[cfg(target_family = "unix")]
use std::os::unix::fs::symlink;

use itertools::Itertools;
use tempfile::TempDir;
use walkdir::WalkDir;

use rattler_conda_types::package::{
    AboutJson, ArchiveType, FileMode, LinkJson, NoArchLinks, PathType, PathsEntry,
    PrefixPlaceholder, PythonEntryPoints,
};
use rattler_conda_types::package::{IndexJson, PathsJson};
use rattler_conda_types::{NoArchType, Platform};
use rattler_digest::compute_file_digest;
use rattler_package_streaming::write::{
    write_conda_package, write_tar_bz2_package, CompressionLevel,
};

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
}

#[allow(unused_variables)]
fn contains_prefix_binary(file_path: &Path, prefix: &Path) -> Result<bool, PackagingError> {
    // Convert the prefix to a Vec<u8> for binary comparison
    // TODO on Windows check both ascii and utf-8 / 16?
    #[cfg(target_family = "windows")]
    {
        tracing::warn!("Windows is not supported yet for binary prefix checking.");
        Ok(false)
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

/// This function requires we know the file content we are matching against is UTF-8
/// In case the source is non utf-8 it will fail with a read error
fn contains_prefix_text(file_path: &Path, prefix: &Path) -> Result<bool, PackagingError> {
    // Open the file
    let file = File::open(file_path)?;
    let mut buf_reader = BufReader::new(file);

    // Read the file's content
    let mut content = String::new();
    buf_reader.read_to_string(&mut content)?;

    // Check if the content contains the prefix
    let src = prefix.to_string_lossy();
    let contains_prefix = content.contains(&src.to_string());

    #[cfg(target_os = "windows")]
    {
        // absolute and unc paths will break but it,
        // will break either way as C:/ can't be converted
        // to something meaningful in unix either way
        let s = to_forward_slash_lossy(prefix);
        return Ok(contains_prefix || content.contains(s.to_string().as_str()));
    }

    #[cfg(not(target_os = "windows"))]
    Ok(contains_prefix)
}

#[allow(dead_code)]
pub(crate) fn to_forward_slash_lossy(path: &Path) -> std::borrow::Cow<'_, str> {
    #[cfg(target_os = "windows")]
    {
        let mut buf = String::new();
        for c in path.components() {
            match c {
                Component::RootDir => { /* root on windows can be skipped */ }
                Component::CurDir => buf.push('.'),
                Component::ParentDir => buf.push_str(".."),
                Component::Prefix(prefix) => {
                    buf.push_str(&prefix.as_os_str().to_string_lossy());
                    continue;
                }
                Component::Normal(s) => buf.push_str(&s.to_string_lossy()),
            }
            // use `/` instead of `\`
            buf.push('/');
        }

        fn ends_with_main_sep(p: &Path) -> bool {
            use std::os::windows::ffi::OsStrExt as _;
            p.as_os_str().encode_wide().last() == Some(std::path::MAIN_SEPARATOR as u16)
        }
        if buf != "/" && !ends_with_main_sep(path) && buf.ends_with('/') {
            buf.pop();
        }

        std::borrow::Cow::Owned(buf)
    }
    #[cfg(not(target_os = "windows"))]
    {
        path.to_string_lossy()
    }
}

fn create_prefix_placeholder(
    file_path: &Path,
    prefix: &Path,
) -> Result<Option<PrefixPlaceholder>, PackagingError> {
    // exclude pyc and pyo files from prefix replacement
    if let Some(ext) = file_path.extension() {
        if ext == "pyc" || ext == "pyo" {
            return Ok(None);
        }
    }
    // read first 1024 bytes to determine file type
    let mut file = File::open(file_path)?;
    let mut buffer = [0; 1024];
    let n = file.read(&mut buffer)?;
    let buffer = &buffer[..n];

    let content_type = content_inspector::inspect(buffer);
    let mut has_prefix = None;

    let file_mode = if content_type.is_text()
        // treat everything else as binary for now!
        && matches!(content_type, ContentType::UTF_8 | ContentType::UTF_8_BOM)
    {
        match contains_prefix_text(file_path, prefix) {
            Ok(true) => {
                has_prefix = Some(prefix.to_path_buf());
                FileMode::Text
            }
            Ok(false) => FileMode::Text,
            Err(PackagingError::IoError(ioe)) if ioe.kind() == std::io::ErrorKind::InvalidData => {
                FileMode::Binary
            }
            Err(e) => return Err(e),
        }
    } else {
        FileMode::Binary
    };

    if file_mode == FileMode::Binary && contains_prefix_binary(file_path, prefix)? {
        has_prefix = Some(prefix.to_path_buf());
    }

    Ok(has_prefix.map(|prefix_placeholder| PrefixPlaceholder {
        file_mode,
        placeholder: prefix_placeholder.to_string_lossy().to_string(),
    }))
}

/// Create a `paths.json` file structure for the given paths.
/// Paths should be given as absolute paths under the `path_prefix` directory.
/// This function will also determine if the file is binary or text, and if it contains the prefix.
fn create_paths_json(
    paths: &HashSet<PathBuf>,
    path_prefix: &Path,
    encoded_prefix: &Path,
) -> Result<PathsJson, PackagingError> {
    let mut paths_json = PathsJson {
        paths: Vec::new(),
        paths_version: 1,
    };

    for p in itertools::sorted(paths) {
        let meta = fs::symlink_metadata(p)?;

        let relative_path = p.strip_prefix(path_prefix)?.to_path_buf();

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
            let mut entries = fs::read_dir(p)?;
            if entries.next().is_none() {
                let path_entry = PathsEntry {
                    sha256: None,
                    relative_path,
                    path_type: PathType::Directory,
                    prefix_placeholder: None,
                    no_link: false,
                    size_in_bytes: None,
                };
                paths_json.paths.push(path_entry);
            }
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

    Ok(paths_json)
}

/// Create the index.json file for the given output.
fn create_index_json(output: &Output) -> Result<String, PackagingError> {
    let recipe = &output.recipe;

    let (platform, arch) = match output.build_configuration.target_platform {
        Platform::NoArch => (None, None),
        p => {
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
    };

    let index_json = IndexJson {
        name: output.name().clone(),
        version: output.version().parse()?,
        build: output
            .build_string()
            .ok_or(PackagingError::BuildStringNotSet)?
            .to_string(),
        build_number: recipe.build().number(),
        arch,
        platform,
        subdir: Some(output.build_configuration.target_platform.to_string()),
        license: recipe.about().license().map(|l| l.to_string()),
        license_family: recipe.about().license_family().map(|l| l.to_owned()),
        timestamp: Some(output.build_configuration.timestamp),
        depends: output
            .finalized_dependencies
            .as_ref()
            .ok_or(PackagingError::DependenciesNotFinalized)?
            .run
            .depends
            .iter()
            .map(|dep| dep.spec().to_string())
            .dedup()
            .collect(),
        constrains: output
            .finalized_dependencies
            .as_ref()
            .ok_or(PackagingError::DependenciesNotFinalized)?
            .run
            .constrains
            .iter()
            .map(|dep| dep.spec().to_string())
            .dedup()
            .collect(),
        noarch: *recipe.build().noarch(),
        track_features: vec![],
        features: None,
    };

    Ok(serde_json::to_string_pretty(&index_json)?)
}

/// Create the about.json file for the given output.
fn create_about_json(output: &Output) -> Result<String, PackagingError> {
    let recipe = &output.recipe;
    // FIXME: Updated recipe specs don't allow for vectors in any of the About fields except license_files
    let about_json = AboutJson {
        home: recipe
            .about()
            .homepage()
            .cloned()
            .map(|s| vec![s])
            .unwrap_or_default(),
        license: recipe.about().license().map(|s| s.to_string()),
        license_family: recipe.about().license_family().map(|s| s.to_owned()),
        summary: recipe.about().summary().map(|s| s.to_owned()),
        description: recipe.about().description().map(|s| s.to_owned()),
        doc_url: recipe
            .about()
            .documentation()
            .cloned()
            .map(|url| vec![url])
            .unwrap_or_default(),
        dev_url: recipe
            .about()
            .repository()
            .cloned()
            .map(|url| vec![url])
            .unwrap_or_default(),
        // TODO ?
        source_url: None,
        channels: output.build_configuration.channels.clone(),
    };

    Ok(serde_json::to_string_pretty(&about_json)?)
}

/// Create the run_exports.json file for the given output.
fn create_run_exports_json(output: &Output) -> Result<Option<String>, PackagingError> {
    if let Some(run_exports) = &output
        .finalized_dependencies
        .as_ref()
        .ok_or(PackagingError::DependenciesNotFinalized)?
        .run
        .run_exports
    {
        Ok(Some(serde_json::to_string_pretty(run_exports)?))
    } else {
        Ok(None)
    }
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

/// This function creates a link.json file for the given output.
fn create_link_json(output: &Output) -> Result<Option<String>, PackagingError> {
    let noarch_links = PythonEntryPoints {
        entry_points: output.recipe.build().python().entry_points().to_owned(),
    };

    let link_json = LinkJson {
        noarch: NoArchLinks::Python(noarch_links),
        package_metadata_version: 1,
    };

    Ok(Some(serde_json::to_string_pretty(&link_json)?))
}

/// Create the python entry point script for the recipe. Overwrites any existing entry points.
fn create_entry_points(
    output: &Output,
    tmp_dir_path: &Path,
) -> Result<Vec<PathBuf>, PackagingError> {
    if output.recipe.build().python().entry_points().is_empty() {
        return Ok(Vec::new());
    }

    let target_platform = &output.build_configuration.target_platform;
    let mut new_files = Vec::new();

    let host_deps = output
        .finalized_dependencies
        .as_ref()
        .ok_or_else(|| PackagingError::DependenciesNotFinalized)?
        .host
        .clone()
        .ok_or_else(|| {
            PackagingError::CannotCreateEntryPoint("Could not find host dependencies".to_string())
        })?;

    let python_record = host_deps
        .resolved
        .iter()
        .find(|dep| dep.package_record.name.as_normalized() == "python");
    let python_version = if let Some(python_record) = python_record {
        python_record.package_record.version.version().clone()
    } else {
        return Err(PackagingError::CannotCreateEntryPoint(
            "Could not find python in host dependencies".to_string(),
        ));
    };

    for ep in output.recipe.build().python().entry_points() {
        let script = python_entry_point_template(
            &output
                .build_configuration
                .directories
                .host_prefix
                .to_string_lossy(),
            ep,
            &PythonInfo::from_version(&python_version, output.build_configuration.target_platform)
                .map_err(|e| {
                    PackagingError::CannotCreateEntryPoint(format!(
                        "Could not create python info: {}",
                        e
                    ))
                })?,
        );

        if target_platform.is_windows() {
            fs::create_dir_all(tmp_dir_path.join("Scripts"))?;

            let script_path = tmp_dir_path.join(format!("Scripts/{}-script.py", ep.command));
            let mut file = File::create(&script_path)?;
            file.write_all(script.as_bytes())?;

            // write exe launcher as well
            let exe_path = tmp_dir_path.join(format!("Scripts/{}.exe", ep.command));
            let mut exe = File::create(&exe_path)?;
            exe.write_all(get_windows_launcher(target_platform))?;

            new_files.extend(vec![script_path, exe_path]);
        } else {
            fs::create_dir_all(tmp_dir_path.join("bin"))?;

            let script_path = tmp_dir_path.join(format!("bin/{}", ep.command));
            let mut file = File::create(&script_path)?;
            file.write_all(script.as_bytes())?;

            #[cfg(target_family = "unix")]
            std::fs::set_permissions(
                &script_path,
                std::os::unix::fs::PermissionsExt::from_mode(0o775),
            )?;

            new_files.push(script_path);
        }
    }

    Ok(new_files)
}

/// This function copies the license files to the info/licenses folder.
fn copy_license_files(
    output: &Output,
    tmp_dir_path: &Path,
) -> Result<Option<Vec<PathBuf>>, PackagingError> {
    if output.recipe.about().license_files().is_empty() {
        Ok(None)
    } else {
        let license_globs = output.recipe.about().license_files();

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
                        .entry_points()
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
                        .entry_points()
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

    let dynamic_linking = output.recipe.build().dynamic_linking();
    let relocation_config = dynamic_linking
        .clone()
        .and_then(|v| v.binary_relocation())
        .unwrap_or_default();
    if output.build_configuration.target_platform != Platform::NoArch
        && !relocation_config.no_relocation()
    {
        let rpath_allowlist = match dynamic_linking {
            Some(v) => v.rpath_allowlist()?,
            None => Vec::new(),
        };
        let mut binaries = tmp_files.clone();
        if let Some(paths) = relocation_config.relocate_paths()? {
            binaries.retain(|v| paths.iter().any(|glob| glob.is_match(v)));
        }
        post_process::relink::relink(
            &binaries,
            tmp_dir_path,
            prefix,
            &output.build_configuration.target_platform,
            &dynamic_linking
                .as_ref()
                .map(|v| v.rpaths())
                .unwrap_or_default(),
            &rpath_allowlist,
        )?;

        post_process::linking_checks(output, &binaries)?;
    }

    post_process::python::python(output.name(), output.version(), &tmp_files)?;

    tracing::info!("Relink done!");

    let info_folder = tmp_dir_path.join("info");
    fs::create_dir_all(&info_folder)?;

    let mut paths_json = File::create(info_folder.join("paths.json"))?;
    let paths_json_struct = create_paths_json(&tmp_files, tmp_dir_path, prefix)?;
    paths_json.write_all(serde_json::to_string_pretty(&paths_json_struct)?.as_bytes())?;
    tmp_files.insert(info_folder.join("paths.json"));

    let mut index_json = File::create(info_folder.join("index.json"))?;
    index_json.write_all(create_index_json(output)?.as_bytes())?;
    tmp_files.insert(info_folder.join("index.json"));

    let mut hash_input_json = File::create(info_folder.join("hash_input.json"))?;
    hash_input_json.write_all(output.build_configuration.hash.hash_input.as_bytes())?;
    tmp_files.insert(info_folder.join("hash_input.json"));

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
        if let Some(link) = create_link_json(output)? {
            let mut link_json = File::create(info_folder.join("link.json"))?;
            link_json.write_all(link.as_bytes())?;
            tmp_files.insert(info_folder.join("link.json"));
        }
    } else {
        let entry_points = create_entry_points(output, tmp_dir_path)?;
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
    use super::create_prefix_placeholder;

    #[test]
    fn detect_prefix() {
        let test_data = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("test-data/binary_files/binary_file_fallback");
        let prefix = std::path::Path::new(env!("CARGO_MANIFEST_DIR"));

        create_prefix_placeholder(&test_data, prefix).unwrap();
    }
}
