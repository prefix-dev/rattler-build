//! Functions for applying patches to a work directory.
use crate::system_tools::{SystemTools, Tool};

use super::SourceError;

use std::io::Write;
use std::{
    collections::HashSet,
    ffi::OsStr,
    path::{Path, PathBuf},
};

use diffy::{ApplyStats, Diff, Patch};
use fs_err::File;
use itertools::Itertools;

fn is_dev_null(path: &str) -> bool {
    let trimmed = path.trim();
    trimmed == "/dev/null" || trimmed == "a/dev/null" || trimmed == "b/dev/null"
}

/// Summarize a single patch file by reading and parsing it.
pub fn summarize_single_patch(
    patch_path: &Path,
    work_dir: &Path,
) -> Result<PatchStats, SourceError> {
    let data = fs_err::read(patch_path).map_err(SourceError::Io)?;
    let patch = patch_from_bytes(&data)
        .map_err(|_| SourceError::PatchParseFailed(patch_path.to_path_buf()))?;
    summarize_patch(&patch, work_dir)
}

/// Normalizes backup file paths (.orig/.bak) to their actual file paths
/// Returns (original_path, modified_path) with backup files resolved
fn normalize_backup_paths(
    original_path: Option<PathBuf>,
    modified_path: Option<PathBuf>,
) -> (Option<PathBuf>, Option<PathBuf>) {
    let (Some(orig), Some(modified)) = (&original_path, &modified_path) else {
        return (original_path, modified_path);
    };

    // If paths are the same, no backup normalization needed
    if orig == modified {
        return (original_path, modified_path);
    }

    // Check if original file is a backup of the modified file
    if let (Some(orig_stem), Some(orig_ext)) = (orig.file_stem(), orig.extension())
        && let Some(mod_filename) = modified.file_name()
        && matches!(orig_ext.to_str(), Some("orig" | "bak"))
        && orig_stem == mod_filename
    {
        // Original is a backup of modified file, treat as modifying the actual file
        return (Some(modified.clone()), Some(modified.clone()));
    }

    (original_path, modified_path)
}

fn parse_patch(patch: &Patch<[u8]>) -> HashSet<PathBuf> {
    let mut affected_files = HashSet::new();

    for diff in patch {
        let original_path = diff
            .original()
            .and_then(|p| std::str::from_utf8(p).ok())
            .filter(|p| !is_dev_null(p))
            .map(PathBuf::from);

        let modified_path = diff
            .modified()
            .and_then(|p| std::str::from_utf8(p).ok())
            .filter(|p| !is_dev_null(p))
            .map(PathBuf::from);

        let (normalized_orig, normalized_mod) =
            normalize_backup_paths(original_path, modified_path);

        if let Some(path) = normalized_orig {
            affected_files.insert(path);
        }
        if let Some(path) = normalized_mod {
            affected_files.insert(path);
        }
    }

    affected_files
}

fn patch_from_bytes(input: &[u8]) -> Result<Patch<'_, [u8]>, diffy::ParsePatchError> {
    diffy::patch_from_bytes_with_config(
        input,
        diffy::ParserConfig {
            hunk_strategy: diffy::HunkRangeStrategy::Recount,
        },
    )
}

fn apply(
    base_image: &[u8],
    diff: &Diff<'_, [u8]>,
) -> Result<(Vec<u8>, ApplyStats), diffy::ApplyError> {
    diffy::apply_bytes_with_config(
        base_image,
        diff,
        &diffy::ApplyConfig {
            fuzzy_config: diffy::FuzzyConfig {
                max_fuzz: 2,
                ignore_whitespace: true,
                ignore_case: false,
            },
            ..Default::default()
        },
    )
}

// Returns number by which all patch paths must be stripped to be
// successfully applied, or returns and error if no such number could
// be determined.
fn guess_strip_level(patch: &Patch<[u8]>, work_dir: &Path) -> Result<usize, SourceError> {
    // There is no /dev/null in here by construction from `parse_patch`.
    let patched_files = parse_patch(patch);

    let max_components = patched_files
        .iter()
        .map(|p| p.components().count())
        .max()
        .unwrap_or(0);

    for strip_level in 0..max_components {
        // We check for _any_ path existing here. Sometimes patches reference
        // files that are deleted (e.g. `foobar.orig`) and thus it's more robust to look for
        // the first match in the affected files.
        let any_paths_exist = patched_files.iter().any(|p| {
            let path: PathBuf = p.components().skip(strip_level).collect();
            work_dir.join(path).exists()
        });
        if any_paths_exist {
            return Ok(strip_level);
        }
    }

    // XXX: This is not entirely correct way of handling this, since
    // path is not necessarily starts with meaningless one letter
    // component. Proper handling requires more in-depth analysis.
    // For example this is fine if source is /dev/null and target is
    // not, but may be incorrect otherwise, if original file does not
    // exist.
    Ok(1)
}

fn custom_patch_stripped_paths(
    diff: &Diff<'_, [u8]>,
    strip_level: usize,
) -> (Option<PathBuf>, Option<PathBuf>) {
    let strip_path = |path_bytes: &[u8]| -> Option<PathBuf> {
        std::str::from_utf8(path_bytes).ok().and_then(|p| {
            (!is_dev_null(p)).then(|| PathBuf::from(p).components().skip(strip_level).collect())
        })
    };

    let original_path = diff.original().and_then(strip_path);
    let modified_path = diff.modified().and_then(strip_path);

    normalize_backup_paths(original_path, modified_path)
}

fn write_patch_content(content: &[u8], path: &Path) -> Result<(), SourceError> {
    if let Some(parent) = path.parent() {
        fs_err::create_dir_all(parent).map_err(SourceError::Io)?;
    }

    // We want to be able to write to file.
    if path.exists() {
        let mut perms = fs_err::metadata(path)
            .map_err(SourceError::Io)?
            .permissions();
        if perms.readonly() {
            #[cfg(unix)]
            {
                use std::os::unix::fs::PermissionsExt;
                let user_write = 0o200;
                perms.set_mode(perms.mode() | user_write);
            }
            #[cfg(not(unix))]
            {
                // Assume this means windows
                #[allow(clippy::permissions_set_readonly_false)]
                perms.set_readonly(false);
            }
            fs_err::set_permissions(path, perms).map_err(SourceError::Io)?;
        }
    }

    let mut new_file = File::create(path).map_err(SourceError::Io)?;
    new_file.write_all(content).map_err(SourceError::Io)?;

    Ok(())
}

#[cfg(windows)]
fn temp_copy<P: AsRef<Path>>(src_path: P) -> std::io::Result<tempfile::NamedTempFile> {
    let mut src = File::open(src_path.as_ref())?;
    let mut tmp = tempfile::NamedTempFile::new()?;
    std::io::copy(&mut src, &mut tmp)?;
    Ok(tmp)
}

#[allow(dead_code)]
pub(crate) fn apply_patch_gnu(
    system_tools: &SystemTools,
    work_dir: &Path,
    patch_file_path: &Path,
) -> Result<(), SourceError> {
    let patch_file_content = fs_err::read(patch_file_path).map_err(SourceError::Io)?;

    let patch = patch_from_bytes(&patch_file_content)
        .map_err(|_| SourceError::PatchParseFailed(patch_file_path.to_path_buf()))?;
    let strip_level = guess_strip_level(&patch, work_dir)?;

    tracing::debug!("Patch {} will be applied", patch_file_path.display());

    // GNU patch treats some paths incorrectly on windows
    #[cfg(windows)]
    let patch_tmp = temp_copy(patch_file_path)?;
    #[cfg(windows)]
    let patch_file_path = patch_tmp.path();

    let mut tool = system_tools
        .call(Tool::Patch)
        .map_err(|_| SourceError::PatchExeNotFound)?;
    let cmd_builder = tool
        .arg(format!("-p{}", strip_level))
        .arg("--no-backup-if-mismatch")
        .arg("-i")
        .arg(String::from(patch_file_path.to_string_lossy()))
        .arg("-d")
        .arg(String::from(work_dir.to_string_lossy()));
    let output = cmd_builder.output()?;

    if !output.status.success() {
        return Err(SourceError::PatchFailed(format!(
            "{}\n`patch` failed with a combination of flags.\n\n{}",
            patch_file_path.display(),
            {
                let stderr = String::from_utf8_lossy(&output.stderr);
                let stdout = String::from_utf8_lossy(&output.stdout);
                format!(
                    "With the the command:\n\n\t{} {}\n\nThe stdout was:\n\n\t{}\n\nThe stderr was:\n\n\t{}\n\n",
                    cmd_builder.get_program().to_string_lossy(),
                    cmd_builder
                        .get_args()
                        .map(OsStr::to_string_lossy)
                        .format(" "),
                    stdout.lines().format("\n\t"),
                    stderr.lines().format("\n\t")
                )
            }
        )));
    }

    Ok(())
}

pub(crate) fn apply_patch_custom(
    work_dir: &Path,
    patch_file_path: &Path,
) -> Result<(), SourceError> {
    let patch_file_content = fs_err::read(patch_file_path).map_err(SourceError::Io)?;

    let patch = patch_from_bytes(&patch_file_content)
        .map_err(|_| SourceError::PatchParseFailed(patch_file_path.to_path_buf()))?;
    let strip_level = guess_strip_level(&patch, work_dir)?;

    for diff in patch {
        let file_paths = custom_patch_stripped_paths(&diff, strip_level);
        let absolute_file_paths = (
            file_paths.0.map(|o| work_dir.join(&o)),
            file_paths.1.map(|m| work_dir.join(&m)),
        );

        tracing::debug!(
            "Patch will be applied:\n\tFrom: {:#?}\n\tTo:{:#?}",
            absolute_file_paths.0,
            absolute_file_paths.1
        );

        match absolute_file_paths {
            (None, None) => continue,
            (None, Some(m)) => {
                let new_file_content = apply(&[], &diff).map_err(SourceError::PatchApplyError)?;
                write_patch_content(&new_file_content.0, &m)?;
            }
            (Some(o), None) => {
                fs_err::remove_file(work_dir.join(o)).map_err(SourceError::Io)?;
            }
            (Some(o), Some(m)) => {
                // Check if the original file exists
                // If it doesn't, treat this as creating a new file
                if !o.exists() {
                    let new_file_content =
                        apply(&[], &diff).map_err(SourceError::PatchApplyError)?;
                    write_patch_content(&new_file_content.0, &m)?;
                } else {
                    let old_file_content = fs_err::read(&o).map_err(SourceError::Io)?;

                    let new_file_content =
                        apply(&old_file_content, &diff).map_err(SourceError::PatchApplyError)?;

                    if o != m {
                        fs_err::remove_file(&o).map_err(SourceError::Io)?;
                    }

                    write_patch_content(&new_file_content.0, &m)?;
                }
            }
        }
    }

    Ok(())
}

/// Applies all patches in a list of patches to the specified work directory
/// Currently only supports patching with the `patch` command.
pub(crate) fn apply_patches(
    patches: &[PathBuf],
    work_dir: &Path,
    recipe_dir: &Path,
    apply_patch: impl Fn(&Path, &Path) -> Result<(), SourceError>,
) -> Result<(), SourceError> {
    for patch_path_relative in patches {
        let patch_file_path = recipe_dir.join(patch_path_relative);

        if !patch_file_path.exists() {
            return Err(SourceError::PatchNotFound(patch_file_path));
        }

        tracing::info!("Applying patch: {}", patch_file_path.to_string_lossy());
        apply_patch(work_dir, patch_file_path.as_path())?;
    }
    Ok(())
}

/// Summarized statistics of patch operations.
#[derive(Debug, Default)]
pub struct PatchStats {
    /// Files that have been modified (both original and modified exist).
    pub changed: Vec<PathBuf>,
    /// Files that have been added (original is /dev/null).
    pub added: Vec<PathBuf>,
    /// Files that have been removed (modified is /dev/null).
    pub removed: Vec<PathBuf>,
}

/// Summarize a single diff into added, removed, and changed files.
pub fn summarize_patch(diff: &Patch<[u8]>, work_dir: &Path) -> Result<PatchStats, SourceError> {
    let mut stats = PatchStats::default();
    let strip_level = guess_strip_level(diff, work_dir)?;
    for hunk in diff {
        let (orig_path, mod_path) = custom_patch_stripped_paths(hunk, strip_level);
        match (orig_path, mod_path) {
            // Both original and modified exist: record original (prefix stripped) as changed file
            (Some(orig), Some(_mod)) => stats.changed.push(orig),
            // Only modified exists: new file added
            (None, Some(modified)) => stats.added.push(modified),
            // Only original exists: file removed
            (Some(orig), None) => stats.removed.push(orig),
            _ => {}
        }
    }
    Ok(stats)
}

#[cfg(test)]
mod tests {
    use crate::source::copy_dir::CopyDir;

    #[cfg(feature = "patch-test-extra")]
    use crate::{
        get_build_output, get_tool_config,
        opt::{BuildData, BuildOpts, CommonOpts},
        recipe::parser::Source,
        script::SandboxArguments,
        tool_configuration::Configuration,
    };

    #[cfg(feature = "patch-test-extra")]
    use std::{ffi::OsStr, process::Command, sync::LazyLock};

    use super::*;
    use line_ending::LineEnding;

    #[cfg(feature = "patch-test-extra")]
    use miette::IntoDiagnostic;

    #[cfg(feature = "patch-test-extra")]
    use regex::Regex;

    #[cfg(feature = "patch-test-extra")]
    use rstest::*;

    use tempfile::TempDir;

    #[test]
    fn test_parse_patch() {
        let manifest_dir = PathBuf::from(env!("CARGO_MANIFEST_DIR"));
        let patches_dir = manifest_dir.join("test-data/patches");

        // for all patches, just try parsing the patch
        for entry in patches_dir.read_dir().unwrap() {
            let patch = entry.unwrap();
            let patch_path = patch.path();
            if patch_path.extension() != Some("patch".as_ref()) {
                continue;
            }

            let patch_file_content =
                fs_err::read(&patch_path).expect("Could not read file contents");
            let _ = patch_from_bytes(&patch_file_content).expect("Failed to parse patch file");

            println!("Parsing patch: {}", patch_path.display());
        }
    }

    #[test]
    fn get_affected_files() {
        let manifest_dir = PathBuf::from(env!("CARGO_MANIFEST_DIR"));
        let patches_dir = manifest_dir.join("test-data/patch_application/patches");

        let patch_file_content =
            fs_err::read(patches_dir.join("test.patch")).expect("Could not read file contents");
        let patch = patch_from_bytes(&patch_file_content).expect("Failed to parse patch file");

        let patched_paths = parse_patch(&patch);
        assert_eq!(patched_paths.len(), 1);
        assert!(patched_paths.contains(&PathBuf::from("text.md")));

        let patch_file_content =
            fs_err::read(patches_dir.join("0001-increase-minimum-cmake-version.patch"))
                .expect("Could not read file contents");
        let patch = patch_from_bytes(&patch_file_content).expect("Failed to parse patch file");
        let patched_paths = parse_patch(&patch);
        assert_eq!(patched_paths.len(), 1);
        assert!(patched_paths.contains(&PathBuf::from("CMakeLists.txt")));
    }

    fn setup_patch_test_dir() -> (TempDir, PathBuf) {
        let manifest_dir = PathBuf::from(env!("CARGO_MANIFEST_DIR"));
        let patch_test_dir = manifest_dir.join("test-data/patch_application");

        let tempdir = TempDir::new().unwrap();
        let _ = CopyDir::new(&patch_test_dir, tempdir.path()).run().unwrap();

        (tempdir, patch_test_dir)
    }

    #[test]
    fn test_apply_patches() {
        let (tempdir, _) = setup_patch_test_dir();

        // Test with normal patch
        apply_patches(
            &[PathBuf::from("test.patch")],
            &tempdir.path().join("workdir"),
            &tempdir.path().join("patches"),
            apply_patch_custom,
        )
        .unwrap();

        let text_md = tempdir.path().join("workdir/text.md");
        let text_md = fs_err::read_to_string(&text_md).unwrap();
        assert!(text_md.contains("Oh, wow, I was patched! Thank you soooo much!"));
    }

    #[test]
    fn test_apply_patches_with_orig() {
        let (tempdir, _) = setup_patch_test_dir();

        // Test with normal patch
        apply_patches(
            &[PathBuf::from("test_with_orig.patch")],
            &tempdir.path().join("workdir"),
            &tempdir.path().join("patches"),
            apply_patch_custom,
        )
        .unwrap();

        let text_md = tempdir.path().join("workdir/text.md");
        let text_md = fs_err::read_to_string(&text_md).unwrap();
        assert!(text_md.contains("Oh, wow, I was patched! Thank you soooo much!"));
    }

    #[test]
    fn test_apply_patches_with_bak() {
        let (tempdir, _) = setup_patch_test_dir();

        // Test with .bak extension patch
        apply_patches(
            &[PathBuf::from("test_with_bak.patch")],
            &tempdir.path().join("workdir"),
            &tempdir.path().join("patches"),
            apply_patch_custom,
        )
        .unwrap();

        let text_md = tempdir.path().join("workdir/text.md");
        let text_md = fs_err::read_to_string(&text_md).unwrap();
        assert!(text_md.contains("This was patched using .bak extension!"));
    }

    #[test]
    fn test_apply_patches_mixed_existing_files() {
        let (tempdir, _) = setup_patch_test_dir();

        // Test with patch that references both existing and non-existing files
        apply_patches(
            &[PathBuf::from("test_simple_mixed.patch")],
            &tempdir.path().join("workdir"),
            &tempdir.path().join("patches"),
            apply_patch_custom,
        )
        .unwrap();

        let existing_file = tempdir.path().join("workdir/existing_file.txt");
        let existing_file = fs_err::read_to_string(&existing_file).unwrap();
        assert!(existing_file.contains("Mixed files patch applied!"));

        let new_file = tempdir.path().join("workdir/new_file.txt");
        let new_file = fs_err::read_to_string(&new_file).unwrap();
        assert!(new_file.contains("This is a new file."));
        assert!(new_file.contains("Created by patch application."));
    }

    #[test]
    fn test_strip_level_detection_edge_case() {
        let (tempdir, _) = setup_patch_test_dir();

        // Test with patch that has deep paths but only some files exist
        apply_patches(
            &[PathBuf::from("test_strip_level_edge_case.patch")],
            &tempdir.path().join("workdir"),
            &tempdir.path().join("patches"),
            apply_patch_custom,
        )
        .unwrap();

        let deep_file = tempdir
            .path()
            .join("workdir/deep/nested/directory/deep_file.txt");
        let deep_file = fs_err::read_to_string(&deep_file).unwrap();
        assert!(deep_file.contains("Strip level test applied!"));
    }

    #[test]
    fn test_strip_level_algorithm_comparison() {
        // Test to demonstrate the difference between 'any' and 'all' logic
        let manifest_dir = PathBuf::from(env!("CARGO_MANIFEST_DIR"));
        let patch_file = manifest_dir
            .join("test-data/patch_application/patches/test_strip_level_edge_case.patch");
        let patch_content = fs_err::read(&patch_file).unwrap();
        let patch = patch_from_bytes(&patch_content).unwrap();

        let (tempdir, _) = setup_patch_test_dir();
        let work_dir = tempdir.path().join("workdir");

        // The current implementation should find strip level 4 based on existing deep file
        let strip_level = guess_strip_level(&patch, &work_dir).unwrap();
        assert_eq!(
            strip_level, 4,
            "Current 'any' logic should find strip level 4"
        );

        // Let's also test what the old 'all' logic would have found
        // (This is what the function used to do before the PR)
        let patched_files = parse_patch(&patch);
        let max_components = patched_files
            .iter()
            .map(|p| p.components().count())
            .max()
            .unwrap_or(0);

        let mut old_algorithm_result = None;
        for strip_level in 0..max_components {
            let all_paths_exist = patched_files.iter().all(|p| {
                let path: PathBuf = p.components().skip(strip_level).collect();
                work_dir.join(path).exists()
            });
            if all_paths_exist {
                old_algorithm_result = Some(strip_level);
                break;
            }
        }

        // The old algorithm would not have found any strip level where ALL files exist
        // because nonexistent_deep.txt.orig doesn't exist
        assert_eq!(
            old_algorithm_result, None,
            "Old 'all' logic should not find a valid strip level"
        );
    }

    #[test]
    fn test_backup_file_logic_edge_cases() {
        // Test edge cases in the backup file handling logic
        let patch_content = b"--- a/file.txt.orig\t2021-12-27 12:22:09.000000000 -0800\n+++ a/file.txt\t2021-12-27 12:22:14.000000000 -0800\n@@ -1 +1,2 @@\n original line\n+new line\n";

        let patch = patch_from_bytes(patch_content).unwrap();
        let diff = patch.into_iter().next().unwrap();

        // Test the backup file logic
        let paths = custom_patch_stripped_paths(&diff, 1);

        // Should treat this as modifying file.txt in place
        assert_eq!(paths.0, Some(PathBuf::from("file.txt")));
        assert_eq!(paths.1, Some(PathBuf::from("file.txt")));

        // Test .bak extension
        let patch_content = b"--- a/file.txt.bak\t2021-12-27 12:22:09.000000000 -0800\n+++ a/file.txt\t2021-12-27 12:22:14.000000000 -0800\n@@ -1 +1,2 @@\n original line\n+new line\n";

        let patch = patch_from_bytes(patch_content).unwrap();
        let diff = patch.into_iter().next().unwrap();

        let paths = custom_patch_stripped_paths(&diff, 1);
        assert_eq!(paths.0, Some(PathBuf::from("file.txt")));
        assert_eq!(paths.1, Some(PathBuf::from("file.txt")));

        // Test case where backup file logic should NOT apply
        let patch_content = b"--- a/different.txt.orig\t2021-12-27 12:22:09.000000000 -0800\n+++ a/file.txt\t2021-12-27 12:22:14.000000000 -0800\n@@ -1 +1,2 @@\n original line\n+new line\n";

        let patch = patch_from_bytes(patch_content).unwrap();
        let diff = patch.into_iter().next().unwrap();

        let paths = custom_patch_stripped_paths(&diff, 1);
        // Should NOT apply backup logic because different.txt.orig is not a backup of file.txt
        assert_eq!(paths.0, Some(PathBuf::from("different.txt.orig")));
        assert_eq!(paths.1, Some(PathBuf::from("file.txt")));
    }

    #[test]
    fn test_apply_patches_with_crlf() {
        let (tempdir, _) = setup_patch_test_dir();

        // Test with CRLF patch
        let patch = tempdir.path().join("patches/test.patch");
        let text = fs_err::read_to_string(&patch).unwrap();
        let clrf_patch = LineEnding::CRLF.apply(&text);

        fs_err::write(tempdir.path().join("patches/test_crlf.patch"), clrf_patch).unwrap();

        // Test with CRLF patch
        apply_patches(
            &[PathBuf::from("test_crlf.patch")],
            &tempdir.path().join("workdir"),
            &tempdir.path().join("patches"),
            apply_patch_custom,
        )
        .unwrap();

        let text_md = tempdir.path().join("workdir/text.md");
        let text_md = fs_err::read_to_string(&text_md).unwrap();
        assert!(text_md.contains("Oh, wow, I was patched! Thank you soooo much!"));
    }

    #[test]
    fn test_apply_pure_rename_patch() {
        let (tempdir, _) = setup_patch_test_dir();

        // Apply patch with pure renames (100% similarity, no content changes)
        apply_patches(
            &[PathBuf::from("test_pure_rename.patch")],
            &tempdir.path().join("workdir"),
            &tempdir.path().join("patches"),
            apply_patch_custom,
        )
        .expect("Pure rename patch should apply successfully");

        // Check that the __init__.py file was deleted
        let init_file = tempdir.path().join("workdir/tinygrad/frontend/__init__.py");
        assert!(
            !init_file.exists(),
            "frontend/__init__.py should be deleted"
        );

        // Check that onnx.py was renamed from frontend to nn
        let old_onnx = tempdir.path().join("workdir/tinygrad/frontend/onnx.py");
        let new_onnx = tempdir.path().join("workdir/tinygrad/nn/onnx.py");
        assert!(!old_onnx.exists(), "frontend/onnx.py should not exist");
        assert!(new_onnx.exists(), "nn/onnx.py should exist");
        let onnx_content = fs_err::read_to_string(&new_onnx).unwrap();
        assert_eq!(
            onnx_content, "# onnx code\n",
            "onnx.py content should be preserved"
        );

        // Check that torch.py was renamed from frontend to nn
        let old_torch = tempdir.path().join("workdir/tinygrad/frontend/torch.py");
        let new_torch = tempdir.path().join("workdir/tinygrad/nn/torch.py");
        assert!(!old_torch.exists(), "frontend/torch.py should not exist");
        assert!(new_torch.exists(), "nn/torch.py should exist");
        let torch_content = fs_err::read_to_string(&new_torch).unwrap();
        assert_eq!(
            torch_content, "# torch code\n",
            "torch.py content should be preserved"
        );
    }

    #[test]
    fn test_apply_create_delete_patch() {
        let (tempdir, _) = setup_patch_test_dir();

        // Create the file that will be deleted
        let to_delete = tempdir.path().join("workdir/to_be_deleted.txt");
        fs_err::write(&to_delete, "This file will be deleted\nby the patch\n").unwrap();

        // Apply patch with creation and deletion
        apply_patches(
            &[PathBuf::from("test_create_delete.patch")],
            &tempdir.path().join("workdir"),
            &tempdir.path().join("patches"),
            apply_patch_custom,
        )
        .expect("Create/delete patch should apply successfully");

        // Check that the file was deleted
        assert!(!to_delete.exists(), "to_be_deleted.txt should be deleted");

        // Check that the new file was created with correct content
        let created_file = tempdir.path().join("workdir/newly_created.txt");
        assert!(created_file.exists(), "newly_created.txt should exist");
        let content = fs_err::read_to_string(&created_file).unwrap();
        assert!(content.contains("This is a newly created file"));
        assert!(content.contains("via patch application"));
    }

    #[test]
    fn test_apply_0001_increase_minimum_cmake_version_patch() {
        let (tempdir, _) = setup_patch_test_dir();

        apply_patches(
            &[PathBuf::from("0001-increase-minimum-cmake-version.patch")],
            &tempdir.path().join("workdir"),
            &tempdir.path().join("patches"),
            apply_patch_custom,
        )
        .expect("Patch 0001-increase-minimum-cmake-version.patch should apply successfully");

        // Read the cmake list file and make sure that it contains `cmake_minimum_required(VERSION 3.12)`
        let cmake_list = tempdir.path().join("workdir/CMakeLists.txt");
        let cmake_list = fs_err::read_to_string(&cmake_list).unwrap();
        assert!(cmake_list.contains("cmake_minimum_required(VERSION 3.12)"));
    }

    #[test]
    fn test_apply_git_patch_in_git_ignored() {
        let (tempdir, _) = setup_patch_test_dir();

        // Apply the patches in the working directory
        apply_patches(
            &[PathBuf::from("0001-increase-minimum-cmake-version.patch")],
            &tempdir.path().join("workdir"),
            &tempdir.path().join("patches"),
            apply_patch_custom,
        )
        .expect("Patch 0001-increase-minimum-cmake-version.patch should apply successfully");

        // Read the cmake list file and make sure that it contains `cmake_minimum_required(VERSION 3.12)`
        let cmake_list = tempdir.path().join("workdir/CMakeLists.txt");
        let cmake_list = fs_err::read_to_string(&cmake_list).unwrap();
        assert!(cmake_list.contains("cmake_minimum_required(VERSION 3.12)"));
    }

    #[test]
    fn test_missing_patch_file_returns_error() {
        let (tempdir, _) = setup_patch_test_dir();

        // Try to apply a patch that doesn't exist (simulating a typo in the patch filename)
        // This could happen e.g. when git format-patch creates a file with a double period
        // due to a commit message ending with a period, and the user accidentally removes one period
        let result = apply_patches(
            &[PathBuf::from("nonexistent-patch-file..patch")],
            &tempdir.path().join("workdir"),
            &tempdir.path().join("patches"),
            apply_patch_custom,
        );

        // The build should fail with PatchNotFound error, not silently continue
        assert!(result.is_err(), "Missing patch file should cause an error");
        let err = result.unwrap_err();
        assert!(
            matches!(err, SourceError::PatchNotFound(_)),
            "Expected PatchNotFound error, got: {:?}",
            err
        );
    }

    /// Prepare all information needed to test patches for package info path.
    #[cfg(feature = "patch-test-extra")]
    async fn prepare_sources(recipe_dir: &Path) -> miette::Result<(Configuration, Vec<Source>)> {
        let artifacts_dir = tempfile::tempdir().unwrap();
        let artifacts_dir_path = artifacts_dir.path().join("original");
        let recipe_path = recipe_dir.join("recipe.yaml");

        let opts = BuildOpts {
            recipe_dir: Some(recipe_dir.into()),
            // // Good if you want to try out recipe for different platform, since we are not building them anyway.
            // build_platform: Some(rattler_conda_types::Platform::Win64),
            // target_platform: Some(rattler_conda_types::Platform::Win64),
            // host_platform: Some(rattler_conda_types::Platform::Win64),
            no_build_id: true,
            no_test: true,
            common: CommonOpts {
                use_zstd: true,
                use_bz2: true,
                use_sharded: true,
                use_jlap: false,
                output_dir: Some(artifacts_dir_path),
                ..Default::default()
            },
            sandbox_arguments: SandboxArguments {
                sandbox: true,
                allow_network: true,
                ..Default::default()
            },
            continue_on_failure: true,
            ..Default::default()
        };

        let build_data: BuildData = BuildData::from_opts_and_config(opts, None);
        let tool_config: Configuration = get_tool_config(&build_data, &None).unwrap();

        let outputs = get_build_output(&build_data, &recipe_path, &tool_config).await?;

        let mut patchable_sources = vec![];
        for output in outputs {
            let sources = output.recipe.sources();
            for source in sources {
                if !source.patches().is_empty() {
                    patchable_sources.push(source.clone())
                }
            }
        }

        patchable_sources.dedup();

        Ok((tool_config, patchable_sources))
    }

    #[cfg(feature = "patch-test-extra")]
    fn show_dir_difference(common_parent: &Path) -> miette::Result<String> {
        let mut cmd = Command::new("diff");
        // So snapshots doesn't change all the time
        let original_dir = PathBuf::from("./original");
        let copy_dir = PathBuf::from("./copy");
        let stdout = cmd
            .current_dir(common_parent)
            .args([
                OsStr::new("-rNul"),
                OsStr::new("--strip-trailing-cr"),
                OsStr::new("--color=auto"),
                original_dir.as_os_str(),
                copy_dir.as_os_str(),
            ])
            .output()
            .into_diagnostic()?
            .stdout;

        static RE: LazyLock<Regex> =
            LazyLock::new(|| Regex::new(r"(?m)^(?<fileonly>(\+\+\+|---) .*)\t.*$").unwrap());

        let dir_difference = String::from_utf8(stdout).unwrap();
        let dir_difference = RE.replace_all(&dir_difference, "$fileonly");

        Ok(dir_difference.to_string())
    }

    /// Applied patches is vector of strip level and diffs from one patch file.
    #[cfg(feature = "patch-test-extra")]
    fn snapshot_patched_files(
        package_name: &str,
        applied_patches: &Vec<(usize, Vec<Diff<'_, [u8]>>)>,
        comparison_dir: &Path,
    ) -> miette::Result<()> {
        let mut patch_results = vec![];
        for (strip_level, patchset) in applied_patches.iter() {
            for patch in patchset.iter() {
                let file_paths = custom_patch_stripped_paths(patch, *strip_level);
                let absolute_file_paths = (
                    file_paths.0.map(|o| comparison_dir.join("copy").join(&o)),
                    file_paths.1.map(|m| comparison_dir.join("copy").join(&m)),
                );

                #[derive(Debug)]
                #[allow(dead_code)]
                enum PatchResult {
                    Created(bool),
                    Deleted(bool),
                    Modified(String),
                }

                match absolute_file_paths {
                    (None, None) => (), // Assume that it will do nothing.
                    (None, Some(m)) => {
                        patch_results.push((patch, PatchResult::Created(m.exists())))
                    }

                    (Some(o), None) => {
                        patch_results.push((patch, PatchResult::Deleted(!o.exists())))
                    }
                    (Some(_), Some(m)) => {
                        let modified_file_contents = fs_err::read(m).into_diagnostic()?;
                        let modified_file_debug_representation =
                            String::from_utf8(modified_file_contents)
                                .unwrap_or_else(|e| format!("{:#?}", e.into_bytes()));
                        patch_results.push((
                            patch,
                            PatchResult::Modified(modified_file_debug_representation),
                        ))
                    }
                }
            }
        }
        insta::assert_debug_snapshot!(package_name, patch_results);

        Ok(())
    }

    /// Compare custom patch application with reference git patch application.
    ///
    /// Takes a long time to execute, on my machine it takes around 7
    /// minutes. Require up to several gigabytes of memory available in
    /// temporary files directory.
    ///
    /// Algorithm:
    ///
    /// 1. Create temporary directory which will contain a copy of a work dir.
    /// 2. Copy work dir to the temporary directory.
    /// 3. Patch original work dir using `git apply`.
    /// 4. Patch temporary work dir using custom patch application.
    /// 5. Compare directories.
    #[cfg(feature = "patch-test-extra")]
    #[ignore]
    #[rstest]
    #[tokio::test]
    async fn test_package_from_conda_forge(
        #[base_dir = "test-data/conda_forge/recipes"]
        #[dirs]
        #[files("*")]
        // Slow tests
        #[exclude("(root)|(tiledbsoma)|(libmodplug)")]
        // Insane patch format, needs further investigation on why it
        // even works.
        #[exclude("mumps")]
        // Failed to download source
        #[exclude("petsc")]
        // GNU patch fails and diffy succeeds, seemingly correctly from the diff output.
        #[exclude("(fastjet-cxx)|(fenics-)|(flask-security-too)")]
        // Parse fails, since createrepo-c/438.patch contains two mail
        // messages in one file. Fix postponed until parser
        // reimplemented.
        #[exclude("createrepo_c")]
        recipe_dir: PathBuf,
    ) -> miette::Result<()> {
        let snapshot_tested = ["love2d"];
        let pkg_name = recipe_dir.as_path().file_name().unwrap().to_str().unwrap();
        let is_snapshot_test = snapshot_tested.contains(&pkg_name);

        let (tool_config, sources) = prepare_sources(&recipe_dir).await?;
        for source in sources {
            use crate::source::fetch_source;

            let comparison_dir = tempfile::tempdir().into_diagnostic()?;

            // If you rename these, don't forget to change names in `show_dir_difference`.
            let original_dir = comparison_dir.path().join("original");
            fs_err::create_dir(&original_dir).into_diagnostic()?;
            let copy_dir = comparison_dir.path().join("copy");
            fs_err::create_dir(&copy_dir).into_diagnostic()?;
            let cache_src = comparison_dir.path().join("cache");
            fs_err::create_dir(&cache_src).into_diagnostic()?;

            let mut _rendered_sources = vec![];

            // Fetch source
            fetch_source(
                &source,
                &mut _rendered_sources,
                &original_dir,
                &recipe_dir,
                &cache_src,
                &SystemTools::new(),
                &tool_config,
                |_, _| Ok(()),
            )
            .await
            .into_diagnostic()?;

            // Create copy of that directory.
            CopyDir::new(&original_dir, &copy_dir)
                .run()
                .into_diagnostic()?;

            let patches = source.patches().to_vec();
            let target_directory = source.target_directory();

            let (original_source_dir_path, patched_source_dir_path) = match target_directory {
                Some(td) => (&original_dir.join(td), &copy_dir.join(td)),
                None => (&original_dir, &copy_dir),
            };

            let gnu_patch_res = if !is_snapshot_test {
                apply_patches(
                    patches.as_slice(),
                    original_source_dir_path,
                    &recipe_dir,
                    |wd, p| apply_patch_gnu(&SystemTools::new(), wd, p),
                )
            } else {
                Ok(())
            };

            let custom_res = apply_patches(
                patches.as_slice(),
                patched_source_dir_path,
                &recipe_dir,
                apply_patch_custom,
            );

            match (custom_res, gnu_patch_res) {
                (Ok(_), Ok(_)) => (),
                (Ok(_), Err(err)) => panic!("Gnu patch failed:\n{}", err),
                (Err(err), Ok(_)) => panic!("Diffy patch failed:\n{}", err),
                (Err(cerr), Err(gerr)) => panic!("Both failed:\n{}\n{}", cerr, gerr),
            }

            let difference = show_dir_difference(comparison_dir.path())
                .expect("Can't show dir difference. Most probably you're missing GNU diff binary.");

            if !difference.trim().is_empty() {
                if is_snapshot_test {
                    let patches_file_content = patches
                        .iter()
                        .map(|pp| fs_err::read(recipe_dir.join(pp)))
                        .collect::<Result<Vec<_>, _>>()
                        .into_diagnostic()?;
                    let mut patch_files = vec![];
                    for patch_file_content in patches_file_content.iter() {
                        let patches = patch_from_bytes(patch_file_content).into_diagnostic()?;
                        let strip_level = guess_strip_level(&patches, original_source_dir_path)
                            .into_diagnostic()?;
                        patch_files.push((strip_level, patches));
                    }
                    snapshot_patched_files(pkg_name, &patch_files, comparison_dir.path())?;
                } else {
                    // If we panic on just nonempty difference then
                    // there are 4 more tests failing, because git
                    // does not apply patches. Specifically
                    // `hf_transfer`, `lua`, `nordugrid_arc`,
                    // `openjph`.
                    panic!("Directories are different:\n{}", difference);
                }
            }
        }

        Ok(())
    }
}
