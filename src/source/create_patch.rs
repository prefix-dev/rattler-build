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
use crate::source::patch::{apply_patch_custom, apply_patches, summarize_patches};
use crate::source::{SourceError, SourceInformation};

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

/// Creates a unified diff patch by comparing the current state of files in the work directory
/// against their original state from the source cache.
pub fn create_patch<P: AsRef<Path>>(
    work_dir: P,
    name: &str,
    overwrite: bool,
    output_dir: Option<&Path>,
    exclude_patterns: &[String],
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

    // Default ignored files that we never want to include in the diff.  The
    // caller can supply additional file names via `--exclude`.
    let ignored_files: Vec<&OsStr> = vec![
        OsStr::new(".source_info.json"), // Ignore the source info file itself
        OsStr::new("conda_build.sh"),    // Ignore conda build script
        OsStr::new("conda_build.bat"),   // Ignore conda build script for Windows
        OsStr::new("build_env.sh"),      // Ignore build environment script
        OsStr::new("build_env.bat"),     // Ignore build environment script for Windows
    ];

    // compile glob patterns from user exclusions
    let glob_set = build_globset(exclude_patterns)?;

    let mut updated_source_info = source_info.clone();
    let cache_dir = &source_info.source_cache;

    for (source_idx, source) in source_info.sources.iter().enumerate() {
        let mut patch_content = String::new();

        match source {
            Source::Git(_git_src) => {
                // Git sources can be diffed pretty easily so I think we can just not care about them for now
                tracing::warn!("Generating patch for git source is not implemented yet.");
            }
            Source::Url(url_src) => {
                // For URL sources, extract cache dir and apply existing patches if any
                if url_src.file_name().is_none() {
                    tracing::info!("Generating patch for URL source: {}", url_src.urls()[0]);
                    // This was extracted, so find the extracted directory
                    let original_dir = find_url_cache_dir(cache_dir, url_src)?;
                    let target_dir = if let Some(target) = url_src.target_directory() {
                        work_dir.join(target)
                    } else {
                        work_dir.to_path_buf()
                    };

                    // Determine the directory where patches are written (custom output or recipe dir)
                    let recipe_dir = source_info.recipe_path.parent().unwrap();
                    let patch_output_dir = output_dir.unwrap_or(recipe_dir);

                    let existing_patches = url_src.patches();
                    // Always do a full-directory diff, applying patches per file
                    let diff = create_directory_diff(
                        &original_dir,
                        &target_dir,
                        url_src.target_directory(),
                        &ignored_files,
                        &glob_set,
                        existing_patches,
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
                }
                // If it has a file_name, it's a single file and likely wasn't modified
            }
            Source::Path(_) => {
                // Path sources are copied from local filesystem,
                // we could compare against the original path if needed
                // For now, skip as the original is still available
                tracing::warn!("Generating patch for path source is not implemented yet.");
            }
        }

        // Determine directory where we should write the patch
        let recipe_dir = source_info
            .recipe_path
            .parent()
            .expect("Recipe path should have a parent");
        let target_dir = output_dir.unwrap_or(recipe_dir);

        let patch_file_name = format!("{}.patch", name);
        let patch_path = target_dir.join(patch_file_name);

        if patch_content.is_empty() {
            tracing::info!("No changes detected for source: {:?}", source);
            // Even if there are no changes, check if patch file exists and warn user
            if patch_path.exists() && !overwrite {
                return Err(GeneratePatchError::PatchFileAlreadyExists(patch_path));
            }
            continue; // Skip if no changes were detected
        }

        if patch_path.exists() && !overwrite {
            return Err(GeneratePatchError::PatchFileAlreadyExists(patch_path));
        }

        if dry_run {
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
            let patch_file_name = PathBuf::from(format!("{}.patch", name));
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
    if !dry_run {
        let source_info_path = work_dir.join(".source_info.json");
        fs::write(
            &source_info_path,
            serde_json::to_string(&updated_source_info).expect("should serialize"),
        )?;
    }

    Ok(())
}

/// Creates a unified diff between two directories, applying existing patches per file before comparison.
fn create_directory_diff(
    original_dir: &Path,
    modified_dir: &Path,
    target_subdir: Option<&PathBuf>,
    ignored_files: &[&OsStr],
    glob_set: &GlobSet,
    existing_patches: &[PathBuf],
    patch_output_dir: &Path,
) -> Result<String, GeneratePatchError> {
    let mut patch_content = String::new();

    // Build a map from file paths to their patches
    let mut file_patch_map: HashMap<PathBuf, Vec<PathBuf>> = HashMap::new();
    for patch in existing_patches {
        let stats = summarize_patches(&[patch.clone()], original_dir, patch_output_dir)
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
                .push(patch.clone());
        }
    }

    // Compare modified files
    for entry in WalkDir::new(modified_dir)
        .into_iter()
        .filter_map(|e| e.ok())
        .filter(|e| e.file_type().is_file())
    {
        let modified_file = entry.path();
        if ignored_files
            .iter()
            .any(|f| modified_file.file_name().is_some_and(|n| n == *f))
            || glob_set.is_match(modified_file)
        {
            tracing::debug!("Skipping ignored file: {}", modified_file.display());
            continue;
        }
        let rel_path = modified_file.strip_prefix(modified_dir)?;
        let patch_path = target_subdir
            .map(|sub| sub.join(rel_path))
            .unwrap_or_else(|| rel_path.to_path_buf());
        // Check if this is a binary file using content inspection
        if is_binary_file(modified_file)? {
            tracing::warn!("Skipping binary file: {}", modified_file.display());
            continue;
        }

        let modified_content = match fs::read_to_string(modified_file) {
            Ok(s) => s,
            Err(e) => return Err(GeneratePatchError::IoError(e)),
        };
        // Determine only the patches relevant to this file
        let applicable_patches = file_patch_map
            .get(rel_path)
            .map(Vec::as_slice)
            .unwrap_or(&[]);
        match get_patched_content_for_file(
            rel_path,
            original_dir,
            applicable_patches,
            patch_output_dir,
        )? {
            Some(original_content) => {
                if original_content != modified_content {
                    let patch = DiffOptions::default()
                        .set_original_filename(format!("a/{}", path_to_patch_format(&patch_path)))
                        .set_modified_filename(format!("b/{}", path_to_patch_format(&patch_path)))
                        .create_patch(&original_content, &modified_content);
                    let formatted = diffy::PatchFormatter::new().fmt_patch(&patch).to_string();
                    patch_content.push_str(&formatted);
                    tracing::info!(
                        "{}",
                        diffy::PatchFormatter::new().with_color().fmt_patch(&patch)
                    );
                }
            }
            None => {
                // New file
                let patch = DiffOptions::default()
                    .set_original_filename("/dev/null")
                    .set_modified_filename(format!("b/{}", path_to_patch_format(&patch_path)))
                    .create_patch("", &modified_content);
                let formatted = diffy::PatchFormatter::new().fmt_patch(&patch).to_string();
                patch_content.push_str(&formatted);
                tracing::info!(
                    "{}",
                    diffy::PatchFormatter::new().with_color().fmt_patch(&patch)
                );
            }
        }
    }

    // Handle deleted files
    for entry in WalkDir::new(original_dir)
        .into_iter()
        .filter_map(|e| e.ok())
        .filter(|e| e.file_type().is_file())
    {
        let original_file = entry.path();
        let rel_path = original_file.strip_prefix(original_dir)?;
        if ignored_files
            .iter()
            .any(|f| original_file.file_name().is_some_and(|n| n == *f))
            || glob_set.is_match(original_file)
        {
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
            if let Some(original_content) = get_patched_content_for_file(
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

/// Helper to get original content for a file after applying only the relevant patches.
fn get_patched_content_for_file(
    rel_path: &Path,
    original_dir: &Path,
    existing_patches: &[PathBuf],
    patch_output_dir: &Path,
) -> Result<Option<String>, GeneratePatchError> {
    fn read_optional(path: &Path) -> Result<Option<String>, GeneratePatchError> {
        match fs::read_to_string(path) {
            Ok(s) => Ok(Some(s)),
            Err(e) if e.kind() == ErrorKind::NotFound || e.kind() == ErrorKind::InvalidData => {
                Ok(None)
            }
            Err(e) => Err(GeneratePatchError::IoError(e)),
        }
    }

    let original_file = original_dir.join(rel_path);

    if existing_patches.is_empty() {
        return read_optional(&original_file);
    }

    let tmp_dir = TempDir::new().map_err(GeneratePatchError::IoError)?;
    let tmp_path = tmp_dir.path();

    if let Some(parent) = original_file.parent() {
        fs::create_dir_all(tmp_path.join(parent))?;
    }
    match fs::copy(&original_file, tmp_path.join(rel_path)) {
        Ok(_) => {}
        Err(e) if e.kind() == ErrorKind::NotFound => {}
        Err(e) => return Err(GeneratePatchError::IoError(e)),
    }

    for patch in existing_patches {
        let stats = summarize_patches(&[patch.clone()], original_dir, patch_output_dir)
            .map_err(GeneratePatchError::SourceError)?;

        let touched = stats
            .changed
            .iter()
            .chain(stats.added.iter())
            .chain(stats.removed.iter())
            .any(|p| p.as_path() == rel_path);

        if touched {
            apply_patches(
                &[patch.clone()],
                tmp_path,
                patch_output_dir,
                apply_patch_custom,
            )
            .map_err(GeneratePatchError::SourceError)?;
        }
    }

    read_optional(&tmp_path.join(rel_path))
}
