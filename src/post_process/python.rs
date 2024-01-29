//! Functions to post-process packages after building
//! This includes:
//!
//! - relinking of shared libraries to be relocatable
//! - checking for "overlinking" (i.e. linking to libraries that are not dependencies
//!   of the package, or linking to system libraries that are not part of the allowed list)

use fs_err as fs;
use std::{collections::HashSet, path::{Component, Path, PathBuf}};

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

pub fn filter_file(path: &Path, noarch_python: bool) -> bool {
    let ext = path.extension().unwrap_or_default();
    // pyo considered harmful: https://www.python.org/dev/peps/pep-0488/
    if ext == "pyo" {
        return true; // skip .pyo files
    }

    if ext == "py" || ext == "pyc" {
        // if we have a .so file of the same name, skip this path
        let so_path = path.with_extension("so");
        let pyd_path = path.with_extension("pyd");
        if so_path.exists() || pyd_path.exists() {
            return true;
        }
    }

    if noarch_python {
        // skip .pyc or .pyo or .egg-info files
        if ["pyc", "egg-info", "pyo"].iter().any(|s| ext.eq(*s)) {
            return true; // skip .pyc files
        }

        // if any part of the path is __pycache__ skip it
        if path
            .components()
            .any(|c| c == Component::Normal("__pycache__".as_ref()))
        {
            return true;
        }
    }

    false
}