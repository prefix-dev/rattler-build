use goblin::mach::load_command::{CommandVariant, RpathCommand};
use goblin::mach::Mach;
use scroll::Pread;
use std::collections::HashSet;
use std::fs;
use std::io::Read;
use std::path::{Path, PathBuf};


struct Dylib {
    path: PathBuf,
    id: Option<PathBuf>,
    rpaths: Vec<PathBuf>,
    dependencies: Vec<PathBuf>,
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

fn exchange_dylib_rpath(dylib: &Path, prefix: &Path) -> Option<PathBuf> {
    if dylib.starts_with(prefix) {
        let new_location =
            pathdiff::diff_paths(dylib, prefix.join("lib")).expect("Could not get relative path");
        let new_path = Path::new("@rpath").join(new_location);
        return Some(new_path);
    }
    None
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
fn modify_dylib(
    dylib_path: &Path,
    prefix: &Path,
    encoded_prefix: &Path,
) -> Result<Dylib, RelinkError> {
    let data = fs::read(dylib_path)?;
    let mut changes = DylibChanges::default();

    match goblin::mach::Mach::parse(&data)? {
        Mach::Binary(mach) => {
            let mut dylib = Dylib {
                path: dylib_path.to_path_buf(),
                id: None,
                rpaths: Vec::new(),
                dependencies: Vec::new(),
            };

            let mut modified = false;
            for command in &mach.load_commands {
                match command.command {
                    CommandVariant::IdDylib(dylib_cmd)
                    | CommandVariant::LoadDylib(dylib_cmd)
                    | CommandVariant::LoadWeakDylib(dylib_cmd)
                    | CommandVariant::ReexportDylib(dylib_cmd) => {
                        let libname_offset = command.offset + dylib_cmd.dylib.name as usize;
                        let libname = data.pread::<&str>(libname_offset)?.to_string();
                        let libname = PathBuf::from(&libname);

                        if matches!(command.command, CommandVariant::IdDylib(_)) {
                            dylib.id = Some(libname.clone());
                        } else {
                            dylib.dependencies.push(libname.clone());
                        }

                        if let Some(new_dylib) = exchange_dylib_rpath(&libname, encoded_prefix) {
                            match command.command {
                                CommandVariant::IdDylib(_) => {
                                    changes.change_id = Some(new_dylib);
                                }
                                CommandVariant::LoadDylib(_)
                                | CommandVariant::LoadWeakDylib(_)
                                | CommandVariant::ReexportDylib(_) => {
                                    changes.change_dylib.push((libname, new_dylib));
                                }
                                _ => {}
                            }
                            modified = true;
                        }
                    }
                    CommandVariant::Rpath(RpathCommand {
                        cmd: _,
                        cmdsize: _,
                        path,
                    }) => {
                        let rpath_offset = command.offset + path as usize;
                        let rpath = PathBuf::from(data.pread::<&str>(rpath_offset)?);

                        dylib.rpaths.push(rpath.clone());

                        if rpath.is_absolute() {
                            let orig_path = encoded_prefix.join(
                                dylib_path
                                    .strip_prefix(prefix)?
                                    .parent()
                                    .expect("Could not get parent"),
                            );

                            let relpath = pathdiff::diff_paths(&rpath, &orig_path).ok_or(
                                RelinkError::PathDiffError {
                                    from: orig_path.clone(),
                                    to: rpath.clone(),
                                },
                            )?;

                            let new_rpath = PathBuf::from(format!(
                                "@loader_path/{}",
                                relpath.to_string_lossy()
                            ));

                            changes.add_rpath.insert(new_rpath);
                            changes.delete_rpath.insert(rpath);
                            modified = true;
                        }
                    }
                    _ => {}
                }
            }

            if modified {
                install_name_tool(dylib_path, &changes)?;
            }
            return Ok(dylib);
        }
        _ => {
            tracing::error!("Not a valid Mach-O binary.");
            return Err(RelinkError::FileTypeNotHandled);
        }
    }
}

pub fn relink_paths(
    paths: &HashSet<PathBuf>,
    prefix: &Path,
    encoded_prefix: &Path,
) -> Result<(), RelinkError> {
    for p in paths {
        if fs::symlink_metadata(p)?.is_symlink() {
            tracing::trace!("relink: skipping symlink {}", p.display());
            continue;
        }

        // Skip files that are not binaries
        let mut buffer = vec![0; 1024];
        let mut file = fs::File::open(p)?;
        let n = file.read(&mut buffer)?;
        let buffer = &buffer[..n];

        let content_type = content_inspector::inspect(buffer);
        if content_type != content_inspector::ContentType::BINARY {
            continue;
        }

        // now check if we find the magic number
        let ctx_res = goblin::mach::parse_magic_and_ctx(buffer, 0);

        if ctx_res.is_err() {
            tracing::trace!("relink: skipping non-mach-o file {}", p.display());
            continue;
        } else {
            tracing::trace!("relink: relinking {}", p.display());
        }

        let (_, ctx) = ctx_res.unwrap();

        if ctx.is_none() {
            tracing::trace!("relink: skipping non-mach-o file {}", p.display());
            continue;
        }

        match modify_dylib(p, prefix, encoded_prefix) {
            Ok(_) => {}
            Err(e) => {
                tracing::error!("Could not modify dylib {}: {}", p.display(), e);
            }
        }
    }

    Ok(())
}
