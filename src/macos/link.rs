//! Relink a dylib to use relative paths for rpaths
use goblin::mach::Mach;
use scroll::Pread;
use std::collections::HashSet;
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

        let exchange_dylib = |path: &Path| {
            if let Ok(relpath) = path.strip_prefix(prefix) {
                let new_path = PathBuf::from(format!("@rpath/{}", relpath.to_string_lossy()));
                Some(new_path)
            } else {
                None
            }
        };

        let data = fs::read(&self.path)?;

        let object = match goblin::mach::Mach::parse(&data)? {
            Mach::Binary(mach) => mach,
            _ => {
                tracing::error!("Not a valid Mach-O binary.");
                return Err(RelinkError::FileTypeNotHandled);
            }
        };
        let mut new_data = data.clone();

        for cmd in object.load_commands.iter() {
            match cmd.command {
                goblin::mach::load_command::CommandVariant::Rpath(ref rpath) => {
                    let cmdsize = rpath.cmdsize as usize;
                    let offset = cmd.offset + rpath.path as usize;
                    let path_data = data.pread::<&str>(offset).unwrap().to_string();
                    println!("path_data: {:?}", path_data);
                    let path = PathBuf::from(&path_data);
                    if path.is_absolute() {
                        let orig_path = encoded_prefix.join(
                            self.path
                                .strip_prefix(prefix)?
                                .parent()
                                .expect("Could not get parent"),
                        );

                        let relpath = pathdiff::diff_paths(&path, &orig_path).ok_or(
                            RelinkError::PathDiffError {
                                from: orig_path.clone(),
                                to: path.clone(),
                            },
                        )?;

                        let new_rpath =
                            PathBuf::from(format!("@loader_path/{}", relpath.to_string_lossy()));
                        println!("Exchange rpath {:?} -> {:?}", path, new_rpath);
                        let new_rpath_string = new_rpath.to_string_lossy();
                        let mut new_rpath_bytes = new_rpath_string.as_bytes().to_vec();
                        // extend with null bytes
                        let string_len = path_data.len();
                        new_rpath_bytes.resize(string_len, 0);

                        new_data.splice(offset..offset + string_len, new_rpath_bytes);

                        modified = true;
                    }
                }
                // check dylib id
                goblin::mach::load_command::CommandVariant::IdDylib(ref id)
                | goblin::mach::load_command::CommandVariant::LoadWeakDylib(ref id)
                | goblin::mach::load_command::CommandVariant::LoadUpwardDylib(ref id)
                | goblin::mach::load_command::CommandVariant::ReexportDylib(ref id)
                | goblin::mach::load_command::CommandVariant::LazyLoadDylib(ref id)
                | goblin::mach::load_command::CommandVariant::LoadDylib(ref id) => {
                    let offset = cmd.offset + id.dylib.name as usize;
                    let path_data = data.pread::<&str>(offset).unwrap().to_string();
                    println!("ID path_data: {:?}", path_data);

                    let path = PathBuf::from(&path_data);

                    if let Some(new_path) = exchange_dylib(&path) {
                        let new_rpath_string = new_path.to_string_lossy();
                        println!("Exchange dylib {:?} -> {:?}", path, new_rpath_string);
                        let mut new_rpath_bytes = new_rpath_string.as_bytes().to_vec();
                        // extend with null bytes
                        let string_len = path_data.len() + 1;
                        new_rpath_bytes.resize(string_len, 0);

                        new_data.splice(offset..offset + string_len, new_rpath_bytes);

                        modified = true;
                    }
                }
                _ => {}
            }
        }

        // for rpath in &self.rpaths {
        //     if rpath.is_absolute() {
        //         let orig_path = encoded_prefix.join(
        //             self.path
        //                 .strip_prefix(prefix)?
        //                 .parent()
        //                 .expect("Could not get parent"),
        //         );

        //         let relpath =
        //             pathdiff::diff_paths(rpath, &orig_path).ok_or(RelinkError::PathDiffError {
        //                 from: orig_path.clone(),
        //                 to: rpath.clone(),
        //             })?;

        //         let new_rpath =
        //             PathBuf::from(format!("@loader_path/{}", relpath.to_string_lossy()));

        //         changes.add_rpath.insert(new_rpath);
        //         changes.delete_rpath.insert(rpath.clone());
        //         modified = true;
        //     }
        // }

        // if let Some(id) = &self.id {
        //     if let Some(new_dylib) = exchange_dylib(id) {
        //         changes.change_id = Some(new_dylib);
        //         modified = true;
        //     }
        // }

        // for lib in &self.libraries {
        //     if let Some(new_dylib) = exchange_dylib(lib) {
        //         changes.change_dylib.push((lib.clone(), new_dylib));
        //         modified = true;
        //     }
        // }

        if modified {
            // install_name_tool(&self.path, &changes)?;
            // overwrite the file
            fs::write(&self.path, new_data)?;
            codesign(&self.path)?;
        }

        Ok(())
    }
}

fn codesign(path: &PathBuf) -> Result<(), std::io::Error> {
    tracing::info!("codesigning {:?}", path);
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

    let install_name_tool_exe = which::which("install_name_tool")?;

    let mut cmd = std::process::Command::new(install_name_tool_exe);

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

#[cfg(test)]
mod test {
    use super::*;
    use std::path::PathBuf;

    #[test]
    fn test_relink() {
        let binary_orig =
            PathBuf::from("/Users/wolfv/Programs/rattler-build/libcurl.4.dylib.start");

        let binary = PathBuf::from("/Users/wolfv/Programs/rattler-build/libcurl.4.dylib");
        fs::copy(&binary_orig, &binary).unwrap();

        // let prefix = PathBuf::from("/Users/runner/work/_temp/_runner_file_commands");
        let encoded_prefix = PathBuf::from("/Users/wolfv/Programs/rattler-build/output/bld/rattler-build_curl_1703190008/host_env_placehold_placehold_placehold_placehold_placehold_placehold_placehold_placehold_placehold_placehold_placehold_placehold_placehold_placehold_placehold_placehold_placehold");
        let dylib = Dylib::new(&binary).unwrap();
        dylib
            .relink(&binary.parent().unwrap(), &encoded_prefix)
            .unwrap();
    }
}
