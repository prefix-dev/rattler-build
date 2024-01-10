//! Relink shared objects to use an relative path prefix
use goblin::elf::Elf;
use goblin::elf64::header::ELFMAG;
use itertools::Itertools;
use std::collections::HashSet;
use std::fs::File;
use std::io::Read;
use std::path::{Path, PathBuf};

/// A linux shared object (ELF)
pub struct SharedObject {
    /// Path to the shared object
    pub path: PathBuf,
    /// Libraries that this shared object depends on
    pub libraries: HashSet<String>,
    /// RPATH entries
    pub rpaths: Vec<String>,
    /// RUNPATH entries
    pub runpaths: Vec<String>,
    /// Whether the shared object is dynamically linked
    pub has_dynamic: bool,
}

#[derive(thiserror::Error, Debug)]
pub enum RelinkError {
    #[error("non-absolute or non-normalized base path")]
    PathDiffFailed,

    #[error("failed to get parent directory")]
    NoParentDir,

    #[error("failed to run patchelf")]
    PatchElfFailed,

    #[error("failed to find patchelf: please install patchelf on your system")]
    PatchElfNotFound(#[from] which::Error),

    #[error("failed to read or write elf file: {0}")]
    IoError(#[from] std::io::Error),

    #[error("failed to strip prefix from path: {0}")]
    StripPrefixError(#[from] std::path::StripPrefixError),

    #[error("failed to parse elf file: {0}")]
    ParseElfError(#[from] goblin::error::Error),
}

impl SharedObject {
    /// Check if the file is an ELF file by reading the first 4 bytes
    pub fn test_file(path: &Path) -> Result<bool, std::io::Error> {
        let mut file = File::open(path)?;
        let mut signature: [u8; 4] = [0; 4];
        file.read_exact(&mut signature)?;
        Ok(ELFMAG.iter().eq(signature.iter()))
    }

    /// Create a new shared object from a path
    pub fn new(path: &Path) -> Result<Self, RelinkError> {
        let mut buffer = Vec::new();
        let mut file = File::open(path).expect("Failed to open the DLL file");
        file.read_to_end(&mut buffer)
            .expect("Failed to read the DLL file");
        let elf = Elf::parse(&buffer).expect("Failed to parse the ELF file");

        Ok(Self {
            path: path.to_path_buf(),
            libraries: elf.libraries.iter().map(|s| s.to_string()).collect(),
            rpaths: elf.rpaths.iter().map(|s| s.to_string()).collect(),
            runpaths: elf.runpaths.iter().map(|s| s.to_string()).collect(),
            has_dynamic: elf.dynamic.is_some(),
        })
    }

    /// find all RPATH and RUNPATH entries
    /// replace them with the encoded prefix
    /// if the rpath is outside of the prefix, it is removed
    pub fn relink(&self, prefix: &Path, encoded_prefix: &Path) -> Result<(), RelinkError> {
        if !self.has_dynamic {
            tracing::debug!("{} is not dynamically linked", self.path.display());
            return Ok(());
        }

        let rpaths = self
            .rpaths
            .iter()
            .flat_map(|r| r.split(':'))
            .map(PathBuf::from)
            .collect::<Vec<_>>();

        let runpaths = self
            .runpaths
            .iter()
            .flat_map(|r| r.split(':'))
            .map(PathBuf::from)
            .collect::<Vec<_>>();

        let mut final_rpath = Vec::new();

        for rpath in rpaths.iter().chain(runpaths.iter()) {
            if let Ok(rel) = rpath.strip_prefix(encoded_prefix) {
                let new_rpath = prefix.join(rel);
                let relative_path = pathdiff::diff_paths(
                    &new_rpath,
                    self.path.parent().ok_or(RelinkError::NoParentDir)?,
                )
                .ok_or(RelinkError::PathDiffFailed)?;
                tracing::info!("New relative path: $ORIGIN/{}", relative_path.display());
                final_rpath.push(PathBuf::from(format!(
                    "$ORIGIN/{}",
                    relative_path.to_string_lossy()
                )));
            } else {
                tracing::warn!(
                    "rpath ({:?}) is outside of prefix ({:?}) for {:?} - removing it",
                    rpath,
                    encoded_prefix,
                    self.path
                );
            }
        }

        // keep only first unique item
        final_rpath = final_rpath.into_iter().unique().collect();

        call_patchelf(&self.path, &final_rpath)?;

        Ok(())
    }
}

fn call_patchelf(elf_path: &Path, new_rpath: &[PathBuf]) -> Result<(), RelinkError> {
    let new_rpath = new_rpath.iter().map(|p| p.to_string_lossy()).join(":");

    tracing::info!("patchelf for {:?}: {:?}", elf_path, new_rpath);

    let patchelf_exe = which::which("patchelf")?;

    let mut cmd = std::process::Command::new(patchelf_exe);

    // prefer using RPATH over RUNPATH because RPATH takes precedence when
    // searching for shared libraries and cannot be overriden with
    // `LD_LIBRARY_PATH`. This ensures that the libraries from the environment
    // are found first, providing better isolation and preventing potential
    // conflicts with system libraries.
    cmd.arg("--force-rpath");

    // set the new rpath
    cmd.arg("--set-rpath").arg(new_rpath).arg(elf_path);

    let output = cmd.output()?;
    if !output.status.success() {
        tracing::error!(
            "patchelf failed: {}",
            String::from_utf8_lossy(&output.stderr)
        );
        Err(RelinkError::PatchElfFailed)
    } else {
        Ok(())
    }
}

#[cfg(test)]
#[cfg(target_os = "linux")]
mod test {
    use super::*;
    use std::{fs, path::Path};
    use tempfile::tempdir_in;

    // Assert the following case:
    //
    // rpath: "/rattler-build_zlink/host_env_placehold/lib"
    // encoded prefix: "/rattler-build_zlink/host_env_placehold"
    // binary path: test-data/binary_files/tmp/zlink
    // prefix: "test-data/binary_files"
    // new rpath: $ORIGIN/../lib
    #[test]
    fn relink() -> Result<(), RelinkError> {
        // copy binary to a temporary directory
        let prefix = Path::new(env!("CARGO_MANIFEST_DIR")).join("test-data/binary_files");
        let tmp_dir = tempdir_in(&prefix)?.into_path();
        let binary_path = tmp_dir.join("zlink");
        fs::copy(prefix.join("zlink"), &binary_path)?;

        // default rpaths of the test binary are:
        // - /rattler-build_zlink/host_env_placehold/lib
        // - /rattler-build_zlink/build_env/lib
        // so we are expecting it to keep the host prefix and discard the build prefix
        let encoded_prefix = Path::new("/rattler-build_zlink/host_env_placehold");
        let object = SharedObject::new(&binary_path)?;
        object.relink(&prefix, encoded_prefix)?;
        let object = SharedObject::new(&binary_path)?;
        assert_eq!(vec!["$ORIGIN/../lib"], object.rpaths);

        // manually clean up temporary directory because it was
        // persisted to disk by calling `into_path`
        fs::remove_dir_all(tmp_dir)?;

        Ok(())
    }
}
