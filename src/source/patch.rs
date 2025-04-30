//! Functions for applying patches to a work directory.
use std::{
    ops::Deref,
    path::{Path, PathBuf},
};

use gitpatch::Patch;

use super::SourceError;
use crate::system_tools::{SystemTools, Tool};

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
    let text = fs_err::read_to_string(patch)?;
    let Ok(patches) = Patch::from_multiple(&text) else {
        return Ok(1);
    };

    // Try to guess the strip level by checking if the path exists in the work directory
    for p in patches {
        let path = PathBuf::from(p.old.path.deref());
        // This means the patch is creating an entirely new file so we can't guess the strip level
        if path == Path::new("/dev/null") {
            continue;
        }
        for strip_level in 0..path.components().count() {
            let mut new_path = work_dir.to_path_buf();
            new_path.extend(path.components().skip(strip_level));
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
    for patch in patches {
        let patch = recipe_dir.join(patch);

        tracing::info!("Applying patch: {}", patch.to_string_lossy());

        if !patch.exists() {
            return Err(SourceError::PatchNotFound(patch));
        }

        let strip_level = guess_strip_level(&patch, work_dir)?;

        let output = system_tools
            .call(Tool::Patch)
            .map_err(|_| SourceError::PatchExeNotFound)?
            .arg(format!("-p{}", strip_level))
            .arg("-i")
            .arg(String::from(patch.to_string_lossy()))
            .arg("-d")
            .arg(String::from(work_dir.to_string_lossy()))
            .output()?;

        if !output.status.success() {
            tracing::error!("Failed to apply patch: {}", patch.to_string_lossy());
            tracing::error!("Stdout: {}", String::from_utf8_lossy(&output.stdout));
            tracing::error!("Stderr: {}", String::from_utf8_lossy(&output.stderr));
            return Err(SourceError::PatchFailed(
                patch.to_string_lossy().to_string(),
            ));
        }
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use gitpatch::Patch;

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
}
