use goblin::elf::Elf;
use itertools::Itertools;
use std::collections::HashSet;
use std::fs::{self, File};
use std::io::Read;
use std::path::{Path, PathBuf};

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

#[derive(Debug, Default)]
struct ElfModifications {
    set_rpath: Vec<PathBuf>,
}

fn call_patchelf(elf_path: &Path, modifications: &ElfModifications) -> Result<(), RelinkError> {
    // call patchelf
    tracing::info!("patchelf for {:?}: {:?}", elf_path, modifications);

    let mut cmd = std::process::Command::new("patchelf");

    let new_rpath = modifications
        .set_rpath
        .iter()
        .map(|p| p.to_string_lossy())
        .join(":");

    // conda-build forces `rpath` -> otherwise patchelf would use the newer `runpath`
    cmd.arg("--force-rpath");
    cmd.arg(new_rpath);

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

/// find all RPATH and RUNPATH entries
/// replace them with the encoded prefix
/// if the prefix is not found, add it to the end of the list
fn modify_elf(elf_path: &Path, prefix: &Path, encoded_prefix: &Path) -> Result<(), RelinkError> {
    let mut file = File::open(elf_path).unwrap();
    let mut buffer = Vec::new();
    file.read_to_end(&mut buffer).unwrap();

    let elf = Elf::parse(&buffer)?;

    // tracing::info!("ELF soname    : {:?}", elf.soname);
    // tracing::info!("ELF libraries : {:?}", elf.libraries);
    // tracing::info!("ELF RPATHS    : {:?}", elf.rpaths);
    // tracing::info!("ELF RUNPATHS  : {:?}", elf.runpaths);
    let mut modifications = ElfModifications::default();

    for rpath in elf.rpaths {
        // split rpath at colon
        rpath.split(':').for_each(|p| {
            tracing::info!("TODO handle all inner RPATH: {}", p);
        });

        if rpath.starts_with(encoded_prefix.to_string_lossy().as_ref()) {
            // remove this rpath and replace with relative path
            tracing::info!("Found encoded rpath: {}", rpath);
            let r = PathBuf::from(rpath);
            let stripped = r.strip_prefix(encoded_prefix)?;
            let new_path = prefix.join(stripped);

            let relative_path = pathdiff::diff_paths(new_path, elf_path.parent().unwrap()).unwrap();
            tracing::info!("New relative path: $ORIGIN/{}", relative_path.display());
            modifications.set_rpath.push(PathBuf::from(format!(
                "$ORIGIN/{}",
                relative_path.to_string_lossy()
            )));
        } else {
            modifications.set_rpath.push(PathBuf::from(rpath));
        }
    }

    for runpath in elf.runpaths {
        if runpath.starts_with(encoded_prefix.to_string_lossy().as_ref()) {
            // remove this rpath and replace with relative path
            tracing::info!("Found encoded runpath: {}", runpath);
            let r = PathBuf::from(runpath);
            let stripped = r.strip_prefix(encoded_prefix)?;
            let new_path = prefix.join(stripped);

            let relative_path = pathdiff::diff_paths(new_path, elf_path.parent().unwrap()).unwrap();
            tracing::info!("New relative path: $ORIGIN/{}", relative_path.display());
            modifications.set_rpath.push(PathBuf::from(format!(
                "$ORIGIN/{}",
                relative_path.to_string_lossy()
            )));
        } else {
            modifications.set_rpath.push(PathBuf::from(runpath));
        }
    }

    // keep only first unique entries
    modifications.set_rpath = modifications.set_rpath.into_iter().unique().collect();

    call_patchelf(elf_path, &modifications)?;

    Ok(())
}

pub fn relink_paths(
    paths: &HashSet<PathBuf>,
    prefix: &Path,
    encoded_prefix: &Path,
) -> Result<(), RelinkError> {
    for p in paths {
        if fs::symlink_metadata(p)?.is_symlink() {
            tracing::info!("Skipping symlink: {}", p.display());
            continue;
        }

        if let Some(ext) = p.extension() {
            if ext.to_string_lossy() == "so" {
                match modify_elf(p, prefix, encoded_prefix) {
                    Ok(_) => {}
                    Err(e) => {
                        tracing::error!("Error: {}", e);
                    }
                }
            }
        } else if p.parent().unwrap().ends_with("bin") {
            match modify_elf(p, prefix, encoded_prefix) {
                Ok(_) => {}
                Err(e) => {
                    tracing::error!("Error: {}", e);
                }
            }
        }
    }

    Ok(())
}
