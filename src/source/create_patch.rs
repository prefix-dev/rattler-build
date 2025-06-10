//! Functions to create a new patch for a given directory using `diffy`.
//! We take all files found in this directory and compare them to the original files
//! from the source cache. Any differences will be written to a patch file.

use fs_err as fs;
use std::io::Write;
use std::path::{Path, PathBuf};
use walkdir::WalkDir;

use crate::recipe::parser::Source;
use crate::source::{SourceError, SourceInformation};

/// Creates a unified diff patch by comparing the current state of files in the work directory
/// against their original state from the source cache.
pub fn create_patch<P: AsRef<Path>>(work_dir: P) -> Result<(), SourceError> {
    let work_dir = work_dir.as_ref();
    let source_info_path = work_dir.join(".source_info.json");

    if !source_info_path.exists() {
        return Err(SourceError::FileNotFound(source_info_path));
    }

    // Load the source information from the work directory
    let source_info: SourceInformation =
        serde_json::from_reader(fs::File::open(&source_info_path)?).map_err(|e| {
            SourceError::UnknownError(format!("Failed to read source information: {}", e))
        })?;

    let patch_path = work_dir.join("changes.patch");
    let mut patch_content = String::new();

    // Get the cache directory (assuming it's relative to the recipe directory)
    // let recipe_dir = source_info.recipe_path.parent()
    //     .ok_or_else(|| SourceError::UnknownError("Invalid recipe path".to_string()))?;
    // let cache_dir = recipe_dir.join("../output/src_cache"); // Adjust this path as needed

    let cache_dir = source_info.source_cache.clone();

    for source in &source_info.sources {
        match source {
            Source::Git(git_src) => {
                let original_dir = find_git_cache_dir(&cache_dir, git_src)?;
                let target_dir = if let Some(target) = git_src.target_directory() {
                    work_dir.join(target)
                } else {
                    work_dir.to_path_buf()
                };

                let diff =
                    create_directory_diff(&original_dir, &target_dir, git_src.target_directory())?;
                if !diff.is_empty() {
                    patch_content.push_str(&diff);
                }
            }
            Source::Url(url_src) => {
                // For URL sources, we need to find the extracted cache directory
                println!("Processing URL source: {:?}", url_src);
                if url_src.file_name().is_none() {
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
                    )?;
                    if !diff.is_empty() {
                        patch_content.push_str(&diff);
                    }
                }
                // If it has a file_name, it's a single file and likely wasn't modified
            }
            Source::Path(_) => {
                // Path sources are copied from local filesystem,
                // we could compare against the original path if needed
                // For now, skip as the original is still available
            }
        }
    }

    if patch_content.is_empty() {
        println!("No changes detected - no patch file created");
        return Ok(());
    }

    fs::write(&patch_path, patch_content)?;
    println!("Created patch file at: {}", patch_path.display());

    Ok(())
}

/// Creates a unified diff between two directories
fn create_directory_diff(
    original_dir: &Path,
    modified_dir: &Path,
    target_subdir: Option<&PathBuf>,
) -> Result<String, SourceError> {
    let mut patch_content = String::new();
    println!(
        "Creating patch for directories:\nOriginal: {}\nModified: {}",
        original_dir.display(),
        modified_dir.display()
    );
    // Walk through all files in the modified directory
    for entry in WalkDir::new(modified_dir)
        .into_iter()
        .filter_map(|e| e.ok())
        .filter(|e| e.file_type().is_file())
    {
        let modified_file = entry.path();

        // Skip the source info file
        if modified_file.file_name() == Some(std::ffi::OsStr::new(".source_info.json")) {
            continue;
        }

        // Calculate the relative path from the modified directory
        let rel_path = modified_file.strip_prefix(modified_dir)?;

        // Find the corresponding original file
        let original_file = original_dir.join(&rel_path);

        // Create the patch path (for display in the patch)
        let patch_path = if let Some(subdir) = target_subdir {
            PathBuf::from(subdir).join(&rel_path)
        } else {
            rel_path.to_path_buf()
        };

        if original_file.exists() {
            // Compare existing files
            let original_content = fs::read_to_string(&original_file).map_err(|_| {
                SourceError::UnknownError(format!(
                    "Failed to read original file: {}",
                    original_file.display()
                ))
            })?;
            let modified_content = fs::read_to_string(modified_file).map_err(|_| {
                SourceError::UnknownError(format!(
                    "Failed to read modified file: {}",
                    modified_file.display()
                ))
            })?;

            if original_content != modified_content {
                let patch = diffy::create_patch(&original_content, &modified_content);
                let unified_diff = format!("{}", diffy::PatchFormatter::new().fmt_patch(&patch));

                // Add proper file headers for the patch
                patch_content.push_str(&format!(
                    "--- a/{}\n+++ b/{}\n{}",
                    patch_path.display(),
                    patch_path.display(),
                    unified_diff
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

            let patch = diffy::create_patch("", &modified_content);
            let unified_diff = format!("{}", diffy::PatchFormatter::new().fmt_patch(&patch));

            patch_content.push_str(&format!(
                "--- /dev/null\n+++ b/{}\n{}",
                patch_path.display(),
                unified_diff
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
        let modified_file = modified_dir.join(&rel_path);

        if !modified_file.exists() {
            let patch_path = if let Some(subdir) = target_subdir {
                PathBuf::from(subdir).join(&rel_path)
            } else {
                rel_path.to_path_buf()
            };

            let original_content = fs::read_to_string(original_file).map_err(|_| {
                SourceError::UnknownError(format!(
                    "Failed to read deleted file: {}",
                    original_file.display()
                ))
            })?;

            let patch = diffy::create_patch(&original_content, "");
            let unified_diff = format!("{}", diffy::PatchFormatter::new().fmt_patch(&patch));

            patch_content.push_str(&format!(
                "--- a/{}\n+++ /dev/null\n{}",
                patch_path.display(),
                unified_diff
            ));
        }
    }

    Ok(patch_content)
}

/// Find the git cache directory for a given git source
fn find_git_cache_dir(
    cache_dir: &Path,
    git_src: &crate::recipe::parser::GitSource,
) -> Result<PathBuf, SourceError> {
    // This would need to match the logic in git_source::git_src
    // You might need to implement a helper function or store more info in SourceInformation
    // For now, this is a placeholder - you'll need to adapt based on your git caching strategy
    // let repo_name = git_src.url().path_segments()
    //     .and_then(|segments| segments.last())
    //     .and_then(|name| name.strip_suffix(".git"))
    //     .unwrap_or("unknown");

    let git_cache_dir = cache_dir.join("git").join("bla");
    // if git_cache_dir.exists() {
    //     Ok(git_cache_dir)
    // } else {
    Err(SourceError::FileNotFound(git_cache_dir))
    // }
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
