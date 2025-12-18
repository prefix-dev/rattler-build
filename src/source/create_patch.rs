//! Functions to create a new patch for a given directory using `diffy`.
//! We take all files found in this directory and compare them to the original files
//! from the source cache. Any differences will be written to a patch file.

use diffy::DiffOptions;
use fs_err as fs;
use globset::{Glob, GlobSet};
use miette::Diagnostic;
use std::collections::HashMap;
use std::ffi::OsStr;
use std::io::ErrorKind;
use std::path::{Path, PathBuf};
use tempfile::TempDir;
use thiserror::Error;
use walkdir::WalkDir;

use crate::recipe::parser::Source;
use crate::source::patch::{apply_patch_custom, summarize_single_patch};
use crate::source::{SourceError, SourceInformation};

// ============================================================================
// Section 1: Error types and type definitions
// ============================================================================

/// Error type for generating patches
#[derive(Debug, Error, Diagnostic)]
pub enum GeneratePatchError {
    /// Error when the source was not
    #[error("Source error: {0}")]
    SourceError(#[from] SourceError),

    /// Error when the source information file cannot be read
    #[error("Failed to read source information: {0}")]
    SourceInfoReadError(String),

    /// Error when the patch file already exists
    #[error("Patch file already exists: {0}")]
    PatchFileAlreadyExists(PathBuf),

    /// An IO error occurred when reading or writing files
    #[error("IO error: {0}")]
    IoError(#[from] std::io::Error),

    /// Error when the path cannot be stripped of its prefix
    #[error("Failed to strip prefix from path: {0}")]
    StripPrefixError(#[from] std::path::StripPrefixError),

    /// Error in user supplied glob pattern
    #[error("Invalid glob pattern: {0}")]
    GlobPatternError(#[from] globset::Error),
}

/// Configuration for patch generation
#[derive(Debug, Clone)]
struct PatchConfig<'a> {
    name: &'a str,
    overwrite: bool,
    output_dir: Option<&'a Path>,
    dry_run: bool,
}

/// Configuration for file filtering during patch generation
#[derive(Debug)]
struct FilterConfig {
    exclude: GlobSet,
    add: GlobSet,
    include: GlobSet,
    ignored_files: Vec<&'static OsStr>,
}

impl FilterConfig {
    /// Check if a file should be skipped based on the filter configuration.
    /// Returns `Some(reason)` if the file should be skipped, `None` if it should be processed.
    fn should_skip(&self, file_path: &Path, rel_path: &Path, check_add_pattern: bool) -> bool {
        // Skip ignored files
        if self
            .ignored_files
            .iter()
            .any(|f| file_path.file_name().is_some_and(|n| n == *f))
        {
            tracing::debug!("Skipping ignored file: {}", file_path.display());
            return true;
        }

        // Skip excluded files
        if self.exclude.is_match(file_path) {
            tracing::debug!("Skipping excluded file: {}", file_path.display());
            return true;
        }

        // If include patterns are specified, skip files that don't match
        if !self.include.is_empty() {
            let matches_include =
                self.include.is_match(file_path) || self.include.is_match(rel_path);
            let matches_add =
                check_add_pattern && (self.add.is_match(file_path) || self.add.is_match(rel_path));

            if !matches_include && !matches_add {
                tracing::debug!(
                    "Skipping file (not matched by filter patterns): {}",
                    file_path.display()
                );
                return true;
            }
        }

        false
    }
}

// ============================================================================
// Section 2: Helper utilities (path formatting, glob building, etc.)
// ============================================================================

/// Convert a path to forward slash format for patch files.
/// Patch files always use forward slashes regardless of platform.
fn path_to_patch_format(path: &Path) -> String {
    path.components()
        .map(|component| component.as_os_str().to_string_lossy())
        .collect::<Vec<_>>()
        .join("/")
}

/// Determine if a file is binary using the existing content type detection.
fn is_binary_file(path: &Path) -> Result<bool, GeneratePatchError> {
    use crate::packaging::content_type;

    let content_type = content_type(path).map_err(GeneratePatchError::IoError)?;

    match content_type {
        Some(ct) => Ok(!ct.is_text()),
        None => Ok(false),
    }
}

/// Determine the directory where patches should be written.
fn get_patch_output_paths<'a>(
    output_dir: Option<&'a Path>,
    recipe_dir: &'a Path,
    name: &str,
) -> (&'a Path, PathBuf) {
    let target_dir = output_dir.unwrap_or(recipe_dir);
    let patch_file_name = format!("{}.patch", name);
    let patch_path = target_dir.join(patch_file_name);
    (target_dir, patch_path)
}

/// Handle URL source patch generation.
fn handle_url_source(
    url_src: &crate::recipe::parser::UrlSource,
    source_idx: usize,
    source_info: &SourceInformation,
    work_dir: &Path,
    cache_dir: &Path,
    config: &PatchConfig,
    filter_config: &FilterConfig,
) -> Result<String, GeneratePatchError> {
    let mut patch_content = String::new();

    // Skip single files that weren't extracted
    if url_src.file_name().is_some() {
        return Ok(patch_content);
    }

    tracing::info!("Generating patch for URL source: {}", url_src.urls()[0]);

    // Determine the original (extracted) directory
    let original_dir = if let Some(extracted_folders) = &source_info.extracted_folders {
        if let Some(Some(extracted)) = extracted_folders.get(source_idx) {
            extracted.clone()
        } else {
            find_url_cache_dir(cache_dir, url_src)?
        }
    } else {
        find_url_cache_dir(cache_dir, url_src)?
    };

    let target_dir = if let Some(target) = url_src.target_directory() {
        work_dir.join(target)
    } else {
        work_dir.to_path_buf()
    };

    // Determine the directory where patches are written
    let recipe_dir = source_info.recipe_path.parent().unwrap();
    let patch_output_dir = config.output_dir.unwrap_or(recipe_dir);

    // Filter out the patch we're currently creating/overwriting from the baseline
    let current_patch_name = PathBuf::from(format!("{}.patch", config.name));
    let existing_patches: Vec<PathBuf> = url_src
        .patches()
        .iter()
        .filter(|p| *p != &current_patch_name)
        .cloned()
        .collect();

    // Create full-directory diff, applying patches per file
    let diff = create_directory_diff(
        &original_dir,
        &target_dir,
        url_src.target_directory(),
        filter_config,
        &existing_patches,
        patch_output_dir,
    )?;

    if !diff.is_empty() {
        patch_content.push_str(&diff);
        if existing_patches.is_empty() {
            tracing::info!("Created patch for URL source: {}", url_src.urls()[0]);
        } else {
            tracing::info!("Created incremental patch ({} bytes)", diff.len());
        }
    }

    Ok(patch_content)
}

/// Handle Git source patch generation (not yet implemented).
fn handle_git_source(
    _git_src: &crate::recipe::parser::GitSource,
) -> Result<String, GeneratePatchError> {
    tracing::warn!("Generating patch for git source is not implemented yet.");
    Ok(String::new())
}

/// Handle Path source patch generation (not yet implemented).
fn handle_path_source(
    _path_src: &crate::recipe::parser::PathSource,
) -> Result<String, GeneratePatchError> {
    tracing::warn!("Generating patch for path source is not implemented yet.");
    Ok(String::new())
}

// ============================================================================
// Section 3: Main entry point (create_patch)
// ============================================================================

/// Creates a unified diff patch by comparing the current state of files in the work directory
/// against their original state from the source cache.
#[allow(clippy::too_many_arguments)]
pub fn create_patch<P: AsRef<Path>>(
    work_dir: P,
    name: &str,
    overwrite: bool,
    output_dir: Option<&Path>,
    exclude_patterns: &[String],
    add_patterns: &[String],
    include_patterns: &[String],
    dry_run: bool,
) -> Result<(), GeneratePatchError> {
    let work_dir = work_dir.as_ref();
    let source_info_path = work_dir.join(".source_info.json");

    if !source_info_path.exists() {
        return Err(GeneratePatchError::SourceInfoReadError(
            "Source information file not found".to_string(),
        ));
    }

    // Load the source information from the work directory
    let source_info: SourceInformation =
        serde_json::from_reader(fs::File::open(&source_info_path)?).map_err(|e| {
            GeneratePatchError::SourceInfoReadError(format!(
                "Failed to parse source information: {}",
                e
            ))
        })?;

    // Create configuration structs
    let config = PatchConfig {
        name,
        overwrite,
        output_dir,
        dry_run,
    };

    let filter_config = FilterConfig {
        exclude: build_globset(exclude_patterns)?,
        add: build_globset(add_patterns)?,
        include: build_globset(include_patterns)?,
        ignored_files: vec![
            OsStr::new(".source_info.json"), // Ignore the source info file itself
            OsStr::new("conda_build.sh"),    // Ignore conda build script
            OsStr::new("conda_build.bat"),   // Ignore conda build script for Windows
            OsStr::new("build_env.sh"),      // Ignore build environment script
            OsStr::new("build_env.bat"),     // Ignore build environment script for Windows
        ],
    };

    let mut updated_source_info = source_info.clone();
    let cache_dir = &source_info.source_cache;

    for (source_idx, source) in source_info.sources.iter().enumerate() {
        let patch_content = match source {
            Source::Git(git_src) => handle_git_source(git_src)?,
            Source::Url(url_src) => handle_url_source(
                url_src,
                source_idx,
                &source_info,
                work_dir,
                cache_dir,
                &config,
                &filter_config,
            )?,
            Source::Path(path_src) => handle_path_source(path_src)?,
        };

        if patch_content.is_empty() {
            tracing::info!("No changes detected for source: {:?}", source);
            let recipe_dir = source_info
                .recipe_path
                .parent()
                .expect("Recipe path should have a parent");
            let (_, patch_path) =
                get_patch_output_paths(config.output_dir, recipe_dir, config.name);
            // Even if there are no changes, check if patch file exists and warn user
            if patch_path.exists() && !config.overwrite {
                return Err(GeneratePatchError::PatchFileAlreadyExists(patch_path));
            }
            continue; // Skip if no changes were detected
        }

        // Determine directory where we should write the patch
        let recipe_dir = source_info
            .recipe_path
            .parent()
            .expect("Recipe path should have a parent");
        let (target_dir, patch_path) =
            get_patch_output_paths(config.output_dir, recipe_dir, config.name);

        if patch_path.exists() && !config.overwrite {
            return Err(GeneratePatchError::PatchFileAlreadyExists(patch_path));
        }

        if config.dry_run {
            tracing::info!(
                "[dry-run] Would create patch file at: {} ({} bytes)",
                patch_path.display(),
                patch_content.len()
            );
        } else {
            fs::create_dir_all(target_dir)?;
            fs::write(&patch_path, &patch_content)?;
            tracing::info!("Created patch file at: {}", patch_path.display());

            // Update the source information to include the newly created patch
            let patch_file_name = PathBuf::from(format!("{}.patch", config.name));
            match &mut updated_source_info.sources[source_idx] {
                Source::Url(url_src) => {
                    if !url_src.patches.contains(&patch_file_name) {
                        url_src.patches.push(patch_file_name);
                    }
                }
                Source::Git(git_src) => {
                    if !git_src.patches.contains(&patch_file_name) {
                        git_src.patches.push(patch_file_name);
                    }
                }
                Source::Path(path_src) => {
                    if !path_src.patches.contains(&patch_file_name) {
                        path_src.patches.push(patch_file_name);
                    }
                }
            }
        }
    }

    // Write updated source information back to .source_info.json if any patches were created
    // Skip if --diff or --dry-run
    if !config.dry_run {
        let source_info_path = work_dir.join(".source_info.json");
        fs::write(
            &source_info_path,
            serde_json::to_string(&updated_source_info).expect("should serialize"),
        )?;
    }

    Ok(())
}

// ============================================================================
// Section 4: Directory diffing logic
// ============================================================================

/// Validate and filter patches, logging information about which patches will be applied.
fn validate_and_filter_patches<'a>(
    existing_patches: &'a [PathBuf],
    patch_output_dir: &Path,
) -> Vec<&'a PathBuf> {
    let valid_patches: Vec<_> = existing_patches
        .iter()
        .filter(|patch| {
            let patch_path = patch_output_dir.join(patch);
            if patch_path.exists() {
                true
            } else {
                tracing::warn!("Patch file not found, skipping: {}", patch_path.display());
                false
            }
        })
        .collect();

    if !valid_patches.is_empty() {
        tracing::info!(
            "Applying {} existing patches to determine baseline:",
            valid_patches.len()
        );
        for patch in &valid_patches {
            tracing::info!("  - {}", patch.display());
        }
    }

    valid_patches
}

/// Build a map from file paths to the patches that affect them.
fn build_file_patch_map(
    valid_patches: &[&PathBuf],
    patch_output_dir: &Path,
    original_dir: &Path,
) -> Result<HashMap<PathBuf, Vec<PathBuf>>, GeneratePatchError> {
    let mut file_patch_map: HashMap<PathBuf, Vec<PathBuf>> = HashMap::new();
    for patch in valid_patches {
        let stats = summarize_single_patch(&patch_output_dir.join(patch), original_dir)
            .map_err(GeneratePatchError::SourceError)?;
        for path in stats
            .changed
            .iter()
            .chain(stats.added.iter())
            .chain(stats.removed.iter())
        {
            file_patch_map
                .entry(path.clone())
                .or_default()
                .push((*patch).clone());
        }
    }
    Ok(file_patch_map)
}

/// Process modified and new files, generating diffs for them.
fn process_modified_files(
    modified_dir: &Path,
    original_dir: &Path,
    target_subdir: Option<&PathBuf>,
    filter_config: &FilterConfig,
    file_patch_map: &HashMap<PathBuf, Vec<PathBuf>>,
    patch_output_dir: &Path,
) -> Result<String, GeneratePatchError> {
    let mut patch_content = String::new();

    for entry in WalkDir::new(modified_dir)
        .into_iter()
        .filter_map(|e| e.ok())
        .filter(|e| e.file_type().is_file())
    {
        let modified_file = entry.path();
        let rel_path = modified_file.strip_prefix(modified_dir)?;

        // Check all filter conditions (ignored, excluded, include/add patterns)
        if filter_config.should_skip(modified_file, rel_path, true) {
            continue;
        }

        let patch_path = target_subdir
            .map(|sub| sub.join(rel_path))
            .unwrap_or_else(|| rel_path.to_path_buf());

        // Check if this is a binary file using content inspection
        if is_binary_file(modified_file)? {
            tracing::info!("Skipping binary file: {}", modified_file.display());
            continue;
        }

        // Try to read as UTF-8, treat as binary if it fails
        let modified_content = match fs::read_to_string(modified_file) {
            Ok(s) => s,
            Err(e) if e.kind() == std::io::ErrorKind::InvalidData => {
                // Not valid UTF-8, treat as binary
                tracing::debug!(
                    "Skipping binary file (invalid UTF-8): {}",
                    modified_file.display()
                );
                continue;
            }
            Err(e) => return Err(GeneratePatchError::IoError(e)),
        };

        // Determine only the patches relevant to this file
        let applicable_patches = file_patch_map
            .get(rel_path)
            .map(Vec::as_slice)
            .unwrap_or(&[]);

        match apply_baseline_patches(rel_path, original_dir, applicable_patches, patch_output_dir)?
        {
            Some(original_content) => {
                // File existed in original directory - include if modified and matches include filter
                if original_content != modified_content {
                    // Check include filter if specified
                    let should_include = if filter_config.include.is_empty() {
                        // No include filter specified, include all modified files
                        true
                    } else {
                        // Include filter specified, only include files that match
                        filter_config.include.is_match(modified_file)
                            || filter_config.include.is_match(rel_path)
                    };

                    if should_include {
                        let patch = DiffOptions::default()
                            .set_original_filename(format!(
                                "a/{}",
                                path_to_patch_format(&patch_path)
                            ))
                            .set_modified_filename(format!(
                                "b/{}",
                                path_to_patch_format(&patch_path)
                            ))
                            .create_patch(&original_content, &modified_content);
                        let formatted = diffy::PatchFormatter::new().fmt_patch(&patch).to_string();
                        patch_content.push_str(&formatted);
                        tracing::info!(
                            "{}",
                            diffy::PatchFormatter::new().with_color().fmt_patch(&patch)
                        );
                    } else {
                        tracing::debug!(
                            "Skipping modified file (not matched by --include patterns): {}",
                            modified_file.display()
                        );
                    }
                }
            }
            None => {
                // Add patterns specified - only include files that match
                let should_add = filter_config.add.is_match(modified_file)
                    || filter_config.add.is_match(rel_path);

                if should_add {
                    let patch = DiffOptions::default()
                        .set_original_filename("/dev/null")
                        .set_modified_filename(format!("b/{}", path_to_patch_format(&patch_path)))
                        .create_patch("", &modified_content);
                    let formatted = diffy::PatchFormatter::new().fmt_patch(&patch).to_string();
                    patch_content.push_str(&formatted);
                    tracing::info!(
                        "New file (matched --add pattern): {}",
                        modified_file.display()
                    );
                    tracing::info!(
                        "{}",
                        diffy::PatchFormatter::new().with_color().fmt_patch(&patch)
                    );
                } else {
                    tracing::debug!(
                        "Skipping new file (not matched by --add patterns): {}",
                        modified_file.display()
                    );
                }
            }
        }
    }

    Ok(patch_content)
}

/// Process deleted files, generating deletion diffs for them.
fn process_deleted_files(
    original_dir: &Path,
    modified_dir: &Path,
    target_subdir: Option<&PathBuf>,
    filter_config: &FilterConfig,
    file_patch_map: &HashMap<PathBuf, Vec<PathBuf>>,
    patch_output_dir: &Path,
) -> Result<String, GeneratePatchError> {
    let mut patch_content = String::new();

    for entry in WalkDir::new(original_dir)
        .into_iter()
        .filter_map(|e| e.ok())
        .filter(|e| e.file_type().is_file())
    {
        let original_file = entry.path();
        let rel_path = original_file.strip_prefix(original_dir)?;

        // Check all filter conditions (ignored, excluded, include patterns)
        // Note: check_add_pattern=false since deleted files can't match --add
        if filter_config.should_skip(original_file, rel_path, false) {
            continue;
        }

        let modified_file = modified_dir.join(rel_path);
        if !modified_file.exists() {
            // Only apply patches for files that were actually touched
            let applicable_patches = file_patch_map
                .get(rel_path)
                .map(Vec::as_slice)
                .unwrap_or(&[]);
            let patch_path = target_subdir
                .map(|sub| sub.join(rel_path))
                .unwrap_or_else(|| rel_path.to_path_buf());
            if is_binary_file(original_file)? {
                tracing::warn!("Skipping binary file deletion: {}", original_file.display());
                let patch = DiffOptions::default()
                    .set_original_filename(format!("a/{}", path_to_patch_format(&patch_path)))
                    .set_modified_filename("/dev/null")
                    .create_patch("", "");
                let formatted = diffy::PatchFormatter::new().fmt_patch(&patch).to_string();
                patch_content.push_str(&formatted);
                continue;
            }
            if let Some(original_content) = apply_baseline_patches(
                rel_path,
                original_dir,
                applicable_patches,
                patch_output_dir,
            )? {
                let patch = DiffOptions::default()
                    .set_original_filename(format!("a/{}", path_to_patch_format(&patch_path)))
                    .set_modified_filename("/dev/null")
                    .create_patch(&original_content, "");
                let formatted = diffy::PatchFormatter::new().fmt_patch(&patch).to_string();
                patch_content.push_str(&formatted);
                tracing::info!(
                    "{}",
                    diffy::PatchFormatter::new().with_color().fmt_patch(&patch)
                );
            }
        }
    }

    Ok(patch_content)
}

/// Creates a unified diff between two directories, applying existing patches per file before comparison.
fn create_directory_diff(
    original_dir: &Path,
    modified_dir: &Path,
    target_subdir: Option<&PathBuf>,
    filter_config: &FilterConfig,
    existing_patches: &[PathBuf],
    patch_output_dir: &Path,
) -> Result<String, GeneratePatchError> {
    // Validate and filter patches
    let valid_patches = validate_and_filter_patches(existing_patches, patch_output_dir);

    // Build map of files to their affecting patches
    let file_patch_map = build_file_patch_map(&valid_patches, patch_output_dir, original_dir)?;

    // Process modified and new files
    let mut patch_content = process_modified_files(
        modified_dir,
        original_dir,
        target_subdir,
        filter_config,
        &file_patch_map,
        patch_output_dir,
    )?;

    // Process deleted files
    let deleted_content = process_deleted_files(
        original_dir,
        modified_dir,
        target_subdir,
        filter_config,
        &file_patch_map,
        patch_output_dir,
    )?;
    patch_content.push_str(&deleted_content);

    Ok(patch_content)
}

// ============================================================================
// Section 5: Source-specific logic and cache management
// ============================================================================

/// Find the URL cache directory for a given URL source
fn find_url_cache_dir(
    cache_dir: &Path,
    url_src: &crate::recipe::parser::UrlSource,
) -> Result<PathBuf, SourceError> {
    // This should match the logic in url_source::extracted_folder
    // You might need to recreate the cache name logic here
    use crate::source::checksum::Checksum;

    let checksum = Checksum::from_url_source(url_src)
        .ok_or_else(|| SourceError::NoChecksum("No checksum for URL source".to_string()))?;

    let first_url = url_src
        .urls()
        .first()
        .ok_or_else(|| SourceError::UnknownError("No URLs in source".to_string()))?;

    // Recreate the cache name logic from url_source.rs
    let filename = first_url
        .path_segments()
        .and_then(|segments| segments.filter(|x| !x.is_empty()).next_back())
        .ok_or_else(|| SourceError::UrlNotFile(first_url.clone()))?;

    let (stem, _) = super::url_source::split_path(Path::new(filename))
        .map_err(|e| SourceError::UnknownError(format!("Failed to split path: {}", e)))?;

    let checksum_hex = checksum.to_hex();
    let cache_name = format!("{}_{}", stem, &checksum_hex[..8]);

    let extracted_dir = cache_dir.join(cache_name);
    if extracted_dir.exists() {
        Ok(extracted_dir)
    } else {
        Err(SourceError::FileNotFound(extracted_dir))
    }
}

/// Build a GlobSet matcher from patterns, returning an empty matcher if the list is empty.
fn build_globset(patterns: &[String]) -> Result<GlobSet, GeneratePatchError> {
    use globset::GlobSetBuilder;
    let mut builder = GlobSetBuilder::new();
    for pat in patterns {
        // Add original pattern
        builder.add(Glob::new(pat)?);
        // If pattern has no path separator, also match it anywhere in the tree
        if !pat.contains('/') && !pat.contains('\\') {
            let anywhere = format!("**/{}", pat);
            builder.add(Glob::new(&anywhere)?);
        }
    }
    Ok(builder.build()?)
}

// ============================================================================
// Section 6: Patch application and baseline establishment
// ============================================================================

/// Helper to read a file optionally (returns None if not found or not valid UTF-8).
fn read_optional(path: &Path) -> Result<Option<String>, GeneratePatchError> {
    match fs::read_to_string(path) {
        Ok(s) => Ok(Some(s)),
        Err(e) if e.kind() == ErrorKind::NotFound || e.kind() == ErrorKind::InvalidData => Ok(None),
        Err(e) => Err(GeneratePatchError::IoError(e)),
    }
}

/// Setup a temporary directory with the original file for patch application.
fn setup_temp_file(
    tmp_path: &Path,
    rel_path: &Path,
    original_file: &Path,
) -> Result<(), GeneratePatchError> {
    // Create parent directory structure in temp dir (using relative path, not absolute)
    if let Some(parent) = rel_path.parent() {
        fs::create_dir_all(tmp_path.join(parent))?;
    }
    match fs::copy(original_file, tmp_path.join(rel_path)) {
        Ok(_) => Ok(()),
        Err(e) if e.kind() == ErrorKind::NotFound => Ok(()),
        Err(e) => Err(GeneratePatchError::IoError(e)),
    }
}

/// Apply baseline patches to a file and return its content after applying those patches.
/// This establishes the baseline for comparison when creating incremental patches.
fn apply_baseline_patches(
    rel_path: &Path,
    original_dir: &Path,
    existing_patches: &[PathBuf],
    patch_output_dir: &Path,
) -> Result<Option<String>, GeneratePatchError> {
    let original_file = original_dir.join(rel_path);

    // No patches to apply - just read the original file
    if existing_patches.is_empty() {
        return read_optional(&original_file);
    }

    // Create temporary directory for patch application
    let tmp_dir = TempDir::new().map_err(GeneratePatchError::IoError)?;
    let tmp_path = tmp_dir.path();

    setup_temp_file(tmp_path, rel_path, &original_file)?;

    // Apply each patch that touches this file
    for patch in existing_patches {
        let patch_path = patch_output_dir.join(patch);

        // Skip missing patches with a warning
        if !patch_path.exists() {
            tracing::debug!("Skipping missing patch file: {}", patch_path.display());
            continue;
        }

        // Check if this patch affects the current file
        let stats = summarize_single_patch(&patch_path, original_dir)
            .map_err(GeneratePatchError::SourceError)?;

        let touches_file = stats
            .changed
            .iter()
            .chain(stats.added.iter())
            .chain(stats.removed.iter())
            .any(|p| p.as_path() == rel_path);

        if touches_file {
            tracing::debug!(
                "Applying patch {} to temp file {} to establish baseline",
                patch.display(),
                rel_path.display()
            );
            apply_patch_custom(tmp_path, &patch_path).map_err(GeneratePatchError::SourceError)?;
        }
    }

    read_optional(&tmp_path.join(rel_path))
}
