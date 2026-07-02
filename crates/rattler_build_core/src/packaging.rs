//! This module contains the functions to package a conda package from a given
//! output.
use std::{
    collections::{HashMap, HashSet},
    io::Write,
    path::{Component, Path, PathBuf},
};

use fs_err as fs;
use fs_err::File;
use indicatif::HumanBytes;
use metadata::clean_url;
use rattler_build_types::GlobVec;
use rattler_conda_types::{
    ChannelUrl, Platform,
    compression_level::CompressionLevel,
    package::{CondaArchiveType, FileMode, PackageFile, PathType, PathsJson},
};
use rattler_package_streaming::write::{write_conda_package, write_tar_bz2_package};
use unicode_normalization::UnicodeNormalization;

mod file_finder;
mod file_mapper;
mod metadata;
pub use file_finder::{Files, TempFiles, content_type, read_package_files_list, record_files};
pub use metadata::{contains_prefix_binary, contains_prefix_text, create_prefix_placeholder};
use tempfile::NamedTempFile;

use crate::{
    metadata::Output,
    package_test::write_test_files,
    post_process,
    source::{self, copy_dir},
    tool_configuration,
    utils::to_lexical_absolute,
};

#[allow(missing_docs)]
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

    #[error("license file path '{0}' escapes the build directories (resolved to '{1}')")]
    LicenseFileTraversal(String, PathBuf),

    #[error("Could not strip a prefix from a Path")]
    StripPrefixError(#[from] std::path::StripPrefixError),

    #[error("Found mixed Prefix placeholders in file (forward- vs backslashes)")]
    MixedPrefixPlaceholders(PathBuf),

    #[error("Could not serialize JSON: {0}")]
    SerializationError(#[from] serde_json::Error),

    #[error("Could not run walkdir: {0}")]
    WalkDirError(#[from] walkdir::Error),

    #[error("Failed to parse version {0}")]
    VersionParseError(#[from] rattler_conda_types::ParseVersionError),

    #[error(transparent)]
    RelinkError(#[from] crate::post_process::relink::RelinkError),

    #[error(transparent)]
    SourceError(#[from] source::SourceError),

    #[error("could not create python entry point: {0}")]
    CannotCreateEntryPoint(String),

    #[error("linking check error: {0}")]
    LinkingCheckError(#[from] crate::post_process::checks::LinkingCheckError),

    #[error("Failed to compile Python bytecode: {0}")]
    PythonCompileError(String),

    #[error("Failed to find content type for file: {0:?}")]
    ContentTypeNotFound(PathBuf),

    #[error("No license files were copied")]
    LicensesNotFound,

    #[error("Invalid Metadata: {0}")]
    InvalidMetadata(String),

    #[error("Invalid MenuInst schema file: {0} - {1}")]
    InvalidMenuInstSchema(PathBuf, serde_json::Error),

    #[error("Package file `{0}` listed in $RATTLER_BUILD_PACKAGE_FILES is not inside the prefix")]
    PackageFileOutsidePrefix(PathBuf),

    #[error("Package file `{0}` listed in $RATTLER_BUILD_PACKAGE_FILES does not exist")]
    PackageFileMissing(PathBuf),
}

/// Split a path into the longest leading directory prefix that contains no glob
/// metacharacters and the remaining glob pattern (components joined with `/`).
///
/// If the path contains no glob metacharacters the pattern is empty and the
/// whole path is returned as the base.
fn split_glob_base(path: &Path) -> (PathBuf, String) {
    let has_glob = |s: &str| s.contains(['*', '?', '[', ']', '{', '}']);
    let mut base = PathBuf::new();
    let mut rest: Vec<String> = Vec::new();
    for component in path.components() {
        let comp_str = component.as_os_str().to_string_lossy();
        if rest.is_empty() && !has_glob(&comp_str) {
            base.push(component);
        } else {
            rest.push(comp_str.into_owned());
        }
    }
    (base, rest.join("/"))
}

/// This function copies the license files to the info/licenses folder.
/// License files are selected from the recipe directory and the source (work) folder.
/// If the same file is found in both locations, the file from the recipe directory is used.
/// Absolute paths are also supported when `allow_absolute_license_paths` is true.
/// License paths that reference late-bound build directory variables (e.g.
/// `${{ PREFIX }}/...`) are resolved here and are always permitted.
fn copy_license_files(
    output: &Output,
    tmp_dir_path: &Path,
    allow_absolute_license_paths: bool,
) -> Result<Option<HashSet<PathBuf>>, PackagingError> {
    let about = output.recipe.about();
    let late_bound_license_files = &about.license_file_late_bound;

    // `license_file` holds the ordinary (relative/absolute) glob patterns, while
    // late-bound entries (e.g. `${{ PREFIX }}/...`) are tracked separately.
    let empty_globs = GlobVec::default();
    let license_file = about.license_file.as_ref().unwrap_or(&empty_globs);

    if license_file.is_empty() && late_bound_license_files.is_empty() {
        return Ok(None);
    }

    let licenses_folder = tmp_dir_path.join("info/licenses/");
    fs::create_dir_all(&licenses_folder)?;

    // Separate absolute paths from relative glob patterns
    let (absolute_paths, relative_globs): (Vec<_>, Vec<_>) = license_file
        .include_globs()
        .iter()
        .partition(|glob| Path::new(glob.source()).is_absolute());

    let mut copied_files = HashSet::new();
    let mut missing_globs = Vec::new();

    // Handle absolute paths directly
    for glob_with_source in &absolute_paths {
        let abs_path = Path::new(glob_with_source.source());

        // Check if absolute paths are allowed
        if !allow_absolute_license_paths {
            return Err(PackagingError::IoError(std::io::Error::new(
                std::io::ErrorKind::InvalidInput,
                format!(
                    "Absolute paths in license_file are not allowed. \
                        Use --allow-absolute-license-paths to enable. Path: {}",
                    abs_path.display()
                ),
            )));
        }

        if abs_path.exists() {
            // Get the file name to use as destination
            let file_name = abs_path.file_name().ok_or_else(|| {
                PackagingError::IoError(std::io::Error::new(
                    std::io::ErrorKind::InvalidInput,
                    format!(
                        "Invalid absolute path for license file: {}",
                        abs_path.display()
                    ),
                ))
            })?;

            let dest_path = licenses_folder.join(file_name);
            fs::copy(abs_path, &dest_path)?;
            copied_files.insert(dest_path);
        } else {
            missing_globs.push(glob_with_source.source().to_string());
        }
    }

    // Only process relative globs if there are any
    if !relative_globs.is_empty() {
        // Create a new GlobVec with only relative patterns
        let relative_globvec =
            GlobVec::from_vec(relative_globs.iter().map(|g| g.source()).collect(), None);

        let copy_dir_work = copy_dir::CopyDir::new(
            &output.build_configuration.directories.work_dir,
            &licenses_folder,
        )
        .with_globvec(&relative_globvec)
        .use_gitignore(false)
        .dereference_symlinks(true)
        .run()?;

        let copied_files_work_dir = copy_dir_work.copied_paths();
        let copy_dir_recipe = copy_dir::CopyDir::new(
            &output.build_configuration.directories.recipe_dir,
            &licenses_folder,
        )
        .with_globvec(&relative_globvec)
        .use_gitignore(false)
        .overwrite(true)
        .dereference_symlinks(true)
        .run()?;

        let copied_files_recipe_dir = copy_dir_recipe.copied_paths();

        // if a file was copied from the recipe dir, and the work dir, we should
        // issue a warning
        for file in copied_files_recipe_dir {
            if copied_files_work_dir.contains(file) {
                let warn_str = format!(
                    "License file from source directory was overwritten by license file from recipe folder ({})",
                    file.display()
                );
                tracing::warn!(warn_str);
                output.record_warning(&warn_str);
            }
        }

        // Merge copied files from work and recipe dirs
        copied_files.extend(
            copied_files_recipe_dir
                .iter()
                .chain(copied_files_work_dir)
                .map(PathBuf::from),
        );

        // Check which globs didn't match any files
        for (glob_str, match_obj) in copy_dir_work.include_globs() {
            if !match_obj.get_matched() {
                // Check if it matched in the recipe dir
                if let Some(recipe_match) = copy_dir_recipe.include_globs().get(glob_str) {
                    if !recipe_match.get_matched() {
                        missing_globs.push(glob_str.clone());
                    }
                } else {
                    missing_globs.push(glob_str.clone());
                }
            }
        }
    }

    // Handle late-bound license files (e.g. `${{ PREFIX }}/share/licenses/...`).
    // These reference build directory variables that are only known now, at
    // packaging time. Because they resolve from a restricted, controlled set of
    // variables they are always allowed (they do not require the
    // `--allow-absolute-license-paths` flag).
    if !late_bound_license_files.is_empty() {
        let directories = &output.build_configuration.directories;
        let resolve_var = |var: &str| -> Option<PathBuf> {
            match var {
                "PREFIX" => Some(directories.host_prefix.clone()),
                "BUILD_PREFIX" => Some(directories.build_prefix.clone()),
                "SRC_DIR" => Some(directories.work_dir.clone()),
                "RECIPE_DIR" => Some(directories.recipe_dir.clone()),
                "BUILD_DIR" => Some(directories.build_dir.clone()),
                _ => None,
            }
        };

        // The directories a resolved late-bound path is allowed to point into.
        // These are exactly the roots `resolve_var` can substitute.
        let license_roots = [
            &directories.host_prefix,
            &directories.build_prefix,
            &directories.work_dir,
            &directories.recipe_dir,
            &directories.build_dir,
        ];

        for late_bound in late_bound_license_files {
            let resolved = late_bound.resolve(resolve_var);

            // Late-bound license paths bypass `--allow-absolute-license-paths`
            // because they resolve from a controlled set of build directories.
            // That guarantee only holds if the resolved path stays inside one of
            // those directories: collapse any `.`/`..` components and reject the
            // path if it escapes (e.g. `${{ PREFIX }}/../../etc/passwd`).
            let normalized = to_lexical_absolute(&resolved, &directories.work_dir);
            if !license_roots
                .iter()
                .any(|root| normalized.starts_with(root))
            {
                return Err(PackagingError::LicenseFileTraversal(
                    late_bound.as_str().to_string(),
                    resolved,
                ));
            }

            // Split the resolved path into a non-glob base directory and a glob
            // remainder so we can reuse the directory-copy + glob machinery.
            let (base_dir, pattern) = split_glob_base(&resolved);

            if pattern.is_empty() {
                // A concrete path: copy the file directly.
                if resolved.is_file() {
                    let file_name = resolved.file_name().ok_or_else(|| {
                        PackagingError::IoError(std::io::Error::new(
                            std::io::ErrorKind::InvalidInput,
                            format!("Invalid license file path: {}", resolved.display()),
                        ))
                    })?;
                    let dest_path = licenses_folder.join(file_name);
                    fs::copy(&resolved, &dest_path)?;
                    copied_files.insert(dest_path);
                } else {
                    missing_globs.push(late_bound.as_str().to_string());
                }
            } else {
                // A glob pattern rooted at the resolved base directory.
                let glob_vec = GlobVec::from_vec(vec![&pattern], None);
                let copy_result = copy_dir::CopyDir::new(&base_dir, &licenses_folder)
                    .with_globvec(&glob_vec)
                    .use_gitignore(false)
                    .dereference_symlinks(true)
                    .run()?;
                let copied = copy_result.copied_paths();
                if copied.is_empty() {
                    missing_globs.push(late_bound.as_str().to_string());
                } else {
                    copied_files.extend(copied.iter().map(PathBuf::from));
                }
            }
        }
    }

    if !missing_globs.is_empty() {
        let error_str = format!(
            "The following license files were not found: {}",
            missing_globs.join(", ")
        );
        tracing::error!(error_str);
        return Err(PackagingError::LicensesNotFound);
    }

    if copied_files.is_empty() {
        Err(PackagingError::LicensesNotFound)
    } else {
        Ok(Some(copied_files))
    }
}

fn write_recipe_folder(
    output: &Output,
    tmp_dir_path: &Path,
) -> Result<Vec<PathBuf>, PackagingError> {
    let recipe_folder = tmp_dir_path.join("info/recipe/");
    let recipe_dir = &output.build_configuration.directories.recipe_dir;
    let recipe_path = &output.build_configuration.directories.recipe_path;
    let output_dir = &output.build_configuration.directories.output_dir;

    let mut copy_builder = copy_dir::CopyDir::new(recipe_dir, &recipe_folder)
        .use_gitignore(true)
        .ignore_hidden_files(true);

    // if the output dir is inside the same directory as the recipe, then we
    // need to ignore the output dir when copying
    if let Ok(ignore_output) = output_dir.strip_prefix(recipe_dir) {
        tracing::info!(
            "Ignoring output dir in recipe folder: {}",
            output_dir.to_string_lossy()
        );
        let output_dir_glob = format!("{}/**", ignore_output.to_string_lossy());
        let glob_vec = GlobVec::from_vec(vec![], Some(vec![&output_dir_glob]));
        copy_builder = copy_builder.with_globvec(&glob_vec);
    }

    let copy_result = copy_builder.run()?;

    let mut files = Vec::from(copy_result.copied_paths());

    // Make sure that the recipe file is "recipe.yaml" in `info/recipe/`
    if recipe_path.file_name() != Some("recipe.yaml".as_ref())
        && let Some(name) = recipe_path.file_name()
    {
        fs::rename(recipe_folder.join(name), recipe_folder.join("recipe.yaml"))?;
        // Update the existing entry with the new recipe file.
        if let Some(pos) = files.iter().position(|x| x == &recipe_folder.join(name)) {
            files[pos] = recipe_folder.join("recipe.yaml");
        }
    }

    // write the variant config to the appropriate file
    let variant_config_file = recipe_folder.join("variant_config.yaml");
    let mut variant_config = File::create(&variant_config_file)?;
    variant_config
        .write_all(serde_yaml::to_string(&output.build_configuration.variant)?.as_bytes())?;
    files.push(variant_config_file);

    let mut output_clean = output.clone();
    // clean URLs of any secrets or tokens
    output_clean.build_configuration.channels = output_clean
        .build_configuration
        .channels
        .iter()
        .map(clean_url)
        .map(|url| ChannelUrl::from(url.parse::<url::Url>().expect("url is valid")))
        .collect();

    // Write out the "rendered" recipe as well (the recipe with all the variables
    // replaced with their values)
    let rendered_recipe_file = recipe_folder.join("rendered_recipe.yaml");
    let mut rendered_recipe = File::create(&rendered_recipe_file)?;
    rendered_recipe.write_all(serde_yaml::to_string(&output_clean)?.as_bytes())?;
    files.push(rendered_recipe_file);

    Ok(files)
}

struct ProgressBar {
    progress_bar: indicatif::ProgressBar,
}

impl rattler_package_streaming::write::ProgressBar for ProgressBar {
    fn set_progress(&mut self, progress: u64, message: &str) {
        self.progress_bar.set_position(progress);
        self.progress_bar.set_message(message.to_string());
    }

    fn set_total(&mut self, total: u64) {
        self.progress_bar.set_length(total);
    }
}

/// Error type for path normalization operations
#[derive(Debug, thiserror::Error)]
pub enum PathNormalizationError {
    /// Error when a path component contains invalid Unicode
    #[error("Path component contains invalid Unicode: {0}")]
    InvalidUnicode(String),
}

/// Normalizes a component string for comparison.
///
/// This helper function applies Unicode normalization (NFKC) and optional case folding to a path component.
/// When case folding is applied, it's done in a way that properly handles special Unicode cases.
fn normalize_component(component_str: &str, to_lowercase: bool) -> String {
    if to_lowercase {
        let normalized = component_str.nfkc().collect::<String>();
        normalized.to_uppercase().to_lowercase()
    } else {
        component_str.nfkc().collect::<String>()
    }
}

/// Normalizes a path for case-insensitive comparison.
///
/// This function:
/// 1. Applies Unicode normalization (NFKC) to each path component
/// 2. Handles path separators consistently across platforms
/// 3. Optionally converts to lowercase for case-insensitive comparison
///
/// Returns a normalized string representation of the path.
fn normalize_path_for_comparison(
    path: &Path,
    to_lowercase: bool,
) -> Result<String, PathNormalizationError> {
    let estimated_capacity = path.as_os_str().len() * 6 / 5 + path.components().count();
    let mut normalized = String::with_capacity(estimated_capacity);

    let separator = '/';

    for c in path.components() {
        match c {
            Component::CurDir => continue,
            Component::RootDir => {
                normalized.push(separator);
            }
            Component::Prefix(_) | Component::ParentDir | Component::Normal(_) => {
                if !normalized.is_empty() && !normalized.ends_with(separator) {
                    normalized.push(separator);
                }

                let os_str = match c {
                    Component::Prefix(p) => p.as_os_str(),
                    _ => c.as_os_str(),
                };

                let component_str = os_str.to_str().ok_or_else(|| {
                    PathNormalizationError::InvalidUnicode(format!(
                        "Path component contains invalid Unicode: {}",
                        os_str.to_string_lossy()
                    ))
                })?;

                normalized.push_str(&normalize_component(component_str, to_lowercase));
            }
        }
    }

    Ok(normalized)
}

/// Finds paths that would collide on case-insensitive filesystems.
///
/// Returns groups of paths that differ only by case.
pub fn find_case_insensitive_collisions<I, P>(paths: I) -> Vec<Vec<PathBuf>>
where
    I: IntoIterator<Item = P>,
    P: AsRef<Path>,
{
    let mut lc_map: HashMap<String, Vec<PathBuf>> = HashMap::new();

    for path_ref in paths {
        let path = path_ref.as_ref();
        let case_folded = normalize_path_for_comparison(path, true).unwrap_or_else(|err| {
            tracing::warn!(
                "Failed to normalize path for comparison: {}: {}",
                path.display(),
                err
            );
            path.display().to_string().to_lowercase()
        });

        lc_map
            .entry(case_folded)
            .or_default()
            .push(path.to_path_buf());
    }

    let mut result: Vec<Vec<PathBuf>> = lc_map
        .into_values()
        .filter(|group| group.len() > 1)
        .map(|group| {
            let mut unique_paths: Vec<PathBuf> = group
                .into_iter()
                .collect::<HashSet<_>>()
                .into_iter()
                .collect();
            unique_paths.sort_by(|a, b| a.as_os_str().cmp(b.as_os_str()));
            unique_paths
        })
        .collect();

    result.sort_by(|a, b| a[0].as_os_str().cmp(b[0].as_os_str()));
    result
}

/// Print enhanced file listing with sizes, symlink targets, and warnings.
fn print_enhanced_file_listing(
    files: &[&Path],
    tmp: &TempFiles,
    output: &Output,
) -> Result<(), PackagingError> {
    use crate::post_process::path_checks::perform_path_checks;
    use rattler_conda_types::package::PathsEntry;

    let normalize_path = |p: &Path| -> String { p.display().to_string().replace('\\', "/") };

    // Read paths.json which contains all the file metadata including prefix placeholder info
    let paths_json_path = tmp.temp_dir.path().join("info").join("paths.json");
    let paths_json = PathsJson::from_path(&paths_json_path).ok();

    // Build a map from relative path to PathsEntry for quick lookup
    let paths_map: HashMap<&Path, &PathsEntry> = paths_json
        .as_ref()
        .map(|pj| {
            pj.paths
                .iter()
                .map(|e| (e.relative_path.as_path(), e))
                .collect()
        })
        .unwrap_or_default();

    // Run path checks on paths.json entries (content files only - info files are generated)
    if let Some(ref pj) = paths_json {
        perform_path_checks(output, &pj.paths);
    }

    // Collect per-file warnings for content files
    let mut path_warnings: HashMap<&Path, Vec<String>> = HashMap::new();
    if let Some(ref pj) = paths_json {
        for entry in &pj.paths {
            let mut warnings = Vec::new();
            let path = entry.relative_path.as_path();

            if let Some(path_str) = path.to_str() {
                if !path_str.is_ascii() {
                    warnings.push("Contains non-ASCII characters".to_string());
                }
                if path_str.contains(' ') {
                    warnings.push("Contains spaces".to_string());
                }
                if path_str.len() > 200 {
                    warnings.push(format!("Path too long ({} > 200)", path_str.len()));
                }
            }

            if !warnings.is_empty() {
                path_warnings.insert(path, warnings);
            }
        }
    }

    // Helper to check if a file is executable
    #[cfg(unix)]
    fn is_executable(path: &Path) -> bool {
        use std::os::unix::fs::PermissionsExt;
        if let Ok(metadata) = fs::metadata(path) {
            metadata.permissions().mode() & 0o111 != 0
        } else {
            false
        }
    }

    #[cfg(not(unix))]
    fn is_executable(path: &Path) -> bool {
        // On Windows, check for common executable file extensions
        const EXECUTABLE_EXTENSIONS: &[&str] = &["exe", "bat", "cmd", "com", "ps1"];
        path.extension()
            .and_then(|ext| ext.to_str())
            .is_some_and(|ext| EXECUTABLE_EXTENSIONS.contains(&ext.to_lowercase().as_str()))
    }

    // Print each file with enhanced information
    let mut total_size: u64 = 0;
    for (index, file) in files.iter().enumerate() {
        let full_path = tmp.temp_dir.path().join(file);
        let is_info = file.components().next() == Some(Component::Normal("info".as_ref()));
        let normalized_path = normalize_path(file);
        let is_last = index == files.len() - 1;

        // Look up entry from paths.json if available
        let entry = paths_map.get(*file);

        let is_symlink = entry
            .map(|e| e.path_type == PathType::SoftLink)
            .unwrap_or_else(|| full_path.is_symlink());
        let is_dir = entry
            .map(|e| e.path_type == PathType::Directory)
            .unwrap_or_else(|| full_path.is_dir());
        let is_exec = !is_symlink && !is_dir && is_executable(&full_path);

        // Get file size from entry or filesystem
        let size = if is_symlink || is_dir {
            None
        } else {
            entry
                .and_then(|e| e.size_in_bytes)
                .or_else(|| fs::metadata(&full_path).ok().map(|m| m.len()))
        };

        if let Some(s) = size {
            total_size += s;
        }

        let size_info = if is_symlink {
            String::new()
        } else if is_dir {
            " (dir)".to_string()
        } else if let Some(s) = size {
            format!(" ({})", HumanBytes(s))
        } else {
            String::new()
        };

        // Check if it's a symlink and get target
        let symlink_info = if is_symlink {
            match fs::read_link(&full_path) {
                Ok(target) => {
                    let target_str = target.display().to_string();
                    format!(" -> {}", console::style(target_str).cyan())
                }
                Err(_) => " -> <invalid symlink>".to_string(),
            }
        } else {
            String::new()
        };

        // Check if file has prefix placeholder (from the entry)
        let prefix_info = entry
            .and_then(|e| e.prefix_placeholder.as_ref())
            .map(|placeholder| {
                let mode_str = match placeholder.file_mode {
                    FileMode::Binary => "bin",
                    FileMode::Text => "text",
                };
                format!(
                    " {}",
                    console::style(format!("[prefix:{}]", mode_str)).yellow()
                )
            })
            .unwrap_or_default();

        // Choose the appropriate tree character
        let tree_char = if is_last { "└─" } else { "├─" };

        // Format the main file entry with appropriate styling
        let file_entry = if is_info {
            format!(
                "  {} {}{}{}",
                tree_char,
                console::style(&normalized_path).dim(),
                console::style(&size_info).dim(),
                symlink_info
            )
        } else if is_symlink {
            format!(
                "  {} {}{}",
                tree_char,
                console::style(&normalized_path).magenta(),
                symlink_info
            )
        } else if is_exec {
            format!(
                "  {} {}{}{}",
                tree_char,
                console::style(&normalized_path).green(),
                console::style(&size_info).dim(),
                prefix_info,
            )
        } else {
            format!(
                "  {} {}{}{}{}",
                tree_char,
                normalized_path,
                console::style(&size_info).dim(),
                prefix_info,
                symlink_info
            )
        };

        tracing::info!("{}", file_entry);

        // Print warnings for this file
        if let Some(warnings) = path_warnings.get(*file) {
            for warning in warnings {
                tracing::warn!("       └─ {}", console::style(warning).yellow());
            }
        }
    }

    // Print package statistics
    let file_count = files.len();
    let non_info_count = files
        .iter()
        .filter(|f| f.components().next() != Some(Component::Normal("info".as_ref())))
        .count();

    tracing::info!("\n");
    tracing::info!(
        "Package statistics: {} files ({} content, {} metadata), total size: {}",
        file_count,
        non_info_count,
        file_count - non_info_count,
        HumanBytes(total_size)
    );

    // Show largest files (top 5) from paths.json
    if let Some(ref pj) = paths_json {
        let mut files_with_sizes: Vec<_> = pj
            .paths
            .iter()
            .filter(|e| e.size_in_bytes.is_some() && e.size_in_bytes.unwrap() > 0)
            .collect();
        files_with_sizes.sort_by_key(|b| std::cmp::Reverse(b.size_in_bytes));

        if !files_with_sizes.is_empty() {
            tracing::info!("Largest files:");
            for entry in files_with_sizes.iter().take(5) {
                let size = entry.size_in_bytes.unwrap_or(0);
                let path_str = entry.relative_path.display().to_string().replace('\\', "/");
                tracing::info!(
                    "  {} - {}",
                    console::style(HumanBytes(size)).cyan(),
                    path_str
                );
            }
        }
    }

    Ok(())
}

/// Given an output and a set of new files, create a conda package.
/// This function will copy all the files to a temporary directory and then
/// create a conda package from that. Note that the output needs to have its
/// dependencies finalized before calling this function.
///
/// The `local_channel_dir` is the path to the local channel / output directory.
pub fn package_conda(
    output: &Output,
    tool_configuration: &tool_configuration::Configuration,
    files: &Files,
) -> Result<(PathBuf, PathsJson), PackagingError> {
    let local_channel_dir = &output.build_configuration.directories.output_dir;
    let packaging_settings = &output.build_configuration.packaging_settings;

    if output.finalized_dependencies.is_none() {
        return Err(PackagingError::DependenciesNotFinalized);
    }

    let mut tmp = files.to_temp_folder(output)?;

    tracing::info!("Copying done!");

    post_process::relink::relink(&tmp, output)?;

    post_process::menuinst::menuinst(&tmp)?;

    tmp.add_files(post_process::python::python(&tmp, output)?);

    post_process::regex_replacements::regex_post_process(&tmp, output)?;

    tracing::info!("Post-processing done!");

    // Validate any dsolist JSON files being packaged (CEP-28)
    post_process::checks::validate_dsolist_files(tmp.temp_dir.path())?;

    let info_folder = tmp.temp_dir.path().join("info");

    tracing::info!("Writing test files");
    let test_files = write_test_files(output, tmp.temp_dir.path())?;
    tmp.add_files(test_files);

    tracing::info!("Writing metadata for package");
    tmp.add_files(output.write_metadata(&tmp)?);

    // TODO move things below also to metadata.rs
    tracing::info!("Copying license files");
    if let Some(license_files) = copy_license_files(
        output,
        tmp.temp_dir.path(),
        tool_configuration.allow_absolute_license_paths,
    )? {
        tmp.add_files(license_files);
    }

    tracing::info!("Copying recipe files");
    if output.build_configuration.store_recipe {
        let recipe_files = write_recipe_folder(output, tmp.temp_dir.path())?;
        tmp.add_files(recipe_files);
    }

    // create any entry points or link.json for noarch packages
    if output.is_python_version_independent() {
        tracing::info!("Creating entry points");
        let link_json = File::create(info_folder.join("link.json"))?;
        serde_json::to_writer_pretty(link_json, &output.link_json()?)?;
        tmp.add_files(vec![info_folder.join("link.json")]);
    }

    // print sorted files
    tracing::info!("\nFiles in package:\n");
    let mut files = tmp
        .files
        .iter()
        .map(|x| x.strip_prefix(tmp.temp_dir.path()))
        .collect::<Result<Vec<_>, _>>()?;
    files.sort_by(|a, b| {
        let a_is_info = a.components().next() == Some(Component::Normal("info".as_ref()));
        let b_is_info = b.components().next() == Some(Component::Normal("info".as_ref()));
        match (a_is_info, b_is_info) {
            (true, true) | (false, false) => a.cmp(b),
            (true, false) => std::cmp::Ordering::Greater,
            (false, true) => std::cmp::Ordering::Less,
        }
    });

    for group in find_case_insensitive_collisions(&files) {
        let list = group
            .iter()
            .map(|p| p.display().to_string())
            .collect::<Vec<_>>()
            .join("\n  - ");
        let warn_str = format!(
            "Mixed-case filenames detected, case-insensitive filesystems may break:\n  - {}",
            list
        );
        tracing::error!(warn_str);
        output.record_warning(&warn_str);
    }

    print_enhanced_file_listing(&files, &tmp, output)?;

    let output_folder =
        local_channel_dir.join(output.build_configuration.target_platform.to_string());
    tracing::info!("Creating target folder '{}'", output_folder.display());

    fs::create_dir_all(&output_folder)?;

    if let Platform::NoArch = output.build_configuration.target_platform {
        create_empty_build_folder(
            local_channel_dir,
            &output.build_configuration.build_platform.platform,
        )?;
    }

    let identifier = output.identifier();
    let tempfile_in_output = NamedTempFile::new_in(&output_folder)?;

    let final_name = output_folder.join(format!(
        "{}{}",
        identifier,
        packaging_settings.archive_type.extension()
    ));

    tracing::info!("Compressing archive...");

    let progress_bar = tool_configuration.fancy_log_handler.add_progress_bar(
        indicatif::ProgressBar::new(0)
            .with_prefix("Compressing ")
            .with_style(tool_configuration.fancy_log_handler.default_bytes_style()),
    );

    match packaging_settings.archive_type {
        CondaArchiveType::TarBz2 => {
            write_tar_bz2_package(
                tempfile_in_output.as_file(),
                tmp.temp_dir.path(),
                &tmp.files.iter().cloned().collect::<Vec<_>>(),
                CompressionLevel::Numeric(packaging_settings.compression_level),
                Some(&output.build_configuration.timestamp),
                Some(Box::new(ProgressBar { progress_bar })),
            )?;
        }
        CondaArchiveType::Conda => {
            write_conda_package(
                tempfile_in_output.as_file(),
                tmp.temp_dir.path(),
                &tmp.files.iter().cloned().collect::<Vec<_>>(),
                CompressionLevel::Numeric(packaging_settings.compression_level),
                tool_configuration.compression_threads,
                &identifier,
                Some(&output.build_configuration.timestamp),
                Some(Box::new(ProgressBar { progress_bar })),
            )?;
        }
    }

    // Atomically move the file to the final location
    tempfile_in_output
        .persist(&final_name)
        .map_err(|e| e.error)?;
    tracing::info!("Archive written to '{}'", final_name.display());

    let paths_json = PathsJson::from_path(info_folder.join("paths.json"))?;
    Ok((final_name, paths_json))
}

/// When building package for noarch, we don't create another build-platform
/// folder together with noarch but conda-build does
/// because of this we have a failure in conda-smithy CI so we also *mimic* this
/// behaviour until this behaviour is changed
/// https://github.com/conda-forge/conda-forge-ci-setup-feedstock/blob/main/recipe/conda_forge_ci_setup/feedstock_outputs.py#L164
fn create_empty_build_folder(
    local_channel_dir: &Path,
    build_platform: &Platform,
) -> miette::Result<(), PackagingError> {
    let build_output_folder = local_channel_dir.join(build_platform.to_string());

    tracing::info!("Creating empty build folder {:?}", build_output_folder);

    fs::create_dir_all(&build_output_folder)?;

    Ok(())
}

impl Output {
    /// Create a conda package from any new files in the host prefix. Note: the
    /// previous stages should have been completed before calling this
    /// function.
    pub async fn create_package(
        &self,
        tool_configuration: &tool_configuration::Configuration,
        post_install_files: Option<&HashSet<PathBuf>>,
    ) -> Result<(PathBuf, PathsJson), PackagingError> {
        let span = tracing::info_span!("Packaging new files");
        let _enter = span.enter();

        let host_prefix = &self.build_configuration.directories.host_prefix;
        let package_files_list = self
            .build_configuration
            .directories
            .package_files_list_path();

        let files_after = match read_package_files_list(&package_files_list)? {
            Some(paths) => {
                tracing::info!(
                    "Using {} explicit package file(s) from {}",
                    paths.len(),
                    package_files_list.display()
                );
                Files::from_paths(
                    host_prefix,
                    paths,
                    &self.recipe.build().always_include_files,
                    &self.recipe.build().files,
                )?
            }
            None => Files::from_prefix(
                host_prefix,
                &self.recipe.build().always_include_files,
                &self.recipe.build().files,
                post_install_files,
            )?,
        };

        package_conda(self, tool_configuration, &files_after)
    }
}

#[cfg(test)]
mod packaging_tests {
    use super::*;
    use std::path::Path;

    #[cfg(unix)]
    use std::ffi::OsStr;
    #[cfg(unix)]
    use std::os::unix::ffi::OsStrExt;
    #[cfg(windows)]
    use std::os::windows::ffi::OsStringExt;

    #[test]
    fn test_split_glob_base() {
        // No glob metacharacters: whole path is the base.
        let (base, pattern) = split_glob_base(Path::new("/opt/conda/lib/LICENSE"));
        assert_eq!(base, PathBuf::from("/opt/conda/lib/LICENSE"));
        assert_eq!(pattern, "");

        // A glob component splits the base from the relative pattern.
        let (base, pattern) = split_glob_base(Path::new("/opt/conda/share/licenses/*/LICENSE"));
        assert_eq!(base, PathBuf::from("/opt/conda/share/licenses"));
        assert_eq!(pattern, "*/LICENSE");
    }

    #[test]
    fn test_find_case_insensitive_collisions_detects() {
        let files = vec![
            Path::new("foo/BAR"),
            Path::new("foo/bar"),
            Path::new("foo/Baz"),
            Path::new("foo/qux"),
        ];
        let groups = find_case_insensitive_collisions(&files);
        assert_eq!(groups.len(), 1);

        let paths_in_group: Vec<String> =
            groups[0].iter().map(|p| p.display().to_string()).collect();

        assert!(paths_in_group.contains(&"foo/BAR".to_string()));
        assert!(paths_in_group.contains(&"foo/bar".to_string()));
    }

    #[test]
    fn test_find_case_insensitive_collisions_empty() {
        let files = vec![Path::new("foo/bar"), Path::new("foo/baz")];
        let groups = find_case_insensitive_collisions(&files);
        assert!(groups.is_empty());
    }

    #[test]
    fn test_find_case_insensitive_collisions_unicode() {
        let files = vec![
            Path::new("foo/straße"),
            Path::new("foo/STRASSE"),
            Path::new("foo/other"),
        ];
        let groups = find_case_insensitive_collisions(&files);
        assert_eq!(groups.len(), 1);
    }

    #[test]
    fn test_slash_collision() {
        let files = vec![
            Path::new("path/to/text/file.py"),
            Path::new("path/to/textfile.py"),
        ];
        let groups = find_case_insensitive_collisions(&files);
        assert_eq!(groups.len(), 0);
    }

    #[test]
    fn test_normalize_path_for_comparison_basic() {
        let path = Path::new("foo/bar/baz.txt");
        let normalized = normalize_path_for_comparison(path, false).unwrap();
        assert_eq!(normalized, "foo/bar/baz.txt");
    }

    #[test]
    fn test_normalize_path_for_comparison_lowercase() {
        let path = Path::new("Foo/BAR/Baz.TXT");
        let normalized = normalize_path_for_comparison(path, true).unwrap();
        assert_eq!(normalized, "foo/bar/baz.txt");
    }

    #[test]
    fn test_normalize_path_for_comparison_unicode() {
        let path = Path::new("straße/café");
        let normalized_case_sensitive = normalize_path_for_comparison(path, false).unwrap();
        let normalized_case_insensitive = normalize_path_for_comparison(path, true).unwrap();

        assert_eq!(normalized_case_sensitive, "straße/café");
        assert_eq!(normalized_case_insensitive, "strasse/café");
    }

    #[test]
    fn test_normalize_path_for_comparison_unicode_equivalence() {
        // Test that different Unicode representations normalize to the same result
        let path1 = Path::new("café"); // é as single character
        let path2 = Path::new("cafe\u{0301}"); // e + combining acute accent

        let norm1 = normalize_path_for_comparison(path1, false).unwrap();
        let norm2 = normalize_path_for_comparison(path2, false).unwrap();

        assert_eq!(norm1, norm2);
    }

    #[test]
    fn test_normalize_path_for_comparison_empty_path() {
        let path = Path::new("");
        let normalized = normalize_path_for_comparison(path, false).unwrap();
        assert_eq!(normalized, "");
    }

    #[test]
    fn test_normalize_path_for_comparison_single_component() {
        let path = Path::new("file.txt");
        let normalized = normalize_path_for_comparison(path, false).unwrap();
        assert_eq!(normalized, "file.txt");
    }

    #[test]
    fn test_normalize_path_for_comparison_current_dir() {
        let path = Path::new("./foo/bar");
        let normalized = normalize_path_for_comparison(path, false).unwrap();
        // Current directory components should be skipped
        assert_eq!(normalized, "foo/bar");
    }

    #[test]
    fn test_normalize_path_for_comparison_parent_dir() {
        let path = Path::new("../foo/bar");
        let normalized = normalize_path_for_comparison(path, false).unwrap();
        assert_eq!(normalized, "../foo/bar");
    }

    #[test]
    fn test_normalize_path_for_comparison_absolute_path() {
        let path = Path::new("/foo/bar/baz");
        let normalized = normalize_path_for_comparison(path, false).unwrap();
        assert_eq!(normalized, "/foo/bar/baz");
    }

    #[test]
    fn test_normalize_path_for_comparison_with_separators() {
        let path = Path::new("foo//bar///baz");
        let normalized = normalize_path_for_comparison(path, false).unwrap();
        // Multiple separators should be normalized to single separators
        assert_eq!(normalized, "foo/bar/baz");
    }

    #[test]
    fn test_normalize_path_for_comparison_trailing_separator() {
        let path = Path::new("foo/bar/");
        let normalized = normalize_path_for_comparison(path, false).unwrap();
        assert_eq!(normalized, "foo/bar");
    }

    #[test]
    fn test_normalize_path_for_comparison_case_folding_special_chars() {
        // Test German ß -> SS conversion in case folding
        let path = Path::new("straße");
        let normalized = normalize_path_for_comparison(path, true).unwrap();
        assert_eq!(normalized, "strasse");
    }

    #[test]
    fn test_normalize_path_for_comparison_complex_path() {
        let path = Path::new("./Foo/../Bar/./Baz.TXT");
        let normalized_case_sensitive = normalize_path_for_comparison(path, false).unwrap();
        let normalized_case_insensitive = normalize_path_for_comparison(path, true).unwrap();

        // Current dir components (.) are skipped, but parent dir (..) and other components are preserved
        assert_eq!(normalized_case_sensitive, "Foo/../Bar/Baz.TXT");
        assert_eq!(normalized_case_insensitive, "foo/../bar/baz.txt");
    }

    #[cfg(windows)]
    #[test]
    fn test_normalize_path_for_comparison_windows_prefix() {
        let path = Path::new("C:\\foo\\bar");
        let normalized = normalize_path_for_comparison(path, false).unwrap();
        // On Windows, backslashes should be normalized to forward slashes
        assert_eq!(normalized, "C:/foo/bar");
    }

    #[test]
    fn test_normalize_path_for_comparison_preserves_path_structure() {
        // Ensure that the original failing test case works correctly
        let path1 = Path::new("path/to/text/file.py");
        let path2 = Path::new("path/to/textfile.py");

        let norm1 = normalize_path_for_comparison(path1, true).unwrap();
        let norm2 = normalize_path_for_comparison(path2, true).unwrap();

        assert_ne!(norm1, norm2);
        assert_eq!(norm1, "path/to/text/file.py");
        assert_eq!(norm2, "path/to/textfile.py");
    }

    #[cfg(windows)]
    #[test]
    fn test_normalize_path_for_comparison_invalid_unicode_windows() {
        // Invalid UTF-16 sequence
        let invalid_utf16: &[u16] = &[0x0043, 0x003A, 0xD800, 0x005C];
        let os_string = std::ffi::OsString::from_wide(invalid_utf16);
        let path = Path::new(&os_string);

        let result = normalize_path_for_comparison(path, false);
        assert!(matches!(
            result,
            Err(PathNormalizationError::InvalidUnicode(_))
        ));
    }

    #[cfg(unix)]
    #[test]
    fn test_normalize_path_for_comparison_invalid_unicode_unix() {
        // Invalid UTF-8 sequence
        let invalid_utf8: &[u8] = &[0x66, 0x6f, 0x80, 0x6f];
        let path = Path::new(OsStr::from_bytes(invalid_utf8));

        let result = normalize_path_for_comparison(path, false);
        assert!(matches!(
            result,
            Err(PathNormalizationError::InvalidUnicode(_))
        ));
    }

    #[test]
    fn test_normalize_path_for_comparison_turkish_i() {
        // Test Turkish 'İ' (I with dot) case folding
        let path = Path::new("İstanbul");
        let normalized = normalize_path_for_comparison(path, true).unwrap();
        // Note: According to Unicode case-folding rules, `İ` (U+0130) maps to
        // `i` followed by COMBINING DOT ABOVE (U+0307). Therefore the
        // normalized representation contains this combining mark.
        assert_eq!(normalized, "i\u{0307}stanbul");
    }

    #[cfg(windows)]
    #[test]
    fn test_normalize_path_for_comparison_mixed_separators() {
        let path = Path::new(r"foo/bar\baz");
        let normalized = normalize_path_for_comparison(path, false).unwrap();
        assert_eq!(normalized, "foo/bar/baz");
    }
}
