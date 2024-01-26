//! Functions to post-process packages after building
//! This includes:
//!
//! - relinking of shared libraries to be relocatable
//! - checking for "overlinking" (i.e. linking to libraries that are not dependencies
//!   of the package, or linking to system libraries that are not part of the allowed list)

use fs_err as fs;
use std::{collections::HashSet, path::PathBuf};

use rattler_conda_types::PackageName;

use crate::packaging::PackagingError;

/// Find any .dist-info/INSTALLER files and replace the contents with "conda"
/// This is to prevent pip from trying to uninstall the package when it is installed with conda
pub fn python(
    name: &PackageName,
    version: &str,
    paths: &HashSet<PathBuf>,
) -> Result<(), PackagingError> {
    let metadata_glob = globset::Glob::new("**/*.dist-info/METADATA")?.compile_matcher();

    if let Some(p) = paths.iter().find(|p| metadata_glob.is_match(p)) {
        // unwraps are OK because we already globbed
        let distinfo = p
            .parent()
            .expect("Should never fail to get parent because we already globbed")
            .file_name()
            .expect("Should never fail to get file name because we already globbed")
            .to_string_lossy()
            .to_lowercase();
        if distinfo.starts_with(name.as_normalized())
            && distinfo != format!("{}-{}.dist-info", name.as_normalized(), version)
        {
            tracing::warn!(
                "Found dist-info folder with incorrect name or version: {}",
                distinfo
            );
        }
    }

    let glob = globset::Glob::new("**/*.dist-info/INSTALLER")?.compile_matcher();
    for p in paths {
        if glob.is_match(p) {
            fs::write(p, "conda\n")?;
        }
    }

    Ok(())
}
