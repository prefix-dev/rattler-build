//! Relink a dylib to use relative paths for rpaths
use goblin::mach::Mach;
use std::collections::HashSet;
use std::fs::{self, File};
use std::io::Read;
use std::path::{Path, PathBuf};

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
    PathDiffError { from: PathBuf, to: PathBuf },
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
    pub fn relink(&self, prefix: &Path, encoded_prefix: &Path) -> Result<(), RelinkError> {
        let mut changes = DylibChanges::default();
        let mut modified = false;
        for rpath in &self.rpaths {
            if rpath.is_absolute() {
                let orig_path = encoded_prefix.join(
                    self.path
                        .strip_prefix(prefix)?
                        .parent()
                        .expect("Could not get parent"),
                );

                let relpath =
                    pathdiff::diff_paths(rpath, &orig_path).ok_or(RelinkError::PathDiffError {
                        from: orig_path.clone(),
                        to: rpath.clone(),
                    })?;

                let new_rpath =
                    PathBuf::from(format!("@loader_path/{}", relpath.to_string_lossy()));

                changes.add_rpath.insert(new_rpath);
                changes.delete_rpath.insert(rpath.clone());
                modified = true;
            }
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
                changes.change_dylib.push((lib.clone(), new_dylib));
                modified = true;
            }
        }

        if modified {
            install_name_tool(&self.path, &changes)?;
        }

        Ok(())
    }
}

/// Changes to apply to a dylib
#[derive(Debug, Default)]
struct DylibChanges {
    // rpaths to delete
    delete_rpath: HashSet<PathBuf>,
    // rpaths to add
    add_rpath: HashSet<PathBuf>,
    // dylib id to change
    change_id: Option<PathBuf>,
    // dylibs to rewrite
    change_dylib: Vec<(PathBuf, PathBuf)>,
}

fn install_name_tool(dylib_path: &Path, changes: &DylibChanges) -> Result<(), RelinkError> {
    tracing::info!("install_name_tool for {:?}: {:?}", dylib_path, changes);

    let mut cmd = std::process::Command::new("install_name_tool");

    if let Some(id) = &changes.change_id {
        cmd.arg("-id").arg(id);
    }

    for (old, new) in &changes.change_dylib {
        cmd.arg("-change").arg(old).arg(new);
    }

    for rpath in &changes.delete_rpath {
        cmd.arg("-delete_rpath").arg(rpath);
    }

    for rpath in &changes.add_rpath {
        cmd.arg("-add_rpath").arg(rpath);
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
