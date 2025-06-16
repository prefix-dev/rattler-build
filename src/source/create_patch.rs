//! Functions to create a new patch for a given directory using `diffy`.
//! We take all files found in this directory and compare them to the original files
//! from the source cache. Any differences will be written to a patch file.

use fs_err as fs;
use std::collections::{HashMap, HashSet};
use std::path::{Path, PathBuf};
use walkdir::WalkDir;

use crate::recipe::parser::Source;
use crate::source::{SourceError, SourceInformation};
use diffy::patches_from_str_with_config;

/// Represents a file modification (for tracking changes per source)
#[derive(Debug, Clone)]
struct FileModification {
    /// Path relative to the work directory
    relative_path: PathBuf,
    /// The patch content for this file
    patch_content: String,
    /// Whether this is a new file
    is_new_file: bool,
}

/// Load existing patches for a source and extract affected file paths
fn load_existing_patch_paths(
    source: &Source,
    recipe_dir: &Path,
) -> Result<HashSet<PathBuf>, SourceError> {
    let mut paths = HashSet::new();

    for patch_path in source.patches() {
        let full_patch_path = recipe_dir.join(patch_path);
        if full_patch_path.exists() {
            let patch_content = fs::read_to_string(&full_patch_path)?;
            let patches = patches_from_str_with_config(
                &patch_content,
                diffy::ParserConfig {
                    hunk_strategy: diffy::HunkRangeStrategy::Recount,
                },
            )
            .map_err(|_| SourceError::PatchParseFailed(full_patch_path.clone()))?;

            // Extract file paths from patches
            for patch in patches {
                if let Some(original) = patch.original() {
                    if original != "/dev/null" {
                        paths.insert(PathBuf::from(original));
                    }
                }
                if let Some(modified) = patch.modified() {
                    if modified != "/dev/null" {
                        paths.insert(PathBuf::from(modified));
                    }
                }
            }
        }
    }

    Ok(paths)
}

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

    let recipe_dir = source_info
        .recipe_path
        .parent()
        .ok_or_else(|| SourceError::UnknownError("Invalid recipe path".to_string()))?;
    let cache_dir = source_info.source_cache.clone();

    // Track modifications per source
    let mut source_modifications: HashMap<usize, Vec<FileModification>> = HashMap::new();
    let mut new_files: Vec<FileModification> = Vec::new();

    // Track which files are already covered by existing patches
    let mut files_in_existing_patches: HashSet<PathBuf> = HashSet::new();

    // Load existing patches for all sources
    for (_idx, source) in source_info.sources.iter().enumerate() {
        let paths = load_existing_patch_paths(source, recipe_dir)?;
        files_in_existing_patches.extend(paths);
    }

    // Process sources in reverse order (last to first)
    for (idx, source) in source_info.sources.iter().enumerate().rev() {
        let modifications = match source {
            Source::Git(git_src) => {
                let original_dir = find_git_cache_dir(&cache_dir, git_src)?;
                let target_dir = if let Some(target) = git_src.target_directory() {
                    work_dir.join(target)
                } else {
                    work_dir.to_path_buf()
                };

                process_directory_changes(
                    &original_dir,
                    &target_dir,
                    git_src.target_directory(),
                    &files_in_existing_patches,
                )?
            }
            Source::Url(url_src) => {
                if url_src.file_name().is_none() {
                    let original_dir = find_url_cache_dir(&cache_dir, url_src)?;
                    let target_dir = if let Some(target) = url_src.target_directory() {
                        work_dir.join(target)
                    } else {
                        work_dir.to_path_buf()
                    };

                    process_directory_changes(
                        &original_dir,
                        &target_dir,
                        url_src.target_directory(),
                        &files_in_existing_patches,
                    )?
                } else {
                    Vec::new()
                }
            }
            Source::Path(_) => Vec::new(),
        };

        // Separate new files from modifications
        for mod_item in modifications {
            if mod_item.is_new_file {
                new_files.push(mod_item);
            } else {
                source_modifications
                    .entry(idx)
                    .or_insert_with(Vec::new)
                    .push(mod_item);
            }
        }
    }

    // Add all new files to the first source
    if !new_files.is_empty() && !source_info.sources.is_empty() {
        source_modifications
            .entry(0)
            .or_insert_with(Vec::new)
            .extend(new_files);
    }

    // Generate patch files per source
    for (source_idx, modifications) in source_modifications {
        if modifications.is_empty() {
            continue;
        }

        let _source = &source_info.sources[source_idx];
        let patch_filename = format!("changes_source_{}.patch", source_idx);
        let patch_path = work_dir.join(&patch_filename);

        let mut patch_content = String::new();
        for mod_item in modifications {
            patch_content.push_str(&mod_item.patch_content);
        }

        fs::write(&patch_path, patch_content)?;
        println!("Created patch file at: {}", patch_path.display());
    }

    Ok(())
}

/// Process directory changes and return FileModification objects
fn process_directory_changes(
    original_dir: &Path,
    modified_dir: &Path,
    target_subdir: Option<&PathBuf>,
    files_in_existing_patches: &HashSet<PathBuf>,
) -> Result<Vec<FileModification>, SourceError> {
    let mut modifications = Vec::new();

    println!(
        "Processing directory changes:\nOriginal: {}\nModified: {}",
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

        // Create the patch path (for display in the patch)
        let patch_path = if let Some(subdir) = target_subdir {
            PathBuf::from(subdir).join(&rel_path)
        } else {
            rel_path.to_path_buf()
        };

        // Skip if this file is already covered by an existing patch
        if files_in_existing_patches.contains(&patch_path) {
            println!(
                "Skipping {} - already in existing patch",
                patch_path.display()
            );
            continue;
        }

        // Find the corresponding original file
        let original_file = original_dir.join(&rel_path);

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

                // Use improved formatting with context lines
                let formatter = diffy::PatchFormatter::new().with_color();
                let unified_diff = formatter.fmt_patch(&patch).to_string();

                // Create a well-formatted patch header
                let patch_content = format!(
                    "diff --git a/{} b/{}\n--- a/{}\n+++ b/{}\n{}",
                    patch_path.display(),
                    patch_path.display(),
                    patch_path.display(),
                    patch_path.display(),
                    unified_diff
                );

                modifications.push(FileModification {
                    relative_path: patch_path,
                    patch_content,
                    is_new_file: false,
                });
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
            let formatter = diffy::PatchFormatter::new().with_color();
            let unified_diff = formatter.fmt_patch(&patch).to_string();

            let patch_content = format!(
                "diff --git a/{} b/{}\nnew file mode 100644\n--- /dev/null\n+++ b/{}\n{}",
                patch_path.display(),
                patch_path.display(),
                patch_path.display(),
                unified_diff
            );

            modifications.push(FileModification {
                relative_path: patch_path,
                patch_content,
                is_new_file: true,
            });
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

        let patch_path = if let Some(subdir) = target_subdir {
            PathBuf::from(subdir).join(&rel_path)
        } else {
            rel_path.to_path_buf()
        };

        // Skip if this file is already covered by an existing patch
        if files_in_existing_patches.contains(&patch_path) {
            continue;
        }

        if !modified_file.exists() {
            let original_content = fs::read_to_string(original_file).map_err(|_| {
                SourceError::UnknownError(format!(
                    "Failed to read deleted file: {}",
                    original_file.display()
                ))
            })?;

            let patch = diffy::create_patch(&original_content, "");
            let formatter = diffy::PatchFormatter::new().with_color();
            let unified_diff = formatter.fmt_patch(&patch).to_string();

            let patch_content = format!(
                "diff --git a/{} b/{}\ndeleted file mode 100644\n--- a/{}\n+++ /dev/null\n{}",
                patch_path.display(),
                patch_path.display(),
                patch_path.display(),
                unified_diff
            );

            modifications.push(FileModification {
                relative_path: patch_path,
                patch_content,
                is_new_file: false,
            });
        }
    }

    Ok(modifications)
}

/// Find the git cache directory for a given git source
fn find_git_cache_dir(
    cache_dir: &Path,
    _git_src: &crate::recipe::parser::GitSource,
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

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::TempDir;

    #[test]
    fn test_process_directory_changes_skip_existing() -> Result<(), Box<dyn std::error::Error>> {
        let temp_dir = TempDir::new()?;
        let original_dir = temp_dir.path().join("original");
        let modified_dir = temp_dir.path().join("modified");

        fs::create_dir_all(&original_dir)?;
        fs::create_dir_all(&modified_dir)?;

        // Create files
        fs::write(original_dir.join("test.txt"), "original content")?;
        fs::write(modified_dir.join("test.txt"), "modified content")?;

        // Test without existing patches
        let empty_set = HashSet::new();
        let modifications =
            process_directory_changes(&original_dir, &modified_dir, None, &empty_set)?;
        assert_eq!(modifications.len(), 1);
        assert!(!modifications[0].is_new_file);

        // Test with existing patches
        let mut existing_patches = HashSet::new();
        existing_patches.insert(PathBuf::from("test.txt"));

        let modifications =
            process_directory_changes(&original_dir, &modified_dir, None, &existing_patches)?;
        assert_eq!(modifications.len(), 0); // Should skip the file

        Ok(())
    }

    #[test]
    fn test_new_file_detection() -> Result<(), Box<dyn std::error::Error>> {
        let temp_dir = TempDir::new()?;
        let original_dir = temp_dir.path().join("original");
        let modified_dir = temp_dir.path().join("modified");

        fs::create_dir_all(&original_dir)?;
        fs::create_dir_all(&modified_dir)?;

        // Create a new file only in modified directory
        fs::write(modified_dir.join("new_file.txt"), "new content")?;

        let empty_set = HashSet::new();
        let modifications =
            process_directory_changes(&original_dir, &modified_dir, None, &empty_set)?;

        assert_eq!(modifications.len(), 1);
        assert!(modifications[0].is_new_file);
        assert!(modifications[0].patch_content.contains("new file mode"));
        assert!(modifications[0].patch_content.contains("--- /dev/null"));

        Ok(())
    }

    #[test]
    fn test_deleted_file_detection() -> Result<(), Box<dyn std::error::Error>> {
        let temp_dir = TempDir::new()?;
        let original_dir = temp_dir.path().join("original");
        let modified_dir = temp_dir.path().join("modified");

        fs::create_dir_all(&original_dir)?;
        fs::create_dir_all(&modified_dir)?;

        // Create a file only in original directory (deleted)
        fs::write(original_dir.join("deleted.txt"), "old content")?;

        let empty_set = HashSet::new();
        let modifications =
            process_directory_changes(&original_dir, &modified_dir, None, &empty_set)?;

        assert_eq!(modifications.len(), 1);
        assert!(!modifications[0].is_new_file);
        assert!(modifications[0].patch_content.contains("deleted file mode"));
        assert!(modifications[0].patch_content.contains("+++ /dev/null"));

        Ok(())
    }
}
