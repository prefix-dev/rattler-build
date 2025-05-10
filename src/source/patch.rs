//! Functions for applying patches to a work directory.
use std::{
    collections::HashSet,
    io::{BufRead, BufReader, Write},
    path::{Path, PathBuf},
    process::Stdio,
};

use super::SourceError;
use crate::system_tools::{SystemTools, Tool};

fn parse_patch_file<P: AsRef<Path>>(patch_file: P) -> std::io::Result<HashSet<PathBuf>> {
    let file = fs_err::File::open(patch_file.as_ref())?;
    let reader = BufReader::new(file);
    let mut affected_files = HashSet::new();

    // Common patch file patterns
    let unified_pattern = "--- ";
    let git_pattern = "diff --git ";
    let traditional_pattern = "Index: ";

    for line in reader.lines() {
        let line = line?;

        if line.starts_with(git_pattern) {
            if let Some(file_path) = extract_git_file_path(&line[git_pattern.len()..]) {
                affected_files.insert(file_path);
            }
        } else if line.starts_with(unified_pattern) && !line.contains("/dev/null") {
            if let Some(file_path) = clean_file_path(&line[unified_pattern.len()..]) {
                affected_files.insert(file_path);
            }
        } else if line.starts_with(traditional_pattern) {
            if let Some(file_path) = clean_file_path(&line[traditional_pattern.len()..]) {
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
        // Take the second part (b/file.txt) and remove the b/ prefix
        let b_file = parts[1];
        if b_file.starts_with("b/") && b_file != "b/dev/null" {
            return Some(PathBuf::from(&b_file));
        }

        // If b/file.txt is /dev/null, use a/file.txt instead
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
    for patch_path_relative in patches {
        let patch_file_path = recipe_dir.join(patch_path_relative);

        tracing::info!("Applying patch: {}", patch_file_path.to_string_lossy());

        if !patch_file_path.exists() {
            return Err(SourceError::PatchNotFound(patch_file_path));
        }

        // Read the patch content into a string. This also normalizes line endings to LF.
        let patch_content_for_stdin =
            fs_err::read_to_string(&patch_file_path).map_err(SourceError::Io)?;

        let strip_level = guess_strip_level(&patch_file_path, work_dir)?;

        let mut cmd_builder = system_tools
            .call(Tool::Git)
            .map_err(SourceError::GitNotFound)?;

        cmd_builder
            .current_dir(work_dir)
            .arg("apply")
            .arg(format!("-p{}", strip_level))
            .arg("--ignore-space-change")
            .arg("--ignore-whitespace")
            .arg("--recount")
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::piped());

        let mut child_process = cmd_builder.spawn().map_err(SourceError::Io)?;

        // Write the patch content to the child process's stdin.
        {
            if let Some(mut child_stdin) = child_process.stdin.take() {
                child_stdin
                    .write_all(patch_content_for_stdin.as_bytes())
                    .map_err(SourceError::Io)?;
            } else {
                return Err(SourceError::Io(std::io::Error::new(
                    std::io::ErrorKind::Other,
                    "Failed to obtain stdin handle for git apply",
                )));
            }
        }

        let output = child_process.wait_with_output().map_err(SourceError::Io)?;

        if !output.status.success() {
            eprintln!(
                "Failed to apply patch: {}",
                patch_file_path.to_string_lossy()
            );
            eprintln!("Stdout: {}", String::from_utf8_lossy(&output.stdout));
            eprintln!("Stderr: {}", String::from_utf8_lossy(&output.stderr));
            return Err(SourceError::PatchFailed(
                patch_file_path.to_string_lossy().to_string(),
            ));
        }
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use crate::source::copy_dir::CopyDir;

    use super::*;
    use gitpatch::Patch;
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

            let ps = fs_err::read_to_string(&patch_path).unwrap();
            let parsed = Patch::from_multiple(&ps);

            println!("Parsing patch: {} {}", patch_path.display(), parsed.is_ok());
        }
    }

    #[test]
    fn get_affected_files() {
        let manifest_dir = PathBuf::from(env!("CARGO_MANIFEST_DIR"));
        let patches_dir = manifest_dir.join("test-data/patch_application/patches");

        let patched_paths = parse_patch_file(patches_dir.join("test.patch")).unwrap();
        assert_eq!(patched_paths.len(), 2);
        assert!(patched_paths.contains(&PathBuf::from("a/text.md")));
        assert!(patched_paths.contains(&PathBuf::from("b/text.md")));

        let patched_paths =
            parse_patch_file(patches_dir.join("0001-increase-minimum-cmake-version.patch"))
                .unwrap();
        assert_eq!(patched_paths.len(), 2);
        assert!(patched_paths.contains(&PathBuf::from("a/CMakeLists.txt")));
        assert!(patched_paths.contains(&PathBuf::from("b/CMakeLists.txt")));
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
}
