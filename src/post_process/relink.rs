use fs_err as fs;
use std::collections::HashSet;

use rattler_conda_types::Platform;

use crate::metadata::Output;
use crate::packaging::TempFiles;
use crate::{linux::link::SharedObject, macos::link::Dylib};

use super::{linking_checks, LinkingCheckError};

#[derive(Debug, thiserror::Error)]
pub enum RelinkError {
    #[error("Error reading file: {0}")]
    Io(#[from] std::io::Error),

    #[error("Error relinking shared object: {0}")]
    SharedObject(#[from] crate::linux::link::RelinkError),

    #[error("Error relinking dylib: {0}")]
    Dylib(#[from] crate::macos::link::RelinkError),

    #[error("Linking check error: {0}")]
    LinkingCheck(#[from] LinkingCheckError),
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
pub fn relink(temp_files: &TempFiles, output: &Output) -> Result<(), RelinkError> {
    let dynamic_linking = output.recipe.build().dynamic_linking();
    let target_platform = output.build_configuration.target_platform;
    let relocation_config = dynamic_linking.binary_relocation();

    if target_platform == Platform::NoArch
        || target_platform.is_windows()
        || relocation_config.is_none()
    {
        return Ok(());
    }

    let rpaths = dynamic_linking.rpaths();
    let rpath_allowlist = dynamic_linking.rpath_allowlist();

    let tmp_prefix = temp_files.temp_dir.path();
    let encoded_prefix = &temp_files.encoded_prefix;

    let mut binaries = HashSet::new();

    for (p, content_type) in &temp_files.content_type_map {
        let metadata = fs::symlink_metadata(p)?;
        if metadata.is_symlink() || metadata.is_dir() {
            tracing::debug!("Relink skipping symlink or directory: {}", p.display());
            continue;
        }

        if content_type != &content_inspector::ContentType::BINARY {
            continue;
        }

        if !relocation_config.is_match(p) {
            continue;
        }

        if target_platform.is_linux() && SharedObject::test_file(p)? {
            let so = SharedObject::new(p)?;
            so.relink(tmp_prefix, encoded_prefix, &rpaths, rpath_allowlist)?;
            binaries.insert(p.clone());
        } else if target_platform.is_osx() && Dylib::test_file(p)? {
            let dylib = Dylib::new(p)?;
            dylib.relink(tmp_prefix, encoded_prefix, &rpaths, rpath_allowlist)?;
            binaries.insert(p.clone());
        }
    }

    linking_checks(output, &binaries)?;

    Ok(())
}
