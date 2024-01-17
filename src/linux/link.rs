//! Relink shared objects to use an relative path prefix
use globset::GlobMatcher;
use goblin::elf::{Dyn, Elf};
use goblin::elf64::header::ELFMAG;
use goblin::strtab::Strtab;
use itertools::Itertools;
use memmap2::MmapMut;
use scroll::ctx::SizeWith;
use scroll::Pwrite;
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
    pub fn relink(
        &self,
        prefix: &Path,
        encoded_prefix: &Path,
        rpath_allowlist: &[GlobMatcher],
    ) -> Result<(), RelinkError> {
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
            } else if rpath_allowlist.iter().any(|glob| glob.is_match(rpath)) {
                tracing::info!("rpath ({:?}) for {:?} found in allowlist", rpath, self.path);
                final_rpath.push(rpath.clone());
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

#[allow(dead_code)]
fn relink(elf_path: &Path, new_rpath: &[PathBuf]) -> Result<(), RelinkError> {
    let new_rpath = new_rpath.iter().map(|p| p.to_string_lossy()).join(":");

    // if have runpath & rpath, delete runpath
    // if only runpath, make rpath = runpath
    // if only rpath, just rewrite the rpath

    let file = std::fs::OpenOptions::new()
        .read(true)
        .write(true)
        .open(&elf_path)
        .unwrap();

    let data = unsafe { memmap2::Mmap::map(&file) }.unwrap();

    let object = goblin::elf::Elf::parse(&data)?;

    let ctx = goblin::container::Ctx {
        container: object
            .is_64
            .then_some(goblin::container::Container::Big)
            .unwrap_or(goblin::container::Container::Little),
        le: object
            .little_endian
            .then_some(goblin::container::Endian::Little)
            .unwrap_or(goblin::container::Endian::Big),
    };

    if object.dynamic.is_none() {
        return Ok(());
    }

    let dynamic = object.dynamic.unwrap();

    let Ok(dynstrtab) = Strtab::parse(&data, dynamic.info.strtab, dynamic.info.strsz, 0x0) else {
        todo!()
    };

    // reopen to please the borrow checker
    let data = unsafe { memmap2::Mmap::map(&file) }.unwrap();

    let has_rpath = dynamic
        .dyns
        .iter()
        .any(|entry| entry.d_tag == goblin::elf::dynamic::DT_RPATH);

    let mut data_mut = data.make_mut().expect("Failed to make data mutable");

    let overwrite_strtab =
        |data_mut: &mut MmapMut, offset: usize, new_value: &str| -> Result<(), RelinkError> {
            let new_value = new_value.as_bytes();
            let old_value = dynstrtab.get_at(offset).unwrap();

            if new_value.len() > old_value.len() {
                panic!("new value is longer than old value");
            }

            let offset = offset + dynamic.info.strtab as usize;
            data_mut[offset..offset + new_value.len()].copy_from_slice(new_value);
            // pad with null bytes
            data_mut[offset + new_value.len()..offset + old_value.len()].fill(0);

            Ok(())
        };

    let mut new_dynamic = Vec::new();
    let mut push_to_end = Vec::new();

    for entry in dynamic.dyns.iter() {
        if entry.d_tag == goblin::elf::dynamic::DT_RPATH {
            overwrite_strtab(&mut data_mut, entry.d_val as usize, &new_rpath)?;
            new_dynamic.push(entry.clone());
        } else if entry.d_tag == goblin::elf::dynamic::DT_RUNPATH {
            if has_rpath {
                // todo: clear value from strtab to avoid any mentions of placeholders in the binary
                overwrite_strtab(&mut data_mut, entry.d_val as usize, "")?;
                push_to_end.push(Dyn {
                    d_tag: goblin::elf::dynamic::DT_RPATH,
                    d_val: entry.d_val,
                });
            } else {
                let mut new_entry = entry.clone();
                new_entry.d_tag = goblin::elf::dynamic::DT_RPATH;
                new_dynamic.push(new_entry);
            }
        } else {
            new_dynamic.push(entry.clone());
        }
    }
    // add empty entries to the end to keep offsets correct
    new_dynamic.extend(push_to_end);

    // now we need to write the new dynamic section
    let phdr_offset = object
        .program_headers
        .iter()
        .find(|phdr| phdr.p_type == goblin::elf::program_header::PT_DYNAMIC)
        .map(|hdr_dynamic| hdr_dynamic.p_offset)
        .unwrap() as usize;

    let mut offset = phdr_offset;
    for d in new_dynamic {
        data_mut.pwrite_with::<goblin::elf::dynamic::Dyn>(d, offset, ctx)?;
        offset += goblin::elf::dynamic::Dyn::size_with(&ctx);
    }

    tracing::info!("Patched dynamic section of {:?}", elf_path);

    data_mut.flush().expect("bla");

    Ok(())
}

#[cfg(test)]
mod test {
    use super::*;
    use globset::Glob;
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
        object.relink(
            &prefix,
            encoded_prefix,
            &[Glob::new("/usr/lib/custom**").unwrap().compile_matcher()],
        )?;
        let object = SharedObject::new(&binary_path)?;
        assert_eq!(
            vec!["$ORIGIN/../lib", "/usr/lib/custom_lib"],
            object
                .rpaths
                .iter()
                .flat_map(|r| r.split(':'))
                .collect::<Vec<&str>>()
        );

        // manually clean up temporary directory because it was
        // persisted to disk by calling `into_path`
        fs::remove_dir_all(tmp_dir)?;

        Ok(())
    }

    #[test]
    fn relink_builtin() -> Result<(), RelinkError> {
        // copy binary to a temporary directory
        let prefix = Path::new(env!("CARGO_MANIFEST_DIR")).join("test-data/binary_files");
        let tmp_dir = tempdir_in(&prefix)?;
        let binary_path = tmp_dir.path().join("zlink");
        fs::copy(prefix.join("zlink"), &binary_path)?;

        super::relink(
            &binary_path,
            vec![
                PathBuf::from("$ORIGIN/../lib"),
                PathBuf::from("/usr/lib/custom_lib"),
            ]
            .as_slice(),
        )?;

        let object = SharedObject::new(&binary_path)?;
        assert_eq!(
            vec!["$ORIGIN/../lib", "/usr/lib/custom_lib"],
            object
                .rpaths
                .iter()
                .flat_map(|r| r.split(':'))
                .collect::<Vec<&str>>()
        );

        Ok(())
    }
}
