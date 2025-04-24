//! Relink a dylib to use relative paths for rpaths
use fs_err::File;
use goblin::mach::Mach;
use indexmap::IndexSet;
use memmap2::MmapMut;
use scroll::Pread;
use std::collections::{HashMap, HashSet};
use std::fmt;
use std::io::Read;
use std::path::{Path, PathBuf};

use crate::post_process::relink::{RelinkError, Relinker};
use crate::recipe::parser::GlobVec;
use crate::system_tools::{SystemTools, Tool};
use crate::unix::permission_guard::{PermissionGuard, READ_WRITE};
use crate::utils::to_lexical_absolute;

/// A macOS dylib (Mach-O)
#[derive(Debug)]
pub struct Dylib {
    /// Path to the dylib
    path: PathBuf,
    /// all dependencies of the dylib
    libraries: HashSet<PathBuf>,
    /// rpaths in the dlib
    rpaths: Vec<PathBuf>,
    /// ID of the dylib (encoded)
    id: Option<PathBuf>,
}

impl Relinker for Dylib {
    /// only parse the magic number of a file and check if it
    /// is a Mach-O file
    fn test_file(path: &Path) -> Result<bool, RelinkError> {
        let mut file = File::open(path)?;
        let mut buf: [u8; 4] = [0; 4];
        match file.read_exact(&mut buf) {
            Ok(_) => {}
            Err(e) if e.kind() == std::io::ErrorKind::UnexpectedEof => return Ok(false),
            Err(e) => return Err(e.into()),
        }

        let ctx_res = goblin::mach::parse_magic_and_ctx(&buf, 0);
        match ctx_res {
            Ok((_, Some(_))) => Ok(true),
            Ok((_, None)) => Ok(false),
            Err(_) => Ok(false),
        }
    }

    /// parse the Mach-O file and extract all relevant information
    fn new(path: &Path) -> Result<Self, RelinkError> {
        let file = File::open(path).expect("Failed to open the Mach-O binary");
        let mmap = unsafe { memmap2::Mmap::map(&file)? };
        match goblin::mach::Mach::parse(&mmap)? {
            Mach::Binary(mach) => Ok(Dylib {
                path: path.to_path_buf(),
                id: mach.name.map(PathBuf::from),
                rpaths: mach.rpaths.iter().map(PathBuf::from).collect(),
                libraries: mach.libs.iter().map(PathBuf::from).collect(),
            }),
            _ => {
                tracing::error!("Not a valid Mach-O binary.");
                Err(RelinkError::FileTypeNotHandled)
            }
        }
    }

    /// Returns the shared libraries contained in the file.
    fn libraries(&self) -> HashSet<PathBuf> {
        self.libraries.clone()
    }

    /// Find libraries in the dylib and resolve them by taking into account the rpaths
    fn resolve_libraries(
        &self,
        prefix: &Path,
        encoded_prefix: &Path,
    ) -> HashMap<PathBuf, Option<PathBuf>> {
        let resolved_rpaths = self
            .rpaths
            .iter()
            .map(|rpath| self.resolve_rpath(rpath, prefix, encoded_prefix))
            .collect::<Vec<_>>();

        let mut resolved_libraries = HashMap::new();
        for lib in self.libraries.iter() {
            if lib == &PathBuf::from("@self") {
                continue;
            }
            resolved_libraries.insert(lib.clone(), None);

            if let Ok(lib_without_rpath) = lib.strip_prefix("@rpath/") {
                for rpath in &resolved_rpaths {
                    let resolved = rpath.join(lib_without_rpath);
                    if resolved.exists() {
                        let resolved_library_path =
                            Some(resolved.canonicalize().unwrap_or(resolved));
                        resolved_libraries.insert(lib.clone(), resolved_library_path);
                        break;
                    }
                }
            } else if lib.is_absolute() {
                resolved_libraries.insert(lib.clone(), Some(lib.clone()));
            }
        }
        resolved_libraries
    }

    /// Resolve the rpath and replace `@loader_path` with the path of the dylib
    fn resolve_rpath(&self, rpath: &Path, prefix: &Path, encoded_prefix: &Path) -> PathBuf {
        // get self path in "encoded prefix"
        let self_path =
            encoded_prefix.join(self.path.strip_prefix(prefix).expect("dylib not in prefix"));
        if let Ok(rpath_without_loader) = rpath.strip_prefix("@loader_path") {
            if let Some(library_parent) = self_path.parent() {
                return to_lexical_absolute(rpath_without_loader, library_parent);
            } else {
                tracing::warn!("shared library {:?} has no parent directory", self.path);
            }
        }
        rpath.to_path_buf()
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
    fn relink(
        &self,
        prefix: &Path,
        encoded_prefix: &Path,
        custom_rpaths: &[String],
        rpath_allowlist: &GlobVec,
        system_tools: &SystemTools,
    ) -> Result<(), RelinkError> {
        let mut changes = DylibChanges::default();
        let mut modified = false;

        let resolved_rpaths = self
            .rpaths
            .iter()
            .map(|rpath| self.resolve_rpath(rpath, prefix, encoded_prefix))
            .collect::<Vec<_>>();
        let mut new_rpaths = self.rpaths.clone();

        for rpath in custom_rpaths.iter().rev() {
            let rpath = encoded_prefix.join(rpath);
            if !resolved_rpaths.contains(&rpath) {
                tracing::debug!("Adding rpath: {:?}", rpath);
                new_rpaths.insert(0, rpath);
            }
        }

        let mut final_rpaths = Vec::new();

        for rpath in &new_rpaths {
            if rpath.starts_with("@loader_path") {
                let resolved = self.resolve_rpath(rpath, prefix, encoded_prefix);
                if resolved.starts_with(encoded_prefix) {
                    final_rpaths.push(rpath.clone());
                } else if rpath_allowlist.is_match(rpath) {
                    tracing::info!("Rpath in allow list: {}", rpath.display());
                    final_rpaths.push(rpath.clone());
                }
                tracing::info!(
                    "Rpath not in prefix or allow-listed: {} - removing it",
                    rpath.display()
                );
            } else if let Ok(rel) = rpath.strip_prefix(encoded_prefix) {
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
            } else if rpath_allowlist.is_match(rpath) {
                tracing::info!("Allowlisted rpath: {}", rpath.display());
                final_rpaths.push(rpath.clone());
            } else {
                tracing::info!(
                    "Rpath not in prefix or allow-listed: {} - removing it",
                    rpath.display()
                );
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

        // find the first rpath that looks like `lib/` and extends the prefix
        // by default, the first element of custom_rpaths is `lib/`
        let base_rpath = custom_rpaths
            .iter()
            .find(|r| !r.contains("@") && !r.starts_with('/') && !r.starts_with('.'));

        let exchange_dylib = |path: &Path| {
            // treat 'libfoo.dylib' the same as $PREFIX/lib/libfoo.dylib
            // if that's where it is installed
            let encoded_prefix_lib = encoded_prefix.join(base_rpath.cloned().unwrap_or_default());
            let resolved_path = encoded_prefix_lib.join(path);

            let path = if path.components().count() == 1 && resolved_path.exists() {
                tracing::debug!("Treating relative {:?} as {:?}", path, resolved_path);
                resolved_path
            } else {
                path.to_path_buf()
            };

            if let Ok(relpath) = path.strip_prefix(encoded_prefix_lib) {
                // absolute $PREFIX/lib/...
                let new_path = PathBuf::from(format!("@rpath/{}", relpath.to_string_lossy()));
                Some(new_path)
            } else {
                tracing::debug!("No need to exchange dylib {}", path.display());
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
            let _permission_guard = PermissionGuard::new(&self.path, READ_WRITE)?;
            // run builtin relink. If it fails, try install_name_tool
            if let Err(e) = relink(&self.path, &changes) {
                assert!(self.path.exists());
                tracing::warn!("Builtin relink failed {:?}, trying install_name_tool", e);
                install_name_tool(&self.path, &changes, system_tools)?;
            }
            codesign(&self.path, system_tools)?;
        }

        Ok(())
    }
}

fn codesign(path: &Path, system_tools: &SystemTools) -> Result<(), RelinkError> {
    let codesign = system_tools.find_tool(Tool::Codesign).map_err(|e| {
        tracing::error!("codesign not found: {}", e);
        RelinkError::CodesignFailed
    })?;

    let is_system_codesign = codesign.starts_with("/usr/bin/");

    let mut cmd = std::process::Command::new(codesign);
    cmd.args(["-f", "-s", "-"]);

    if is_system_codesign {
        cmd.arg("--preserve-metadata=entitlements,requirements");
    }
    cmd.arg(path);

    // log the cmd invocation
    tracing::info!("Running codesign: {:?}", cmd);

    let output = cmd.output().map_err(|e| {
        tracing::error!("codesign failed: {}", e);
        e
    })?;

    if !output.status.success() {
        tracing::error!(
            "codesign failed with status {}. \n  stdout: {}\n  stderr: {}",
            output.status,
            String::from_utf8_lossy(&output.stdout),
            String::from_utf8_lossy(&output.stderr)
        );
        return Err(RelinkError::CodesignFailed);
    }

    Ok(())
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
                let mut pb = PathBuf::from("$PREFIX");
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
                        " - changing absolute rpath from {:?} to {:?}",
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
            tracing::info!(
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

fn install_name_tool(
    dylib_path: &Path,
    changes: &DylibChanges,
    system_tools: &SystemTools,
) -> Result<(), RelinkError> {
    tracing::info!("install_name_tool for {:?}:\n{}", dylib_path, changes);

    let mut cmd = system_tools.call(Tool::InstallNameTool)?;

    if let Some(id) = &changes.change_id {
        cmd.arg("-id").arg(id);
    }

    for (old, new) in &changes.change_dylib {
        cmd.arg("-change").arg(old).arg(new);
    }

    let mut add_set = IndexSet::new();
    let mut remove_set = IndexSet::new();

    for change in &changes.change_rpath {
        match change {
            (Some(old), Some(new)) => {
                remove_set.insert(old);
                add_set.insert(new);
            }
            (Some(old), None) => {
                remove_set.insert(old);
            }
            (None, Some(new)) => {
                add_set.insert(new);
            }
            (None, None) => {}
        }
    }

    // ignore any that are added and removed
    for rpath in add_set.difference(&remove_set) {
        cmd.arg("-add_rpath").arg(rpath);
    }
    for rpath in remove_set.difference(&add_set) {
        cmd.arg("-delete_rpath").arg(rpath);
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
    use fs_err as fs;
    use std::{
        collections::{HashMap, HashSet},
        path::{Path, PathBuf},
    };
    use tempfile::tempdir_in;

    use super::{RelinkError, install_name_tool};
    use crate::{
        macos::link::{Dylib, DylibChanges},
        system_tools::SystemTools,
    };
    use crate::{post_process::relink::Relinker, recipe::parser::GlobVec};

    const EXPECTED_PATH: &str = "/Users/wolfv/Programs/rattler-build/output/bld/rattler-build_zlink_1705569778/host_env_placehold_placehold_placehold_placehold_placehold_placehold_placehold_placehold_placehold_placehold_placehold_placehold_placehold_placehold_placehold_placehold_placehol/lib";

    #[test]
    fn test_relink_builtin() -> Result<(), RelinkError> {
        let prefix = Path::new(env!("CARGO_MANIFEST_DIR")).join("test-data/binary_files");
        let tmp_dir = tempdir_in(&prefix)?;
        let binary_path = tmp_dir.path().join("zlink");
        fs::copy(prefix.join("zlink-macos"), &binary_path)?;

        let object = Dylib::new(&binary_path).unwrap();
        assert!(Dylib::test_file(&binary_path)?);
        let expected_rpath = PathBuf::from(EXPECTED_PATH);

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
    fn test_relink_install_name_tool() -> Result<(), RelinkError> {
        let prefix = Path::new(env!("CARGO_MANIFEST_DIR")).join("test-data/binary_files");
        let tmp_dir = tempdir_in(&prefix)?;
        let binary_path = tmp_dir.path().join("zlink");
        fs::copy(prefix.join("zlink-macos"), &binary_path)?;

        let object = Dylib::new(&binary_path).unwrap();
        assert!(Dylib::test_file(&binary_path)?);
        let expected_rpath = PathBuf::from(EXPECTED_PATH);
        // first change the rpath to just @loader_path
        let changes = DylibChanges {
            change_rpath: vec![(
                Some(expected_rpath.clone()),
                Some(PathBuf::from("@loader_path/")),
            )],
            change_id: None,
            change_dylib: HashMap::default(),
        };

        super::relink(&binary_path, &changes)?;

        assert_eq!(object.rpaths, vec![expected_rpath.clone()]);

        let changes = DylibChanges {
            change_rpath: vec![
                (
                    Some("@loader_path/".into()),
                    Some("@loader_path/../../../".into()),
                ),
                (None, Some("@loader_path/".into())),
            ],
            change_id: None,
            change_dylib: HashMap::default(),
        };

        let system_tools = SystemTools::default();
        super::install_name_tool(&binary_path, &changes, &system_tools)?;

        let rpaths = Dylib::new(&binary_path)?.rpaths;
        assert_eq!(
            rpaths,
            vec![
                PathBuf::from("@loader_path/"),
                PathBuf::from("@loader_path/../../../")
            ]
        );

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
        assert!(Dylib::test_file(&binary_path)?);

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

        install_name_tool(&binary_path, &changes, &SystemTools::default())?;

        let object = Dylib::new(&binary_path)?;
        assert!(object.rpaths.is_empty());

        let expected_rpath = PathBuf::from("/Users/blabla/myrpath");
        let changes = DylibChanges {
            change_rpath: vec![(None, Some(expected_rpath.clone()))],
            change_id: None,
            change_dylib: HashMap::default(),
        };

        install_name_tool(&binary_path, &changes, &SystemTools::default())?;

        let object = Dylib::new(&binary_path)?;
        assert_eq!(vec![expected_rpath], object.rpaths);

        Ok(())
    }

    #[test]
    fn test_keep_relative_rpath() -> Result<(), RelinkError> {
        // check if install_name_tool is installed
        if which::which("install_name_tool").is_err() {
            println!("install_name_tool not found, skipping test");
            return Ok(());
        }

        let prefix = Path::new(env!("CARGO_MANIFEST_DIR")).join("test-data/binary_files");
        let tmp_dir = tempdir_in(&prefix)?;
        let bin_dir = tmp_dir.path().join("bin");
        fs::create_dir(bin_dir)?;
        let binary_path = tmp_dir.path().join("bin/zlink-relink-relative");
        fs::copy(prefix.join("zlink-macos"), &binary_path)?;

        let object = Dylib::new(&binary_path).unwrap();
        assert!(Dylib::test_file(&binary_path)?);

        let delete_paths = object
            .rpaths
            .iter()
            .map(|p| (Some(p.clone()), None))
            .chain(std::iter::once((
                None,
                Some(PathBuf::from("@loader_path/../lib")),
            )))
            .collect();

        let changes = DylibChanges {
            change_rpath: delete_paths,
            change_id: None,
            change_dylib: HashMap::default(),
        };

        install_name_tool(&binary_path, &changes, &SystemTools::default())?;

        let object = Dylib::new(&binary_path)?;
        assert!(object.rpaths == vec![PathBuf::from("@loader_path/../lib")]);

        let tmp_prefix = tmp_dir.path();
        let encoded_prefix = PathBuf::from("/encoded/long_install_prefix/bla/bin");

        object
            .relink(
                tmp_prefix,
                &encoded_prefix,
                &[],
                &GlobVec::default(),
                &SystemTools::default(),
            )
            .unwrap();

        let object = Dylib::new(&binary_path)?;
        assert_eq!(vec![PathBuf::from("@loader_path/../lib")], object.rpaths);

        Ok(())
    }

    #[test]
    fn test_rpath_resolve() {
        let dylib = Dylib {
            path: PathBuf::from("/foo/prefix/bar.dylib"),
            id: None,
            rpaths: vec![PathBuf::from("@loader_path/../lib")],
            libraries: HashSet::new(),
        };

        let prefix = PathBuf::from("/foo/prefix");
        let encoded_prefix = PathBuf::from("/foo/very_long_encoded_prefix/bin");

        let resolved = dylib.resolve_rpath(
            &PathBuf::from("@loader_path/../lib"),
            &prefix,
            &encoded_prefix,
        );
        assert_eq!(resolved, PathBuf::from("/foo/very_long_encoded_prefix/lib"));
    }
}
