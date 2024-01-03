use fs_err as fs;
use std::fs::File;
use std::io::Read;
use std::{
    collections::HashSet,
    path::{Path, PathBuf},
};

use globset::GlobMatcher;
use rattler_conda_types::Platform;

use crate::{linux::link::SharedObject, macos::link::Dylib};

#[derive(Debug, thiserror::Error)]
pub enum RelinkError {
    #[error("Error reading file: {0}")]
    Io(#[from] std::io::Error),

    #[error("Error relinking shared object: {0}")]
    SharedObject(#[from] crate::linux::link::RelinkError),

    #[error("Error relinking dylib: {0}")]
    Dylib(#[from] crate::macos::link::RelinkError),
}

/// Relink dynamic libraries in the given paths to be relocatable
/// This function first searches for any dynamic libraries (ELF or Mach-O) in the given paths,
/// and then relinks them by changing the rpath to make them easily relocatable.
///
/// ### What is an "rpath"?
///
/// The rpath is a list of paths that are searched for shared libraries when a program is run.
/// For example, if a program links to `libfoo.so`, the rpath is searched for `libfoo.so`.
/// If the rpath is not set, the system library paths are searched.
///
/// ### Relinking
///
/// On Linux (ELF files) we relink the executables or shared libraries by setting the `rpath` to something that is relative to
/// the library or executable location with the special `$ORIGIN` variable. The change is applied with the `patchelf` executable.
/// For example, any rpath that starts with `/just/some/folder/_host_prefix/lib` will be changed to `$ORIGIN/../lib`.
///
/// On macOS (Mach-O files), we do the same trick and set the rpath to a relative path with the special
/// `@loader_path` variable. The change for Mach-O files is applied with the `install_name_tool`.
pub fn relink(
    paths: &HashSet<PathBuf>,
    prefix: &Path,
    encoded_prefix: &Path,
    target_platform: &Platform,
    rpath_allowlist: &[GlobMatcher],
) -> Result<(), RelinkError> {
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
                so.relink(prefix, encoded_prefix, rpath_allowlist)?;
            }
        } else if target_platform.is_osx() && Dylib::test_file(p)? {
            let dylib = Dylib::new(p)?;
            dylib.relink(prefix, encoded_prefix)?;
        }
    }

    Ok(())
}
