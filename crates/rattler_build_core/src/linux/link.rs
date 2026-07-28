//! Relink shared objects to use an relative path prefix

use goblin::elf::header::{ELFCLASS32, ELFCLASS64, ET_DYN, ET_EXEC, et_to_str, header32, header64};
use goblin::elf::{Dyn, Elf};
use goblin::strtab::Strtab;
use itertools::Itertools;
use memmap2::MmapMut;
use rattler_build_recipe::stage1::GlobVec;
use rattler_conda_types::Platform;
use scroll::Pwrite;
use scroll::ctx::SizeWith;
use std::collections::{HashMap, HashSet};
use std::fs::File;
use std::io::Read;
use std::path::{Path, PathBuf};

use crate::post_process::relink::{RelinkError, Relinker};
use crate::system_tools::{SystemTools, Tool};
use crate::unix::permission_guard::{PermissionGuard, READ_WRITE};
use crate::utils::to_lexical_absolute;

/// A linux shared object (ELF)
#[derive(Debug)]
pub struct SharedObject {
    /// Path to the shared object
    path: PathBuf,
    /// Libraries that this shared object depends on
    libraries: HashSet<PathBuf>,
    /// RPATH entries
    rpaths: Vec<String>,
    /// RUNPATH entries
    runpaths: Vec<String>,
    /// Whether the shared object is dynamically linked
    has_dynamic: bool,
    /// Whether to record the search path as `DT_RUNPATH` instead of `DT_RPATH`.
    ///
    /// This is set for Android targets, whose dynamic linker never implemented
    /// `DT_RPATH`: bionic handles `DT_RUNPATH` only, and warns about anything else
    /// ("unused DT entry ... (ignoring)"). Writing `DT_RPATH` there leaves the binary
    /// with no search path at all, which cannot be seen until it fails to load on a
    /// device.
    use_runpath: bool,
}

impl SharedObject {
    /// Creates a new shared object relinker for a specific target platform.
    ///
    /// Prefer this over [`Relinker::new`] when the target platform is known, so that
    /// Android binaries get `DT_RUNPATH` rather than a `DT_RPATH` their linker ignores.
    pub fn new_for_platform(path: &Path, target_platform: Platform) -> Result<Self, RelinkError> {
        Ok(Self {
            use_runpath: target_platform.is_android(),
            ..<Self as Relinker>::new(path)?
        })
    }
}

impl Relinker for SharedObject {
    /// Check if the file is an ELF file by reading the first 4 bytes
    fn test_file(path: &Path) -> Result<bool, RelinkError> {
        let mut file = File::open(path)?;

        // Read enough bytes for the largest possible header (64-bit)
        let mut header_buf = [0u8; header64::SIZEOF_EHDR];
        match file.read_exact(&mut header_buf) {
            Ok(_) => {}
            Err(e) if e.kind() == std::io::ErrorKind::UnexpectedEof => return Ok(false),
            Err(e) => return Err(e.into()),
        }

        // Check ELF magic
        if &header_buf[0..4] != goblin::elf64::header::ELFMAG {
            return Ok(false);
        }

        // Determine if it's 32 or 64 bit and parse accordingly
        let e_type = match header_buf[4] {
            // EI_CLASS
            ELFCLASS32 => {
                let header_bytes: &[u8; header32::SIZEOF_EHDR] = header_buf
                    [..header32::SIZEOF_EHDR]
                    .try_into()
                    .expect("Invalid header size for ELFCLASS32");
                header32::Header::from_bytes(header_bytes).e_type
            }
            ELFCLASS64 => {
                let header_bytes: &[u8; header64::SIZEOF_EHDR] = &header_buf; // Already the right size
                header64::Header::from_bytes(header_bytes).e_type
            }
            _ => return Ok(false), // Invalid ELF class
        };

        // Only process executables and shared libraries
        let should_relink = matches!(e_type, ET_EXEC | ET_DYN);

        if !should_relink {
            tracing::debug!(
                "Skipping ELF file with type {}: {}",
                et_to_str(e_type),
                path.display()
            );
        }

        Ok(should_relink)
    }

    /// Create a new shared object from a path
    fn new(path: &Path) -> Result<Self, RelinkError> {
        let file = File::open(path).expect("Failed to open the ELF file");
        let mmap = unsafe { memmap2::Mmap::map(&file)? };
        let elf = Elf::parse(&mmap).expect("Failed to parse the ELF file");
        Ok(Self {
            path: path.to_path_buf(),
            libraries: elf.libraries.iter().map(PathBuf::from).collect(),
            rpaths: elf.rpaths.iter().map(|s| s.to_string()).collect(),
            runpaths: elf.runpaths.iter().map(|s| s.to_string()).collect(),
            has_dynamic: elf.dynamic.is_some(),
            use_runpath: false,
        })
    }

    /// Returns the shared libraries contained in the file.
    fn libraries(&self) -> HashSet<PathBuf> {
        self.libraries.clone()
    }

    /// Resolve the libraries, taking into account the rpath / runpath of the binary
    fn resolve_libraries(
        &self,
        prefix: &Path,
        encoded_prefix: &Path,
    ) -> HashMap<PathBuf, Option<PathBuf>> {
        // TODO this does not yet check in the global / system library paths
        let resolved_rpaths = self
            .rpaths
            .iter()
            .flat_map(|r| std::env::split_paths(r))
            .map(|r| self.resolve_rpath(&r, prefix, encoded_prefix))
            .collect::<Vec<_>>();

        let resolved_runpaths = self
            .runpaths
            .iter()
            .flat_map(|r| std::env::split_paths(r))
            .map(|r| self.resolve_rpath(&r, prefix, encoded_prefix))
            .collect::<Vec<_>>();

        let mut libraries = HashMap::new();
        for library in self.libraries.iter() {
            let library_path = Path::new(library);
            let resolved_library_path = if library_path.is_absolute() {
                Some(library_path.to_path_buf())
            } else {
                let library_path = Path::new(library);
                let mut resolved_library_path = None;
                // Note: this treats rpaths and runpaths equally, which is not quite correct
                for rpath in resolved_rpaths.iter().chain(resolved_runpaths.iter()) {
                    let candidate = rpath.join(library_path);
                    if candidate.exists() {
                        resolved_library_path = Some(candidate.canonicalize().unwrap_or(candidate));
                        break;
                    }
                }
                resolved_library_path
            };
            libraries.insert(library_path.to_path_buf(), resolved_library_path);
        }

        libraries
    }

    /// Resolve the rpath with the path of the dylib
    fn resolve_rpath(&self, rpath: &Path, prefix: &Path, encoded_prefix: &Path) -> PathBuf {
        // get self path in "encoded prefix"
        let self_path = encoded_prefix.join(
            self.path
                .strip_prefix(prefix)
                .expect("library not in prefix"),
        );
        if let Ok(rpath_without_loader) = rpath
            .strip_prefix("$ORIGIN")
            .or_else(|_| rpath.strip_prefix("${ORIGIN}"))
        {
            if let Some(library_parent) = self_path.parent() {
                return to_lexical_absolute(rpath_without_loader, library_parent);
            } else {
                tracing::warn!("shared library {:?} has no parent directory", self.path);
            }
        }
        rpath.to_path_buf()
    }

    /// Find all RPATH and RUNPATH entries and replace them with the encoded prefix.
    ///
    /// If the rpath is outside of the prefix, it is removed.
    fn relink(
        &self,
        prefix: &Path,
        encoded_prefix: &Path,
        custom_rpaths: &[String],
        rpath_allowlist: &GlobVec,
        system_tools: &SystemTools,
    ) -> Result<(), RelinkError> {
        if !self.has_dynamic {
            tracing::info!("{} is not dynamically linked", self.path.display());
            return Ok(());
        }

        let mut rpaths = self
            .rpaths
            .iter()
            .flat_map(|r| r.split(':'))
            .filter(|r| !r.is_empty())
            .map(PathBuf::from)
            .collect::<Vec<_>>();
        rpaths.extend(
            custom_rpaths
                .iter()
                .map(|v| encoded_prefix.join(v))
                .collect::<Vec<PathBuf>>(),
        );

        let runpaths = self
            .runpaths
            .iter()
            .flat_map(|r| r.split(':'))
            .filter(|r| !r.is_empty())
            .map(PathBuf::from)
            .collect::<Vec<_>>();

        let mut final_rpaths = Vec::new();

        for rpath in rpaths.iter().chain(runpaths.iter()) {
            if rpath.starts_with("$ORIGIN") || rpath.starts_with("${ORIGIN}") {
                let resolved = self.resolve_rpath(rpath, prefix, encoded_prefix);
                if resolved.starts_with(encoded_prefix) {
                    final_rpaths.push(rpath.clone());
                } else if rpath_allowlist.is_match(rpath) {
                    tracing::info!("Rpath in allow list: {}", rpath.display());
                    final_rpaths.push(rpath.clone());
                } else {
                    tracing::info!(
                        "Rpath not in prefix or allow-listed: {} – removing it",
                        rpath.display()
                    );
                }
            } else if let Ok(rel) = rpath.strip_prefix(encoded_prefix) {
                let new_rpath = prefix.join(rel);

                let parent = self.path.parent().ok_or(RelinkError::NoParentDir)?;

                let relative_path = pathdiff::diff_paths(&new_rpath, parent).ok_or(
                    RelinkError::PathDiffFailed {
                        from: new_rpath.clone(),
                        to: parent.to_path_buf(),
                    },
                )?;

                tracing::debug!(
                    "Converted rpath {} to $ORIGIN/{} for {:?}",
                    rpath.display(),
                    relative_path.display(),
                    self.path
                );
                final_rpaths.push(PathBuf::from(format!(
                    "$ORIGIN/{}",
                    relative_path.to_string_lossy()
                )));
            } else if rpath_allowlist.is_match(rpath) {
                tracing::info!("rpath ({:?}) for {:?} found in allowlist", rpath, self.path);
                final_rpaths.push(rpath.clone());
            } else {
                tracing::info!(
                    "rpath ({:?}) is outside of prefix ({:?}) for {:?} - removing it",
                    rpath,
                    encoded_prefix,
                    self.path
                );
            }
        }

        // keep only first unique item
        final_rpaths = final_rpaths.into_iter().unique().collect();

        let _permission_guard = PermissionGuard::new(&self.path, READ_WRITE)?;

        // run builtin relink. if it fails, try patchelf
        if builtin_relink(&self.path, &final_rpaths, self.use_runpath).is_err() {
            call_patchelf(&self.path, &final_rpaths, system_tools, self.use_runpath)?;
        }

        Ok(())
    }
}

/// Calls `patchelf` utility for updating the rpath/runpath of the binary.
fn call_patchelf(
    elf_path: &Path,
    new_rpath: &[PathBuf],
    system_tools: &SystemTools,
    use_runpath: bool,
) -> Result<(), RelinkError> {
    let new_rpath = new_rpath.iter().map(|p| p.to_string_lossy()).join(":");

    tracing::info!(
        "Relinking {:?} (patchelf)",
        elf_path.file_name().unwrap_or_default()
    );
    tracing::debug!("New rpath: {:?}", new_rpath);

    let mut cmd = system_tools.call(Tool::Patchelf)?;

    // prefer using RPATH over RUNPATH because RPATH takes precedence when
    // searching for shared libraries and cannot be overridden with
    // `LD_LIBRARY_PATH`. This ensures that the libraries from the environment
    // are found first, providing better isolation and preventing potential
    // conflicts with system libraries.
    //
    // Android is the exception: bionic ignores DT_RPATH entirely, so forcing it would
    // leave the binary with no search path rather than a stricter one. patchelf writes
    // DT_RUNPATH by default, which is what bionic reads.
    if !use_runpath {
        cmd.arg("--force-rpath");
    }

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

/// Returns the binary parsing context for the given ELF binary.
fn get_context(object: &Elf) -> goblin::container::Ctx {
    let container = if object.is_64 {
        goblin::container::Container::Big
    } else {
        goblin::container::Container::Little
    };

    let le = if object.little_endian {
        goblin::container::Endian::Little
    } else {
        goblin::container::Endian::Big
    };

    goblin::container::Ctx { container, le }
}

/// To relink binaries we do the following operations, where the *preferred* tag is
/// `DT_RPATH`, or `DT_RUNPATH` when `use_runpath` is set (Android, whose linker ignores
/// `DT_RPATH`):
///
/// - if the binary has both, we keep the preferred one and empty out the other
/// - if the binary has only the other one, we change its tag to the preferred one
/// - if the binary has only the preferred one, we just rewrite its value
fn builtin_relink(
    elf_path: &Path,
    new_rpath: &[PathBuf],
    use_runpath: bool,
) -> Result<(), RelinkError> {
    let new_rpath = new_rpath.iter().map(|p| p.to_string_lossy()).join(":");

    let file = std::fs::OpenOptions::new()
        .read(true)
        .write(true)
        .open(elf_path)?;

    let data = unsafe { memmap2::Mmap::map(&file) }?;

    let object = goblin::elf::Elf::parse(&data)?;

    let dynamic = match object.dynamic.as_ref() {
        Some(dynamic) => dynamic,
        None => {
            tracing::debug!("{} is not dynamically linked", elf_path.display());
            return Ok(());
        }
    };

    let dynstrtab =
        Strtab::parse(&data, dynamic.info.strtab, dynamic.info.strsz, 0x0).map_err(|e| {
            tracing::error!("Failed to parse strtab: {:?}", e);
            RelinkError::BuiltinRelinkFailed
        })?;

    // reopen to please the borrow checker
    let data = unsafe { memmap2::Mmap::map(&file) }?;

    let has_rpath = dynamic
        .dyns
        .iter()
        .any(|entry| entry.d_tag == goblin::elf::dynamic::DT_RPATH);

    let has_runpath = dynamic
        .dyns
        .iter()
        .any(|entry| entry.d_tag == goblin::elf::dynamic::DT_RUNPATH);

    // Which tag we write, and which one we neutralise. Both are the same size, so
    // swapping a tag never moves anything in the dynamic section.
    let (preferred_tag, other_tag) = if use_runpath {
        (
            goblin::elf::dynamic::DT_RUNPATH,
            goblin::elf::dynamic::DT_RPATH,
        )
    } else {
        (
            goblin::elf::dynamic::DT_RPATH,
            goblin::elf::dynamic::DT_RUNPATH,
        )
    };
    let has_preferred = if use_runpath { has_runpath } else { has_rpath };

    // fallback to patchelf if there is no rpath found
    if !has_rpath && !has_runpath {
        return Err(RelinkError::RpathNotFound);
    }

    let mut data_mut = data.make_mut().expect("Failed to make data mutable");

    let overwrite_strtab =
        |data_mut: &mut MmapMut, offset: usize, new_value: &str| -> Result<(), RelinkError> {
            let new_value = new_value.as_bytes();
            let old_value = dynstrtab
                .get_at(offset)
                .ok_or(RelinkError::BuiltinRelinkFailed)?;

            if new_value.len() > old_value.len() {
                tracing::error!("new value is longer than old value");
                return Err(RelinkError::BuiltinRelinkFailed);
            }

            let offset = offset + dynamic.info.strtab as usize;
            data_mut[offset..offset + new_value.len()].copy_from_slice(new_value);
            // pad with null bytes
            data_mut[offset + new_value.len()..offset + old_value.len()].fill(0);

            Ok(())
        };

    let mut new_dynamic = Vec::new();
    let mut push_to_end = Vec::new();
    let mut needs_rewrite = false;

    for entry in dynamic.dyns.iter() {
        if entry.d_tag == preferred_tag {
            overwrite_strtab(&mut data_mut, entry.d_val as usize, &new_rpath)?;
            new_dynamic.push(entry.clone());
        } else if entry.d_tag == other_tag {
            needs_rewrite = true;
            if has_preferred {
                // todo: clear value from strtab to avoid any mentions of placeholders in the binary
                overwrite_strtab(&mut data_mut, entry.d_val as usize, "")?;
                push_to_end.push(Dyn {
                    d_tag: preferred_tag,
                    d_val: entry.d_val,
                });
            } else {
                let mut new_entry = entry.clone();
                new_entry.d_tag = preferred_tag;
                overwrite_strtab(&mut data_mut, entry.d_val as usize, &new_rpath)?;
                new_dynamic.push(new_entry);
            }
        } else {
            new_dynamic.push(entry.clone());
        }
    }
    // add empty entries to the end to keep offsets correct
    new_dynamic.extend(push_to_end);

    if needs_rewrite {
        // now we need to write the new dynamic section
        let mut offset = object
            .program_headers
            .iter()
            .find(|header| header.p_type == goblin::elf::program_header::PT_DYNAMIC)
            .map(|header| header.p_offset)
            .ok_or(RelinkError::BuiltinRelinkFailed)? as usize;
        let ctx = get_context(&object);
        for d in new_dynamic {
            data_mut.pwrite_with::<goblin::elf::dynamic::Dyn>(d, offset, ctx)?;
            offset += goblin::elf::dynamic::Dyn::size_with(&ctx);
        }
    }

    tracing::info!("Relinking {:?}", elf_path.file_name().unwrap_or_default());

    data_mut.flush()?;

    Ok(())
}

#[cfg(test)]
mod test {
    use super::*;
    use fs_err as fs;
    use std::path::Path;
    use tempfile::tempdir_in;

    #[test]
    #[cfg(target_os = "linux")]
    fn test_file_type_detection() -> Result<(), RelinkError> {
        let test_data = Path::new(env!("CARGO_MANIFEST_DIR")).join("../../test-data/binary_files");

        // Test that object files are not recognized as valid for relinking
        let object_file = test_data.join("simple-elf.o");
        assert!(
            !SharedObject::test_file(&object_file)?,
            "Object files should not be valid for relinking"
        );

        // Test that shared libraries are recognized as valid
        let so_file = test_data.join("simple.so");
        assert!(
            SharedObject::test_file(&so_file)?,
            "Shared libraries should be valid for relinking"
        );

        // Test existing binary that we know is valid
        let binary_file = test_data.join("zlink");
        assert!(
            SharedObject::test_file(&binary_file)?,
            "Executables should be valid for relinking"
        );

        Ok(())
    }

    #[test]
    fn relink_patchelf() -> Result<(), RelinkError> {
        if which::which("patchelf").is_err() {
            tracing::warn!("patchelf not found, skipping test");
            return Ok(());
        }

        // copy binary to a temporary directory
        let prefix = Path::new(env!("CARGO_MANIFEST_DIR")).join("../../test-data/binary_files");
        let tmp_dir = tempdir_in(&prefix)?.keep();
        let binary_path = tmp_dir.join("zlink");
        fs::copy(prefix.join("zlink"), &binary_path)?;

        let globvec = GlobVec::from_strings(vec!["/usr/lib/custom**".to_string()], vec![]).unwrap();

        // default rpaths of the test binary are:
        // - /rattler-build_zlink/host_env_placehold/lib
        // - /rattler-build_zlink/build_env/lib
        // so we are expecting it to keep the host prefix and discard the build prefix
        let encoded_prefix = Path::new("/rattler-build_zlink/host_env_placehold");
        let object = SharedObject::new(&binary_path)?;
        object.relink(
            &prefix,
            encoded_prefix,
            &[],
            &globvec,
            &SystemTools::new("rattler-build", "0.0.0"),
        )?;
        let object = SharedObject::new(&binary_path)?;
        assert!(SharedObject::test_file(&binary_path)?);
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

    // rpath: none
    // encoded prefix: "/rattler-build_zlink/host_env_placehold"
    // binary path: test-data/binary_files/tmp/zlink
    // prefix: "test-data/binary_files"
    // new rpath: $ORIGIN/../lib
    #[test]
    fn relink_add_rpath() -> Result<(), RelinkError> {
        if which::which("patchelf").is_err() {
            tracing::warn!("patchelf not found, skipping test");
            return Ok(());
        }

        // copy binary to a temporary directory
        let prefix = Path::new(env!("CARGO_MANIFEST_DIR")).join("../../test-data/binary_files");
        let tmp_dir = tempdir_in(&prefix)?.keep();
        let binary_path = tmp_dir.join("zlink-no-rpath");
        fs::copy(prefix.join("zlink-no-rpath"), &binary_path)?;

        let encoded_prefix = Path::new("/rattler-build_zlink/host_env_placehold");
        let object = SharedObject::new(&binary_path)?;
        assert!(SharedObject::test_file(&binary_path)?);
        object.relink(
            &prefix,
            encoded_prefix,
            &[String::from("lib/")],
            &GlobVec::default(),
            &SystemTools::new("rattler-build", "0.0.0"),
        )?;
        let object = SharedObject::new(&binary_path)?;
        assert_eq!(
            vec!["$ORIGIN/../lib"],
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
        let prefix = Path::new(env!("CARGO_MANIFEST_DIR")).join("../../test-data/binary_files");
        let tmp_dir = tempdir_in(&prefix)?;
        let binary_path = tmp_dir.path().join("zlink");
        fs::copy(prefix.join("zlink"), &binary_path)?;

        let object = SharedObject::new(&binary_path)?;
        assert!(SharedObject::test_file(&binary_path)?);
        assert!(object.runpaths.is_empty() && !object.rpaths.is_empty());

        super::builtin_relink(
            &binary_path,
            &[
                PathBuf::from("$ORIGIN/../lib"),
                PathBuf::from("/usr/lib/custom_lib"),
            ],
            false,
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

    #[test]
    fn relink_builtin_runpath() -> Result<(), RelinkError> {
        // copy binary to a temporary directory
        let prefix = Path::new(env!("CARGO_MANIFEST_DIR")).join("../../test-data/binary_files");
        let tmp_dir = tempdir_in(&prefix)?;
        let binary_path = tmp_dir.path().join("zlink");
        fs::copy(prefix.join("zlink-runpath"), &binary_path)?;

        let object = SharedObject::new(&binary_path)?;
        assert!(SharedObject::test_file(&binary_path)?);
        assert!(!object.runpaths.is_empty() && object.rpaths.is_empty());

        super::builtin_relink(
            &binary_path,
            &[
                PathBuf::from("$ORIGIN/../lib"),
                PathBuf::from("/usr/lib/custom_lib"),
            ],
            false,
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
    /// Android's linker only reads DT_RUNPATH, so relinking for an Android target has to
    /// convert an existing DT_RPATH rather than rewrite it in place. `zlink` has RPATH
    /// and no RUNPATH, which is the case that used to silently produce a binary with no
    /// usable search path at all on device.
    #[test]
    fn relink_builtin_android_converts_rpath_to_runpath() -> Result<(), RelinkError> {
        let prefix = Path::new(env!("CARGO_MANIFEST_DIR")).join("../../test-data/binary_files");
        let tmp_dir = tempdir_in(&prefix)?;
        let binary_path = tmp_dir.path().join("zlink");
        fs::copy(prefix.join("zlink"), &binary_path)?;

        let object = SharedObject::new(&binary_path)?;
        assert!(object.runpaths.is_empty() && !object.rpaths.is_empty());

        super::builtin_relink(&binary_path, &[PathBuf::from("$ORIGIN/../lib")], true)?;

        let object = SharedObject::new(&binary_path)?;
        assert_eq!(
            vec!["$ORIGIN/../lib"],
            object
                .runpaths
                .iter()
                .flat_map(|r| r.split(':'))
                .collect::<Vec<&str>>()
        );
        assert!(
            object.rpaths.iter().all(|r| r.is_empty()),
            "DT_RPATH should be gone or empty, got {:?}",
            object.rpaths
        );

        Ok(())
    }

    /// And a binary that already has DT_RUNPATH keeps it, with the new value.
    #[test]
    fn relink_builtin_android_keeps_runpath() -> Result<(), RelinkError> {
        let prefix = Path::new(env!("CARGO_MANIFEST_DIR")).join("../../test-data/binary_files");
        let tmp_dir = tempdir_in(&prefix)?;
        let binary_path = tmp_dir.path().join("zlink");
        fs::copy(prefix.join("zlink-runpath"), &binary_path)?;

        let object = SharedObject::new(&binary_path)?;
        assert!(!object.runpaths.is_empty() && object.rpaths.is_empty());

        super::builtin_relink(&binary_path, &[PathBuf::from("$ORIGIN/../lib")], true)?;

        let object = SharedObject::new(&binary_path)?;
        assert_eq!(
            vec!["$ORIGIN/../lib"],
            object
                .runpaths
                .iter()
                .flat_map(|r| r.split(':'))
                .collect::<Vec<&str>>()
        );
        assert!(object.rpaths.is_empty());

        Ok(())
    }

    /// The choice is made from the target platform, not from the binary.
    #[test]
    fn new_for_platform_selects_the_dynamic_tag() -> Result<(), RelinkError> {
        let prefix = Path::new(env!("CARGO_MANIFEST_DIR")).join("../../test-data/binary_files");
        let binary_path = prefix.join("zlink");

        assert!(!SharedObject::new_for_platform(&binary_path, Platform::LinuxAarch64)?.use_runpath);
        assert!(
            SharedObject::new_for_platform(&binary_path, Platform::AndroidAarch64)?.use_runpath
        );

        Ok(())
    }
}
