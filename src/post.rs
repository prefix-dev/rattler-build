//! Functions to post-process packages after building
//! This includes:
//!
//! - relinking of shared libraries to be relocatable
//! - checking for "overlinking" (i.e. linking to libraries that are not dependencies
//!   of the package, or linking to system libraries that are not part of the allowed list)

use std::{
    collections::HashSet,
    fs::{self, File},
    io::Read,
    path::{Path, PathBuf},
};

use rattler_conda_types::Platform;

use crate::{linux::link::SharedObject, macos::link::Dylib};

pub fn relink(
    paths: &HashSet<PathBuf>,
    prefix: &Path,
    encoded_prefix: &Path,
    target_platform: &Platform,
) -> Result<(), Box<dyn std::error::Error>> {
    for p in paths {
        let metadata = fs::symlink_metadata(p)?;
        if metadata.is_symlink() || metadata.is_dir() {
            tracing::info!("Relink skipping symlink or directory: {}", p.display());
            continue;
        }

        // Skip files that are not binaries
        let mut buffer = vec![0; 1024];
        let mut file = File::open(p)?;
        let n = file.read(&mut buffer)?;
        let buffer = &buffer[..n];

        let content_type = content_inspector::inspect(buffer);
        if content_type != content_inspector::ContentType::BINARY {
            continue;
        }

        if target_platform.is_linux() {
            if SharedObject::test_file(p)? {
                let so = SharedObject::new(p)?;
                so.relink(prefix, encoded_prefix)?;
            }
        } else if target_platform.is_osx() && Dylib::test_file(p)? {
            let dylib = Dylib::new(p)?;
            dylib.relink(prefix, encoded_prefix)?;
        }
    }

    Ok(())
}
