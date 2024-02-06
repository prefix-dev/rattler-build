//! Functions for applying patches to a work directory.
use std::path::{Path, PathBuf};

use super::SourceError;
use crate::system_tools::{SystemTools, Tool};

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

        let output = system_tools
            .call(Tool::Patch)
            .map_err(|_| SourceError::PatchNotFound)?
            .arg("-p1")
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
