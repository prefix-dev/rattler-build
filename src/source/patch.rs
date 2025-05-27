//! Functions for applying patches to a work directory.
use super::SourceError;
use crate::system_tools::{SystemTools, Tool};
use itertools::Itertools;
use std::process::{Command, Output};
use std::{
    collections::HashSet,
    ffi::OsStr,
    io::{BufRead, BufReader},
    path::{Path, PathBuf},
    process::Stdio,
};

fn parse_patch_file<P: AsRef<Path>>(patch_file: P) -> std::io::Result<HashSet<PathBuf>> {
    let file = fs_err::File::open(patch_file.as_ref())?;
    let reader = BufReader::new(file);
    let mut affected_files = HashSet::new();

    // Common patch file patterns
    let unified_pattern = "--- ";
    let git_pattern = "diff --git ";
    let traditional_pattern = "Index: ";
    let mut is_git = false;
    for line in reader.lines() {
        let line = line?;

        if let Some(git_line) = line.strip_prefix(git_pattern) {
            is_git = true;
            if let Some(file_path) = extract_git_file_path(git_line) {
                affected_files.insert(file_path);
            }
        } else if let Some(unified_line) = line.strip_prefix(unified_pattern) {
            if is_git || unified_line.contains("/dev/null") {
                continue;
            }
            if let Some(file_path) = clean_file_path(unified_line) {
                affected_files.insert(file_path);
            }
        } else if let Some(traditional_line) = line.strip_prefix(traditional_pattern) {
            if let Some(file_path) = clean_file_path(traditional_line) {
                affected_files.insert(file_path);
            }
        }
    }

    Ok(affected_files)
}

fn clean_file_path(path_str: &str) -> Option<PathBuf> {
    let path = path_str.trim();

    // Handle timestamp in unified diff format (file.txt\t2023-05-10 10:00:00)
    let path = path.split('\t').next().unwrap_or(path);

    // Skip /dev/null entries
    if path.is_empty() || path == "/dev/null" {
        return None;
    }

    Some(PathBuf::from(path))
}

fn extract_git_file_path(content: &str) -> Option<PathBuf> {
    // Format: "a/file.txt b/file.txt"
    let parts: Vec<&str> = content.split(' ').collect();
    if parts.len() >= 2 {
        let a_file = parts[0];
        if a_file.starts_with("a/") && a_file != "a/dev/null" {
            return Some(PathBuf::from(&a_file));
        }
    }

    None
}

/// We try to guess the "strip level" for a patch application. This is done by checking
/// what files are present in the work directory and comparing them to the paths in the patch.
///
/// If we find a file in the work directory that matches the path in the patch, we can guess the
/// strip level and use that when invoking the `patch` command.
///
/// For example, a patch might contain a line saying something like: `a/repository/contents/file.c`.
/// But in our work directory, we only have `contents/file.c`. In this case, we can guess that the
/// strip level is 2 and we can apply the patch successfully.
fn guess_strip_level(patch: &Path, work_dir: &Path) -> Result<usize, std::io::Error> {
    let patched_files = parse_patch_file(patch).map_err(|e| {
        std::io::Error::new(
            std::io::ErrorKind::InvalidData,
            format!("Failed to parse patch file: {}", e),
        )
    })?;

    // Try to guess the strip level by checking if the path exists in the work directory
    for file in patched_files {
        // This means the patch is creating an entirely new file so we can't guess the strip level
        if file == Path::new("/dev/null") {
            continue;
        }
        for strip_level in 0..file.components().count() {
            let mut new_path = work_dir.to_path_buf();
            new_path.extend(file.components().skip(strip_level));
            if new_path.exists() {
                return Ok(strip_level);
            }
        }
    }

    // If we can't guess the strip level, default to 1 (usually the patch file starts with a/ and b/)
    Ok(1)
}

/// Applies all patches in a list of patches to the specified work directory
/// Currently only supports patching with the `patch` command.
pub(crate) fn apply_patches(
    system_tools: &SystemTools,
    patches: &[PathBuf],
    work_dir: &Path,
    recipe_dir: &Path,
) -> Result<(), SourceError> {
    // Early out to avoid unnecessary work
    if patches.is_empty() {
        return Ok(());
    }

    // Ensure that the working directory is a valid git directory.
    let git_dir = work_dir.join(".git");
    let _dot_git_dir = if !git_dir.exists() {
        Some(TempDotGit::setup(work_dir)?)
    } else {
        None
    };

    for patch_path_relative in patches {
        let patch_file_path = recipe_dir.join(patch_path_relative);

        tracing::info!("Applying patch: {}", patch_file_path.to_string_lossy());

        if !patch_file_path.exists() {
            return Err(SourceError::PatchNotFound(patch_file_path));
        }

        let strip_level = guess_strip_level(&patch_file_path, work_dir)?;

        struct GitApplyAttempt {
            command: Command,
            output: Output,
        }

        let mut outputs = Vec::new();
        for try_extra_flag in [None, Some("--recount")] {
            let mut cmd_builder = system_tools
                .call(Tool::Git)
                .map_err(SourceError::GitNotFound)?;
            cmd_builder
                .current_dir(work_dir)
                .arg("apply")
                .arg(format!("-p{}", strip_level))
                .arg("--verbose")
                .arg("--ignore-space-change")
                .arg("--ignore-whitespace")
                .args(try_extra_flag.into_iter())
                .arg(patch_file_path.as_os_str())
                .stdout(Stdio::piped())
                .stderr(Stdio::piped());

            tracing::debug!(
                "Running: {} {}",
                cmd_builder.get_program().to_string_lossy(),
                cmd_builder
                    .get_args()
                    .map(OsStr::to_string_lossy)
                    .format(" ")
            );

            let output = cmd_builder.output().map_err(SourceError::Io)?;
            outputs.push(GitApplyAttempt {
                command: cmd_builder,
                output: output.clone(),
            });

            if outputs
                .last()
                .expect("we just added an entry")
                .output
                .status
                .success()
            {
                break;
            }
        }

        // Check if the last output was successful, if not, we report all the errors.
        let last_output = outputs.last().expect("we just added at least one entry");
        if !last_output.output.status.success() {
            return Err(SourceError::PatchFailed(format!(
                "{}\n`git apply` failed with a combination of flags.\n\n{}",
                patch_path_relative.display(),
                outputs
                    .into_iter()
                    .map(
                        |GitApplyAttempt {
                             output, command, ..
                         }| {
                            let stderr = String::from_utf8_lossy(&output.stderr);
                            format!(
                                "With the che command:\n\n\t{} {}The output was:\n\n\t{}\n\n",
                                command.get_program().to_string_lossy(),
                                command.get_args().map(OsStr::to_string_lossy).format(" "),
                                stderr.lines().format("\n\t")
                            )
                        }
                    )
                    .format("\n\n")
            )));
        }

        // Sometimes git apply will skip the contents of a patch. This usually is *not* what we
        // want, so we detect this behavior and return an error.
        let stderr = String::from_utf8_lossy(&last_output.output.stderr);
        let skipped_patch = stderr
            .lines()
            .any(|line| line.starts_with("Skipped patch "));
        if skipped_patch {
            return Err(SourceError::PatchFailed(format!(
                "{}\n`git apply` seems to have skipped some of the contents of the patch. The output of the command is:\n\n\t{}\n\nThe command was invoked with:\n\n\t{} {}",
                patch_path_relative.display(),
                stderr.lines().format("\n\t"),
                last_output.command.get_program().to_string_lossy(),
                last_output
                    .command
                    .get_args()
                    .map(OsStr::to_string_lossy)
                    .format(" ")
            )));
        }
    }
    Ok(())
}

/// A temporary .git directory that contains the bare minimum files and
/// directories needed for git to function as if the directory that contains
/// the .git directory is a proper git repository.
struct TempDotGit {
    path: PathBuf,
}

impl TempDotGit {
    /// Creates a temporary .git directory in the specified root directory.
    fn setup(root: &Path) -> std::io::Result<Self> {
        // Initialize a temporary .git directory
        let dot_git = root.join(".git");
        fs_err::create_dir(&dot_git)?;
        let dot_git = TempDotGit { path: dot_git };

        // Add the minimum number of files and directories to the .git directory that are needed for
        // git to work
        fs_err::create_dir(dot_git.path.join("objects"))?;
        fs_err::create_dir(dot_git.path.join("refs"))?;
        fs_err::write(dot_git.path.join("HEAD"), "ref: refs/heads/main")?;

        Ok(dot_git)
    }
}

impl Drop for TempDotGit {
    fn drop(&mut self) {
        fs_err::remove_dir_all(&self.path).unwrap_or_else(|e| {
            eprintln!("Failed to remove temporary .git directory: {}", e);
        });
    }
}

#[cfg(test)]
mod tests {
    use crate::source::copy_dir::CopyDir;

    use super::*;
    use line_ending::LineEnding;
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

            let parsed = parse_patch_file(&patch_path);
            if let Err(e) = &parsed {
                eprintln!("Failed to parse patch: {} {}", patch_path.display(), e);
            }

            println!("Parsing patch: {} {}", patch_path.display(), parsed.is_ok());
        }
    }

    #[test]
    fn get_affected_files() {
        let manifest_dir = PathBuf::from(env!("CARGO_MANIFEST_DIR"));
        let patches_dir = manifest_dir.join("test-data/patch_application/patches");

        let patched_paths = parse_patch_file(patches_dir.join("test.patch")).unwrap();
        assert_eq!(patched_paths.len(), 1);
        assert!(patched_paths.contains(&PathBuf::from("a/text.md")));

        let patched_paths =
            parse_patch_file(patches_dir.join("0001-increase-minimum-cmake-version.patch"))
                .unwrap();
        assert_eq!(patched_paths.len(), 1);
        assert!(patched_paths.contains(&PathBuf::from("a/CMakeLists.txt")));
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
            &SystemTools::new(),
            &[PathBuf::from("test.patch")],
            &tempdir.path().join("workdir"),
            &tempdir.path().join("patches"),
        )
        .unwrap();

        let text_md = tempdir.path().join("workdir/text.md");
        let text_md = fs_err::read_to_string(&text_md).unwrap();
        assert!(text_md.contains("Oh, wow, I was patched! Thank you soooo much!"));
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
            &SystemTools::new(),
            &[PathBuf::from("test_crlf.patch")],
            &tempdir.path().join("workdir"),
            &tempdir.path().join("patches"),
        )
        .unwrap();

        let text_md = tempdir.path().join("workdir/text.md");
        let text_md = fs_err::read_to_string(&text_md).unwrap();
        assert!(text_md.contains("Oh, wow, I was patched! Thank you soooo much!"));
    }

    #[test]
    fn test_apply_0001_increase_minimum_cmake_version_patch() {
        let (tempdir, _) = setup_patch_test_dir();

        apply_patches(
            &SystemTools::new(),
            &[PathBuf::from("0001-increase-minimum-cmake-version.patch")],
            &tempdir.path().join("workdir"),
            &tempdir.path().join("patches"),
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

        // Initialize a temporary .git directory at the root of the temporary directory. This makes
        // git take the working directory is in a git repository.
        let _temp_dot_git = TempDotGit::setup(tempdir.path()).unwrap();

        // Apply the patches in the working directory
        apply_patches(
            &SystemTools::new(),
            &[PathBuf::from("0001-increase-minimum-cmake-version.patch")],
            &tempdir.path().join("workdir"),
            &tempdir.path().join("patches"),
        )
        .expect("Patch 0001-increase-minimum-cmake-version.patch should apply successfully");

        // Read the cmake list file and make sure that it contains `cmake_minimum_required(VERSION 3.12)`
        let cmake_list = tempdir.path().join("workdir/CMakeLists.txt");
        let cmake_list = fs_err::read_to_string(&cmake_list).unwrap();
        assert!(cmake_list.contains("cmake_minimum_required(VERSION 3.12)"));
    }
}
