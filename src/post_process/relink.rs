use fs_err as fs;

use crate::metadata::Output;
use crate::packaging::TempFiles;

use crate::linux::link::SharedObject;
use crate::macos::link::Dylib;
use crate::recipe::parser::GlobVec;
use crate::system_tools::{SystemTools, ToolError};
use crate::windows::link::Dll;
use rattler_conda_types::{Arch, Platform};
use std::collections::{HashMap, HashSet};
use std::path::{Path, PathBuf};
use thiserror::Error;

use super::checks::{LinkingCheckError, perform_linking_checks};

#[derive(Error, Debug)]
#[allow(missing_docs)]
pub enum RelinkError {
    #[error("linking check error: {0}")]
    LinkingCheck(#[from] LinkingCheckError),

    #[error("failed to run install_name_tool")]
    InstallNameToolFailed,

    #[error("Codesign failed")]
    CodesignFailed,

    #[error(transparent)]
    SystemToolError(#[from] ToolError),

    #[error("failed to read or write file: {0}")]
    IoError(#[from] std::io::Error),

    #[error("failed to strip prefix from path: {0}")]
    StripPrefixError(#[from] std::path::StripPrefixError),

    #[error("failed to parse dynamic file: {0}")]
    ParseError(#[from] goblin::error::Error),

    #[error("filetype not handled")]
    FileTypeNotHandled,

    #[error("could not read string from MachO file: {0}")]
    ReadStringError(#[from] scroll::Error),

    #[error("failed to get relative path from {from} to {to}")]
    PathDiffFailed { from: PathBuf, to: PathBuf },

    #[error("failed to relink with built-in relinker")]
    BuiltinRelinkFailed,

    #[error("shared library has no parent directory")]
    NoParentDir,

    #[error("failed to run patchelf")]
    PatchElfFailed,

    #[error("rpath not found in dynamic section")]
    RpathNotFound,

    #[error("unknown platform for relinking")]
    UnknownPlatform,

    #[error("unknown file format for relinking")]
    UnknownFileFormat,
}

/// Platform specific relinker.
pub trait Relinker {
    /// Returns true if the file is valid (i.e. ELF or Mach-o)
    fn test_file(path: &Path) -> Result<bool, RelinkError>
    where
        Self: Sized;

    /// Creates a new relinker.
    fn new(path: &Path) -> Result<Self, RelinkError>
    where
        Self: Sized;

    /// Returns the shared libraries.
    #[allow(dead_code)]
    fn libraries(&self) -> HashSet<PathBuf>;

    /// Find libraries in the shared library and resolve them by taking into account the rpaths.
    fn resolve_libraries(
        &self,
        prefix: &Path,
        encoded_prefix: &Path,
    ) -> HashMap<PathBuf, Option<PathBuf>>;

    /// Resolve the rpath with the path of the dylib.
    fn resolve_rpath(&self, rpath: &Path, prefix: &Path, encoded_prefix: &Path) -> PathBuf;

    /// Relinks the file.
    fn relink(
        &self,
        prefix: &Path,
        encoded_prefix: &Path,
        custom_rpaths: &[String],
        rpath_allowlist: &GlobVec,
        system_tools: &SystemTools,
    ) -> Result<(), RelinkError>;
}

/// Returns true if the file is valid (i.e. ELF or Mach-o or PE)
pub fn is_valid_file(platform: Platform, path: &Path) -> Result<bool, RelinkError> {
    if platform.is_linux() {
        SharedObject::test_file(path)
    } else if platform.is_osx() {
        Dylib::test_file(path)
    } else if platform.is_windows() {
        Dll::test_file(path)
    } else {
        Err(RelinkError::UnknownPlatform)
    }
}

/// Returns the relink helper for the current platform.
pub fn get_relinker(platform: Platform, path: &Path) -> Result<Box<dyn Relinker>, RelinkError> {
    if !is_valid_file(platform, path)? {
        return Err(RelinkError::UnknownFileFormat);
    }
    if platform.is_linux() {
        Ok(Box::new(SharedObject::new(path)?))
    } else if platform.is_osx() {
        Ok(Box::new(Dylib::new(path)?))
    } else if platform.is_windows() {
        Ok(Box::new(Dll::new(path)?))
    } else {
        Err(RelinkError::UnknownPlatform)
    }
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
        // skip linking checks for wasm
        || target_platform.arch() == Some(Arch::Wasm32)
        || relocation_config.is_none()
    {
        return Ok(());
    }

    let rpaths = dynamic_linking.rpaths();
    let rpath_allowlist = dynamic_linking.rpath_allowlist();

    let tmp_prefix = temp_files.temp_dir.path();
    let encoded_prefix = &temp_files.encoded_prefix;

    let mut binaries = HashSet::new();
    // allow to use tools from build prefix such as patchelf, install_name_tool, ...
    let system_tools = output.system_tools.with_build_prefix(output.build_prefix());

    for (p, content_type) in temp_files.content_type_map() {
        let metadata = fs::symlink_metadata(p)?;
        if metadata.is_symlink() || metadata.is_dir() {
            tracing::debug!("Relink skipping symlink or directory: {}", p.display());
            continue;
        }

        if content_type != &Some(content_inspector::ContentType::BINARY) {
            continue;
        }

        if !relocation_config.is_match(p) {
            continue;
        }
        if is_valid_file(target_platform, p)? {
            let relinker = get_relinker(target_platform, p)?;
            if !target_platform.is_windows() {
                relinker.relink(
                    tmp_prefix,
                    encoded_prefix,
                    &rpaths,
                    rpath_allowlist,
                    &system_tools,
                )?;
            }
            binaries.insert(p.clone());
        }
    }
    perform_linking_checks(output, &binaries, tmp_prefix)?;

    Ok(())
}
