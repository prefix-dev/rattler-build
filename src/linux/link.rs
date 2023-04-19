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
}

#[derive(thiserror::Error, Debug)]
pub enum RelinkError {
    #[error("failed to run patchelf")]
    PatchElfFailed,

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
        })
    }

    /// find all RPATH and RUNPATH entries
    /// replace them with the encoded prefix
    /// if the prefix is not found, add it to the end of the list
    pub fn relink(&self, prefix: &Path, encoded_prefix: &Path) -> Result<(), RelinkError> {
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
            if rpath.starts_with(encoded_prefix) {
                let rel = rpath.strip_prefix(encoded_prefix)?;
                let new_rpath = prefix.join(rel);

                let relative_path =
                    pathdiff::diff_paths(new_rpath, self.path.parent().unwrap()).unwrap();
                tracing::info!("New relative path: $ORIGIN/{}", relative_path.display());
                final_rpath.push(PathBuf::from(format!(
                    "$ORIGIN/{}",
                    relative_path.to_string_lossy()
                )));
            } else {
                final_rpath.push(rpath.clone());
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

    let mut cmd = std::process::Command::new("patchelf");

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
