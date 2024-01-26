//! Relink a dylib to use relative paths for rpaths
use globset::GlobSet;
use goblin::mach::Mach;
use memmap2::MmapMut;
use scroll::Pread;
use std::collections::HashMap;
use std::fmt;
use std::fs::{self, File};
use std::io::Read;
use std::path::{Path, PathBuf};
use std::process::Command;

/// A macOS dylib (Mach-O)
pub struct Dylib {
    /// Path to the dylib
    pub path: PathBuf,
    /// ID of the dylib (encoded)
    pub id: Option<PathBuf>,
    /// rpaths in the dlib
    pub rpaths: Vec<PathBuf>,
    /// all dependencies of the dylib
    pub libraries: Vec<PathBuf>,
}

#[derive(thiserror::Error, Debug)]
pub enum RelinkError {
    #[error("failed to run install_name_tool")]
    InstallNameToolFailed,

    #[error(
        "failed to find install_name_tool: please install xcode / install_name_tool on your system"
    )]
    InstallNameToolNotFound(#[from] which::Error),

    #[error("failed to read or write MachO file: {0}")]
    IoError(#[from] std::io::Error),

    #[error("failed to strip prefix from path: {0}")]
    StripPrefixError(#[from] std::path::StripPrefixError),

    #[error("failed to parse MachO file: {0}")]
    ParseMachOError(#[from] goblin::error::Error),

    #[error("filetype not handled")]
    FileTypeNotHandled,

    #[error("could not read string from MachO file: {0}")]
    ReadStringError(#[from] scroll::Error),

    #[error("failed to get relative path from {from} to {to}")]
    PathDiffFailed { from: PathBuf, to: PathBuf },

    #[error("failed to relink dylib with builtin relink (new path is longer than old path)")]
    BuiltinRelinkFailed,

    #[error("shared library has no parent directory")]
    NoParentDir,
}

impl Dylib {
    /// only parse the magic number of a file and check if it
    /// is a Mach-O file
    pub fn test_file(path: &Path) -> Result<bool, std::io::Error> {
        let mut file = File::open(path)?;
        let mut buf: [u8; 4] = [0; 4];
        file.read_exact(&mut buf)?;
        let ctx_res = goblin::mach::parse_magic_and_ctx(&buf, 0);
        match ctx_res {
            Ok((_, Some(_))) => Ok(true),
            Ok((_, None)) => Ok(false),
            Err(_) => Ok(false),
        }
    }

    /// parse the Mach-O file and extract all relevant information
    pub fn new(path: &Path) -> Result<Self, RelinkError> {
        let data = fs::read(path)?;

        match goblin::mach::Mach::parse(&data)? {
            Mach::Binary(mach) => {
                return Ok(Dylib {
                    path: path.to_path_buf(),
                    id: mach.name.map(PathBuf::from),
                    rpaths: mach.rpaths.iter().map(PathBuf::from).collect(),
                    libraries: mach.libs.iter().map(PathBuf::from).collect(),
                });
            }
            _ => {
                tracing::error!("Not a valid Mach-O binary.");
                Err(RelinkError::FileTypeNotHandled)
            }
        }
    }

    /// Modify a dylib to use relative paths for rpaths and dylibs
    /// This makes the dylib relocatable and allows it to be used in a conda environment.
    ///
    /// The main trick is to use `install_name_tool` to change the rpaths and dylibs to use relative paths.
    ///
    /// ### What is an RPath?
    ///
    /// An RPath is a path that is searched for dylibs when loading a dylib. It is similar to the `LD_LIBRARY_PATH`
    /// on Linux. The RPath is encoded in the dylib itself.
    ///
    /// We change the rpath to use `@loader_path` which is the *path of the dylib* itself.
    /// When loading a dylib, we use `@rpath` which is the rpath of the executable that loads the dylib. This allows
    /// us to use the same dylib in different environments/prefixes.
    ///
    /// We also change the dylib id to use `@rpath` so that the dylib can be loaded by other dylibs. The dylib id
    /// is the path that other dylibs use when linking to this dylib.
    ///
    /// # Arguments
    ///
    /// * `dylib_path` - Path to the dylib to modify
    /// * `prefix` - The prefix of the file (usually a temporary directory)
    /// * `encoded_prefix` - The prefix of the file as encoded in the dylib at build time (e.g. the host prefix)
    pub fn relink(
        &self,
        prefix: &Path,
        encoded_prefix: &Path,
        custom_rpaths: &[String],
        rpath_allowlist: Option<&GlobSet>,
    ) -> Result<(), RelinkError> {
        let mut changes = DylibChanges::default();
        let mut modified = false;

        let mut rpaths = self.rpaths.clone();

        for rpath in custom_rpaths.iter().rev() {
            let rpath = encoded_prefix.join(rpath);
            if !rpaths.contains(&rpath) {
                rpaths.insert(0, rpath);
            }
        }

        let mut final_rpaths = Vec::new();

        for rpath in &self.rpaths {
            if let Ok(rel) = rpath.strip_prefix(encoded_prefix) {
                let new_rpath = prefix.join(rel);

                let parent = self.path.parent().ok_or(RelinkError::NoParentDir)?;

                let relative_path = pathdiff::diff_paths(&new_rpath, parent).ok_or(
                    RelinkError::PathDiffFailed {
                        from: new_rpath.clone(),
                        to: parent.to_path_buf(),
                    },
                )?;

                let new_rpath =
                    PathBuf::from(format!("@loader_path/{}", relative_path.to_string_lossy()));

                final_rpaths.push(new_rpath.clone());
                // changes.change_rpath.insert(rpath.clone(), new_rpath);
                // modified = true;
            } else if rpath_allowlist.map(|g| g.is_match(rpath)).unwrap_or(false) {
                tracing::info!("Allowlisted rpath: {}", rpath.display());
                final_rpaths.push(rpath.clone());
            } else {
                tracing::warn!("Rpath not in prefix: {} – removing it", rpath.display());
            }
        }

        if final_rpaths != self.rpaths {
            for (old, new) in self.rpaths.iter().zip(final_rpaths.iter()) {
                changes
                    .change_rpath
                    .push((Some(old.clone()), Some(new.clone())));
            }

            if self.rpaths.len() > final_rpaths.len() {
                for old in self.rpaths.iter().skip(final_rpaths.len()) {
                    changes.change_rpath.push((Some(old.clone()), None));
                }
            } else {
                for new in final_rpaths.iter().skip(self.rpaths.len()) {
                    changes.change_rpath.push((None, Some(new.clone())));
                }
            }

            modified = true;
        }

        let exchange_dylib = |path: &Path| {
            if let Ok(relpath) = path.strip_prefix(prefix) {
                let new_path = PathBuf::from(format!("@rpath/{}", relpath.to_string_lossy()));
                Some(new_path)
            } else {
                None
            }
        };

        if let Some(id) = &self.id {
            if let Some(new_dylib) = exchange_dylib(id) {
                changes.change_id = Some(new_dylib);
                modified = true;
            }
        }

        for lib in &self.libraries {
            if let Some(new_dylib) = exchange_dylib(lib) {
                changes.change_dylib.insert(lib.clone(), new_dylib);
                modified = true;
            }
        }

        if modified {
            // run builtin relink. if it fails, try install_name_tool
            if let Err(e) = relink(&self.path, &changes) {
                tracing::warn!(
                    "\n\nbuiltin relink failed for {}: {}. Please file an issue on Github!\n\n",
                    &self.path.display(),
                    e
                );
                install_name_tool(&self.path, &changes)?;
            }
            codesign(&self.path)?;
        }

        Ok(())
    }
}

fn codesign(path: &Path) -> Result<(), std::io::Error> {
    tracing::info!("codesigning {:?}", path.file_name().unwrap_or_default());
    Command::new("codesign")
        .arg("-f")
        .arg("-s")
        .arg("-")
        .arg(path)
        .output()
        .map(|_| ())
        .map_err(|e| {
            tracing::error!("codesign failed: {}", e);
            e
        })
}

/// Changes to apply to a dylib
#[derive(Debug, Default)]
struct DylibChanges {
    // rpaths to change
    change_rpath: Vec<(Option<PathBuf>, Option<PathBuf>)>,
    // dylib id to change
    change_id: Option<PathBuf>,
    // dylibs to rewrite
    change_dylib: HashMap<PathBuf, PathBuf>,
}

impl fmt::Display for DylibChanges {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> std::fmt::Result {
        fn strip_placeholder_prefix(path: &Path) -> PathBuf {
            let placeholder_index = path.components().position(|c| {
                c.as_os_str()
                    .to_string_lossy()
                    .starts_with("host_env_placehold_placehold")
            });
            if let Some(idx) = placeholder_index {
                let mut pb = PathBuf::from("$PLACEHOLDER_PREFIX");
                pb.extend(path.components().skip(idx + 1));
                pb
            } else {
                path.to_path_buf()
            }
        }

        for change in &self.change_rpath {
            match change {
                (Some(old), Some(new)) => {
                    writeln!(
                        f,
                        " - change rpath from {:?} to {:?}",
                        strip_placeholder_prefix(old),
                        new
                    )?;
                }
                (Some(old), None) => {
                    writeln!(f, " - delete rpath {:?}", strip_placeholder_prefix(old))?;
                }
                (None, Some(new)) => {
                    writeln!(f, " - add rpath {:?}", new)?;
                }
                (None, None) => {}
            }
        }

        if let Some(id) = &self.change_id {
            writeln!(f, " - change dylib id to {:?}", id)?;
        }

        for (old, new) in &self.change_dylib {
            writeln!(f, " - change dylib from {:?} to {:?}", old, new)?;
        }

        Ok(())
    }
}

/// The builtin relink function is used instead of calling out to `install_name_tool`.
/// The function attempts to modify the dylib rpath, dylib id and dylib dependencies
/// in order to make it more easily relocatable.
fn relink(dylib_path: &Path, changes: &DylibChanges) -> Result<(), RelinkError> {
    // we can currently only deal with rpath changes internally if:
    // - the new path is shorter than the old path
    // - no removal or addition is performed
    let can_deal_with_rpath = changes.change_rpath.iter().all(|(old, new)| {
        old.is_some()
            && new.is_some()
            && old.as_ref().unwrap().to_string_lossy().len()
                >= new.as_ref().unwrap().to_string_lossy().len()
    });

    if !can_deal_with_rpath {
        tracing::debug!("Builtin relink can't deal with rpath changes");
        return Err(RelinkError::BuiltinRelinkFailed);
    }

    tracing::info!(
        "builtin relink for {:?}:\n{}",
        dylib_path.file_name().unwrap_or_default(),
        changes
    );

    let mut modified = false;

    let file = std::fs::OpenOptions::new()
        .read(true)
        .write(true)
        .open(dylib_path)?;

    let data = unsafe { memmap2::Mmap::map(&file) }?;

    let object = match goblin::mach::Mach::parse(&data)? {
        Mach::Binary(mach) => mach,
        _ => {
            tracing::error!("Not a valid Mach-O binary.");
            return Err(RelinkError::FileTypeNotHandled);
        }
    };

    // Reopen for the borrow checker
    let mut data_mut = unsafe { memmap2::MmapMut::map_mut(&file) }?;

    let overwrite_path = |data_mut: &mut MmapMut,
                          offset: usize,
                          new_path: &Path,
                          old_path: &str|
     -> Result<(), RelinkError> {
        let new_path = new_path.to_string_lossy();
        if new_path == old_path {
            return Ok(());
        }
        let new_path = new_path.as_bytes();

        if new_path.len() > old_path.len() {
            tracing::debug!(
                "new path is longer than old path: {} > {}",
                new_path.len(),
                old_path.len()
            );
            return Err(RelinkError::BuiltinRelinkFailed);
        }

        data_mut[offset..offset + new_path.len()].copy_from_slice(new_path);
        // fill with null bytes
        data_mut[offset + new_path.len()..offset + old_path.len()].fill(0);

        Ok(())
    };

    let rpath_changes = changes
        .change_rpath
        .iter()
        .map(|(old, new)| (old.as_ref().unwrap(), new.as_ref().unwrap()))
        .collect::<HashMap<&PathBuf, &PathBuf>>();

    for cmd in object.load_commands.iter() {
        match cmd.command {
            goblin::mach::load_command::CommandVariant::Rpath(ref rpath) => {
                let offset = cmd.offset + rpath.path as usize;
                let old_path = data.pread::<&str>(offset).unwrap().to_string();

                let path = PathBuf::from(&old_path);
                if let Some(new_path) = rpath_changes.get(&path) {
                    overwrite_path(&mut data_mut, offset, new_path, &old_path)?;
                    modified = true;
                }
            }

            // check dylib id
            goblin::mach::load_command::CommandVariant::IdDylib(ref id) => {
                let offset = cmd.offset + id.dylib.name as usize;
                let old_path = data_mut.pread::<&str>(offset)?.to_string();

                if let Some(new_path) = changes.change_id.as_ref() {
                    overwrite_path(&mut data_mut, offset, new_path, &old_path)?;
                    modified = true;
                }
            }
            goblin::mach::load_command::CommandVariant::LoadWeakDylib(ref id)
            | goblin::mach::load_command::CommandVariant::LoadUpwardDylib(ref id)
            | goblin::mach::load_command::CommandVariant::ReexportDylib(ref id)
            | goblin::mach::load_command::CommandVariant::LazyLoadDylib(ref id)
            | goblin::mach::load_command::CommandVariant::LoadDylib(ref id) => {
                let offset = cmd.offset + id.dylib.name as usize;
                let old_path = data_mut.pread::<&str>(offset)?.to_string();

                let path = PathBuf::from(&old_path);
                if let Some(new_path) = changes.change_dylib.get(&path) {
                    overwrite_path(&mut data_mut, offset, new_path, &old_path)?;
                    modified = true;
                }
            }
            _ => {}
        }
    }

    // overwrite the file and resign
    if modified {
        data_mut.flush()?;
    }

    Ok(())
}

fn install_name_tool(dylib_path: &Path, changes: &DylibChanges) -> Result<(), RelinkError> {
    tracing::info!("install_name_tool for {:?}:\n{}", dylib_path, changes);

    let install_name_tool_exe = which::which("install_name_tool")?;

    let mut cmd = std::process::Command::new(install_name_tool_exe);

    if let Some(id) = &changes.change_id {
        cmd.arg("-id").arg(id);
    }

    for (old, new) in &changes.change_dylib {
        cmd.arg("-change").arg(old).arg(new);
    }

    for change in &changes.change_rpath {
        match change {
            (Some(old), Some(new)) => {
                cmd.arg("-delete_rpath").arg(old);
                cmd.arg("-add_rpath").arg(new);
            }
            (Some(old), None) => {
                cmd.arg("-delete_rpath").arg(old);
            }
            (None, Some(new)) => {
                cmd.arg("-add_rpath").arg(new);
            }
            (None, None) => {}
        }
    }

    cmd.arg(dylib_path);

    let output = cmd.output()?;

    if !output.status.success() {
        tracing::error!(
            "install_name_tool failed: {}",
            String::from_utf8_lossy(&output.stderr)
        );
        return Err(RelinkError::InstallNameToolFailed);
    }

    Ok(())
}

#[cfg(test)]
#[cfg(target_os = "macos")]
mod tests {
    use std::{
        collections::HashMap,
        fs,
        path::{Path, PathBuf},
    };

    use tempfile::tempdir_in;

    use crate::macos::link::{Dylib, DylibChanges};

    use super::{install_name_tool, RelinkError};

    #[test]
    fn test_relink_builtin() -> Result<(), RelinkError> {
        let prefix = Path::new(env!("CARGO_MANIFEST_DIR")).join("test-data/binary_files");
        let tmp_dir = tempdir_in(&prefix)?;
        let binary_path = tmp_dir.path().join("zlink");
        fs::copy(prefix.join("zlink-macos"), &binary_path)?;

        let object = Dylib::new(&binary_path).unwrap();
        let expected_rpath = PathBuf::from("/Users/wolfv/Programs/rattler-build/output/bld/rattler-build_zlink_1705569778/host_env_placehold_placehold_placehold_placehold_placehold_placehold_placehold_placehold_placehold_placehold_placehold_placehold_placehold_placehold_placehold_placehold_placehol/lib");

        assert_eq!(object.rpaths, vec![expected_rpath.clone()]);

        let changes = DylibChanges {
            change_rpath: vec![(
                Some(expected_rpath.clone()),
                Some(PathBuf::from("@loader_path/../lib")),
            )],
            change_id: None,
            change_dylib: HashMap::default(),
        };

        super::relink(&binary_path, &changes)?;

        let object = Dylib::new(&binary_path)?;
        assert_eq!(vec![PathBuf::from("@loader_path/../lib")], object.rpaths);

        Ok(())
    }

    #[test]
    fn test_relink_add_path() -> Result<(), RelinkError> {
        // check if install_name_tool is installed
        if which::which("install_name_tool").is_err() {
            println!("install_name_tool not found, skipping test");
            return Ok(());
        }

        let prefix = Path::new(env!("CARGO_MANIFEST_DIR")).join("test-data/binary_files");
        let tmp_dir = tempdir_in(&prefix)?;
        let binary_path = tmp_dir.path().join("zlink-force-rpath");
        fs::copy(prefix.join("zlink-macos"), &binary_path)?;

        let object = Dylib::new(&binary_path).unwrap();

        let delete_paths = object
            .rpaths
            .iter()
            .map(|p| (Some(p.clone()), None))
            .collect();
        let changes = DylibChanges {
            change_rpath: delete_paths,
            change_id: None,
            change_dylib: HashMap::default(),
        };
        install_name_tool(&binary_path, &changes)?;

        let object = Dylib::new(&binary_path)?;
        assert!(object.rpaths.is_empty());

        let expected_rpath = PathBuf::from("/Users/blabla/myrpath");
        let changes = DylibChanges {
            change_rpath: vec![(None, Some(expected_rpath.clone()))],
            change_id: None,
            change_dylib: HashMap::default(),
        };

        install_name_tool(&binary_path, &changes)?;

        let object = Dylib::new(&binary_path)?;
        assert_eq!(vec![expected_rpath], object.rpaths);

        Ok(())
    }
}
