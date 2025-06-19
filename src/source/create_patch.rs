//! Functions to create a new patch for a given directory using `diffy`.
//! We take all files found in this directory and compare them to the original files
//! from the source cache. Any differences will be written to a patch file.

use diffy::DiffOptions;
use fs_err as fs;
use globset::{Glob, GlobSet};
use std::ffi::OsStr;
use std::path::{Path, PathBuf};
use thiserror::Error;
use walkdir::WalkDir;

use crate::recipe::parser::Source;
use crate::source::{SourceError, SourceInformation};

/// Error type for generating patches
#[derive(Debug, Error)]
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

    let cache_dir = source_info.source_cache;

    for source in &source_info.sources {
        let mut patch_content = String::new();

        match source {
            Source::Git(_git_src) => {
                // Git sources can be diffed pretty easily so I think we can just not care about them for now
                tracing::warn!("Generating patch for git source is not implemented yet.");
            }
            Source::Url(url_src) => {
                // For URL sources, we need to find the extracted cache directory
                if url_src.file_name().is_none() {
                    tracing::info!("Generating patch for URL source: {}", url_src.urls()[0]);
                    // This was extracted, so find the extracted directory
                    let original_dir = find_url_cache_dir(&cache_dir, url_src)?;
                    let target_dir = if let Some(target) = url_src.target_directory() {
                        work_dir.join(target)
                    } else {
                        work_dir.to_path_buf()
                    };

                    let diff = create_directory_diff(
                        &original_dir,
                        &target_dir,
                        url_src.target_directory(),
                        &ignored_files,
                        &glob_set,
                    )?;

                    if !diff.is_empty() {
                        patch_content.push_str(&diff);
                    }

                    // Parse the patch content with diffy and print it colored
                    if !diff.is_empty() {
                        tracing::info!("Created patch for URL source: {}", url_src.urls()[0]);
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

        if patch_content.is_empty() {
            tracing::info!("No changes detected for source: {:?}", source);
            continue; // Skip if no changes were detected
        }

        // Determine directory where we should write the patch
        let recipe_dir = source_info
            .recipe_path
            .parent()
            .expect("Recipe path should have a parent");
        let target_dir = output_dir.unwrap_or(recipe_dir);

        let patch_file_name = format!("{}.patch", name);
        let patch_path = target_dir.join(patch_file_name);

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
            fs::write(&patch_path, patch_content)?;
            tracing::info!("Created patch file at: {}", patch_path.display());
        }
    }

    Ok(())
}

/// Creates a unified diff between two directories
fn create_directory_diff(
    original_dir: &Path,
    modified_dir: &Path,
    target_subdir: Option<&PathBuf>,
    ignored_files: &[&OsStr],
    glob_set: &GlobSet,
) -> Result<String, GeneratePatchError> {
    let mut patch_content = String::new();

    // Walk through all files in the modified directory
    for entry in WalkDir::new(modified_dir)
        .into_iter()
        .filter_map(|e| e.ok())
        .filter(|e| e.file_type().is_file())
    {
        let modified_file = entry.path();

        if ignored_files
            .iter()
            .any(|f| modified_file.file_name().is_some_and(|name| &name == f))
            || glob_set.is_match(modified_file)
        {
            // Skip ignored files
            tracing::debug!("Skipping ignored file: {}", modified_file.display());
            continue;
        }

        // Calculate the relative path from the modified directory
        let rel_path = modified_file.strip_prefix(modified_dir)?;

        // Find the corresponding original file
        let original_file = original_dir.join(rel_path);

        // Create the patch path (for display in the patch)
        let patch_path = if let Some(subdir) = target_subdir {
            PathBuf::from(subdir).join(rel_path)
        } else {
            rel_path.to_path_buf()
        };

        if original_file.exists() {
            // Compare existing files, first by modification time and size
            let modified_metadata = fs::metadata(modified_file)?;
            let original_metadata = fs::metadata(&original_file)?;
            if modified_metadata.modified().is_err()
                || original_metadata.modified().is_err()
                || modified_metadata.len() != original_metadata.len()
            {
                // If the file has been modified, create a patch
                tracing::debug!(
                    "File changed: {} -> {}",
                    original_file.display(),
                    modified_file.display()
                );
            } else {
                // If the file hasn't changed, skip it
                continue;
            }

            let original_content = fs::read_to_string(&original_file)?;
            let modified_content = fs::read_to_string(modified_file)?;

            if original_content != modified_content {
                let patch = DiffOptions::default()
                    .set_original_filename(format!("a/{}", patch_path.display()))
                    .set_modified_filename(format!("b/{}", patch_path.display()))
                    .create_patch(&original_content, &modified_content);

                patch_content.push_str(&format!(
                    "{}",
                    diffy::PatchFormatter::new().fmt_patch(&patch)
                ));
            }
        } else {
            // This is a new file
            let modified_content = fs::read_to_string(modified_file).map_err(|_| {
                SourceError::UnknownError(format!(
                    "Failed to read new file: {}",
                    modified_file.display()
                ))
            })?;

            let patch = DiffOptions::default()
                .set_original_filename("/dev/null")
                .set_modified_filename(format!("b/{}", patch_path.display()))
                .create_patch("", &modified_content);

            patch_content.push_str(&format!(
                "{}",
                diffy::PatchFormatter::new().fmt_patch(&patch)
            ));
        }
    }

    // Check for deleted files (files that exist in original but not in modified)
    for entry in WalkDir::new(original_dir)
        .into_iter()
        .filter_map(|e| e.ok())
        .filter(|e| e.file_type().is_file())
    {
        let original_file = entry.path();
        let rel_path = original_file.strip_prefix(original_dir)?;
        let modified_file = modified_dir.join(rel_path);

        if !modified_file.exists() {
            if glob_set.is_match(original_file) {
                continue;
            }

            let patch_path = if let Some(subdir) = target_subdir {
                PathBuf::from(subdir).join(rel_path)
            } else {
                rel_path.to_path_buf()
            };

            let original_content = fs::read_to_string(original_file).map_err(|_| {
                SourceError::UnknownError(format!(
                    "Failed to read deleted file: {}",
                    original_file.display()
                ))
            })?;

            let patch = DiffOptions::default()
                .set_original_filename(format!("a/{}", patch_path.display()))
                .set_modified_filename("/dev/null")
                .create_patch(&original_content, "");

            patch_content.push_str(&format!(
                "{}",
                diffy::PatchFormatter::new().fmt_patch(&patch)
            ));
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
