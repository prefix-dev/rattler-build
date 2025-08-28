//! Copy a directory to another location using globs to filter the files and directories to copy.
use std::{
    collections::{HashMap, HashSet},
    fs::FileTimes,
    path::{Path, PathBuf},
};

use fs_err::create_dir_all;

use globset::Glob;
use ignore::WalkBuilder;
use rayon::iter::{ParallelBridge, ParallelIterator};

use crate::recipe::parser::{GlobVec, GlobWithSource};

use super::SourceError;

/// The copy options for the copy_dir function.
pub struct CopyOptions {
    /// Overwrite files if they already exist (default: false)
    pub overwrite: bool,
    /// Skip files if they already exist (default: false)
    pub skip_exist: bool,
    /// Buffer size for copying files (default: 8 MiB)
    pub buffer_size: usize,
}

impl Default for CopyOptions {
    fn default() -> Self {
        Self {
            overwrite: false,
            skip_exist: false,
            buffer_size: 8 * 1024 * 1024,
        }
    }
}

/// Copy metadata from source to destination
/// `fs::copy` handles permissions, but it won't be called if the file is reflinked
/// We need to deal with permissions and timestamps ourselves
fn copy_metadata(from: &Path, to: &Path) -> std::io::Result<()> {
    let metadata = fs_err::metadata(from)?;

    // Copy timestamps using std::fs::FileTimes
    let file_times = FileTimes::new()
        .set_accessed(metadata.accessed()?)
        .set_modified(metadata.modified()?);

    let file = std::fs::OpenOptions::new().write(true).open(to)?;
    file.set_times(file_times)?;
    file.set_permissions(metadata.permissions())?;

    Ok(())
}

/// Cross platform way of creating a symlink
/// Creates a symlink from `link` to `original`
/// The file that is newly created is the `link` file
pub(crate) fn create_symlink(
    original: impl AsRef<Path>,
    link: impl AsRef<Path>,
) -> Result<(), SourceError> {
    let original = original.as_ref();
    let link = link.as_ref();

    if link.exists() {
        fs_err::remove_file(link)?;
    }

    #[cfg(unix)]
    fs_err::os::unix::fs::symlink(original, link)?;
    #[cfg(windows)]
    {
        if original.is_dir() {
            std::os::windows::fs::symlink_dir(original, link)?;
        } else {
            std::os::windows::fs::symlink_file(original, link)?;
        }
    }

    Ok(())
}

/// Copy a file or directory, or symlink to another location.
/// Use reflink if possible.
pub(crate) fn copy_file(
    from: impl AsRef<Path>,
    to: impl AsRef<Path>,
    paths_created: &mut HashSet<PathBuf>,
    options: &CopyOptions,
) -> Result<(), SourceError> {
    let path = from.as_ref();
    let dest_path = to.as_ref();

    // if file is a symlink, copy it as a symlink. Note: it can be a symlink to a file or directory
    if path.is_symlink() {
        let link_target = fs_err::read_link(path)?;

        if let Some(parent) = dest_path.parent() {
            create_dir_all_cached(parent, paths_created)?;
        }

        create_symlink(link_target, dest_path)?;
        Ok(())
    } else if path.is_dir() {
        create_dir_all_cached(dest_path, paths_created)?;
        Ok(())
    } else {
        // create dir if parent does not exist
        if let Some(parent) = dest_path.parent() {
            create_dir_all_cached(parent, paths_created)?;
        }

        if dest_path.exists() {
            if !(options.overwrite || options.skip_exist) {
                tracing::error!("File already exists: {:?}", dest_path);
            } else if options.skip_exist {
                tracing::warn!("File already exists! Skipping file: {:?}", dest_path);
            } else if options.overwrite {
                tracing::warn!("File already exists! Overwriting file: {:?}", dest_path);
            }
        }
        reflink_or_copy(path, dest_path, options).map_err(SourceError::FileSystemError)?;
        Ok(())
    }
}

/// The copy_dir function accepts additionally a list of globs to ignore or include in the copy process.
/// It uses the `ignore` crate to read the `.gitignore` file in the source directory and uses the globs
/// to filter the files and directories to copy.
///
/// # Return
///
/// The returned `Vec<PathBuf>` contains the paths of the copied files.
/// The `bool` flag indicates whether any of the _include_ globs matched.
/// If a directory is created in this function, the path to the directory is _not_ returned.
pub(crate) struct CopyDir<'a> {
    from_path: &'a Path,
    to_path: &'a Path,
    globvec: GlobVec,
    use_gitignore: bool,
    use_git_global: bool,
    use_condapackageignore: bool,
    hidden: bool,
    copy_options: CopyOptions,
}

impl<'a> CopyDir<'a> {
    pub fn new(from_path: &'a Path, to_path: &'a Path) -> Self {
        Self {
            from_path,
            to_path,
            globvec: GlobVec::default(),
            // use the gitignore file by default
            use_gitignore: false,
            // use the global git ignore file by default
            use_git_global: false,
            // use .condapackageignore files by default
            use_condapackageignore: true,
            // include hidden files by default
            hidden: false,
            copy_options: CopyOptions::default(),
        }
    }

    pub fn with_globvec(mut self, globvec: &GlobVec) -> Self {
        self.globvec = globvec.clone();
        self
    }

    pub fn use_gitignore(mut self, b: bool) -> Self {
        self.use_gitignore = b;
        self
    }

    #[allow(unused)]
    pub fn use_git_global(mut self, b: bool) -> Self {
        self.use_git_global = b;
        self
    }

    #[allow(unused)]
    pub fn use_condapackageignore(mut self, b: bool) -> Self {
        self.use_condapackageignore = b;
        self
    }

    #[allow(unused)]
    pub fn ignore_hidden_files(mut self, b: bool) -> Self {
        self.hidden = b;
        self
    }

    /// Setup copy options, overwrite if needed, only copy the contents as we want to specify the
    /// dir name manually
    #[allow(unused)]
    pub fn with_copy_options(mut self, copy_options: CopyOptions) -> Self {
        self.copy_options = copy_options;
        self
    }

    #[allow(unused)]
    pub fn overwrite(mut self, b: bool) -> Self {
        self.copy_options.overwrite = b;
        self
    }

    pub fn run(self) -> Result<CopyDirResult, SourceError> {
        // Create the to path because we're going to copy the contents only
        create_dir_all(self.to_path)?;

        let mut result = CopyDirResult {
            copied_paths: Vec::with_capacity(0), // do not allocate as we overwrite this anyways
            include_globs: make_glob_match_map(self.globvec.include_globs())?,
            exclude_globs: make_glob_match_map(self.globvec.exclude_globs())?,
        };

        let mut walk_builder = WalkBuilder::new(self.from_path);
        walk_builder
            // disregard global gitignore
            .git_global(self.use_git_global)
            // ignore any .gitignore files from parent directories
            .parents(false)
            .git_ignore(self.use_gitignore)
            // Always disable .ignore files - they should not affect source copying
            .ignore(false)
            .hidden(self.hidden);
        if self.use_condapackageignore {
            walk_builder.add_custom_ignore_filename(".condapackageignore");
        }

        let copied_paths = walk_builder
            .build()
            .filter_map(|entry| {
                let entry = match entry {
                    Ok(entry) => entry,
                    Err(e) => return Some(Err(e)),
                };

                let is_dir = entry
                    .file_type()
                    .as_ref()
                    .map(|ft| ft.is_dir())
                    .unwrap_or(false);
                // if the entry is a directory, ignore it for the final output
                if is_dir {
                    // if the dir is empty, check if we should create it anyways
                    if entry.path().read_dir().ok()?.next().is_some()
                        || !result.include_globs().is_empty()
                    {
                        return None;
                    }
                }

                // We need to strip the path to the entry to make sure that the glob matches on relative paths
                let stripped_path: PathBuf = {
                    let mut components: Vec<_> = entry
                        .path()
                        .components()
                        .rev()
                        .take(entry.depth())
                        .collect();
                    components.reverse();
                    components.iter().collect()
                };

                // include everything
                let include = result.include_globs().is_empty();

                let include = include
                    || result
                        .include_globs_mut()
                        .iter_mut()
                        .filter(|(_, m)| m.is_match(&stripped_path))
                        .map(|(_, g)| g.set_matched(true))
                        .count()
                        != 0;

                let exclude = result
                    .exclude_globs_mut()
                    .iter_mut()
                    .filter(|(_, m)| m.is_match(&stripped_path))
                    .map(|(_, g)| g.set_matched(true))
                    .count()
                    != 0;

                (include && !exclude).then_some(Ok(entry))
            })
            .par_bridge()
            .map_with(
                HashSet::from_iter([self.to_path.to_path_buf()]),
                |paths_created: &mut HashSet<PathBuf>, entry| {
                    let entry = entry?;
                    let path = entry.path();

                    let stripped_path = path.strip_prefix(self.from_path)?;
                    let dest_path = self.to_path.join(stripped_path);

                    if path.is_symlink() {
                        let link_target = fs_err::read_link(path)?;

                        if let Some(parent) = dest_path.parent() {
                            create_dir_all_cached(parent, paths_created)?;
                        }

                        create_symlink(link_target, &dest_path)?;
                        Ok(Some(dest_path))
                    } else if path.is_dir() {
                        create_dir_all_cached(&dest_path, paths_created)?;
                        Ok(Some(dest_path))
                    } else {
                        // create dir if parent does not exist
                        if let Some(parent) = dest_path.parent() {
                            create_dir_all_cached(parent, paths_created)?;
                        }

                        if dest_path.exists() {
                            if !(self.copy_options.overwrite || self.copy_options.skip_exist) {
                                tracing::error!("File already exists: {:?}", dest_path);
                            } else if self.copy_options.skip_exist {
                                tracing::warn!(
                                    "File already exists! Skipping file: {:?}",
                                    dest_path
                                );
                            } else if self.copy_options.overwrite {
                                tracing::warn!(
                                    "File already exists! Overwriting file: {:?}",
                                    dest_path
                                );
                            }
                        }
                        reflink_or_copy(path, &dest_path, &self.copy_options)
                            .map_err(SourceError::FileSystemError)?;

                        Ok(Some(dest_path))
                    }
                },
            )
            .filter_map(|res| res.transpose())
            .collect::<Result<Vec<_>, SourceError>>()?;

        result.copied_paths = copied_paths;
        Ok(result)
    }
}

/// Recursively creates directories and keeps an in-memory cache of the directories that have been
/// created before. This speeds up creation of large amounts of directories significantly because
/// there are fewer IO operations.
fn create_dir_all_cached(path: &Path, paths_created: &mut HashSet<PathBuf>) -> std::io::Result<()> {
    // Find the first directory that is not already created
    let mut dirs_to_create = Vec::new();
    let mut path = path;
    loop {
        if paths_created.contains(path) {
            break;
        }

        if path.is_dir() {
            paths_created.insert(path.to_path_buf());
            break;
        }

        dirs_to_create.push(path.to_path_buf());
        path = match path.parent() {
            Some(path) => path,
            None => break,
        }
    }

    // Actually create the directories
    for path in dirs_to_create.into_iter().rev() {
        match fs_err::create_dir(&path) {
            Err(e) if e.kind() == std::io::ErrorKind::AlreadyExists => {}
            Ok(()) => {}
            Err(e) => return Err(e),
        }

        paths_created.insert(path);
    }

    Ok(())
}

/// Reflinks or copies a file. If reflinking fails the file is copied instead.
///
/// The implementation of this function is partially taken from fs_extra.
pub fn reflink_or_copy<P, Q>(from: P, to: Q, options: &CopyOptions) -> std::io::Result<()>
where
    P: AsRef<Path>,
    Q: AsRef<Path>,
{
    let from = from.as_ref();
    if !from.exists() {
        let msg = format!(
            "Path \"{}\" does not exist or you don't have access!",
            from.to_str().unwrap_or("???")
        );
        return Err(std::io::Error::new(std::io::ErrorKind::NotFound, msg));
    }

    if !from.is_file() {
        let msg = format!("Path \"{}\" is not a file!", from.to_str().unwrap_or("???"));
        return Err(std::io::Error::new(std::io::ErrorKind::InvalidInput, msg));
    }

    if to.as_ref().exists() {
        if !options.overwrite {
            if options.skip_exist {
                return Ok(());
            }

            let msg = format!(
                "Path \"{}\" already exists!",
                to.as_ref().to_str().unwrap_or("???")
            );
            return Err(std::io::Error::new(std::io::ErrorKind::AlreadyExists, msg));
        }

        // Reflinking on windows cannot overwrite files. It will fail with a permission denied error.
        fs_err::remove_file(&to)?;
    }

    // Reflink or copy the file
    match reflink_copy::reflink_or_copy(from, &to) {
        Ok(None) => {
            // File has been reflinked
            #[cfg(target_os = "linux")]
            {
                copy_metadata(from, to.as_ref())?;
            }
        }
        Ok(Some(_)) => {
            // File has been copied
            match copy_metadata(from, to.as_ref()) {
                Ok(()) => {}
                Err(e) => {
                    tracing::debug!("Failed to copy metadata for {:?} {:?}", to.as_ref(), e);
                }
            }
        }
        Err(e) => {
            return Err(e);
        }
    }

    Ok(())
}

pub(crate) struct CopyDirResult {
    copied_paths: Vec<PathBuf>,
    include_globs: HashMap<String, Match>,
    exclude_globs: HashMap<String, Match>,
}

impl CopyDirResult {
    pub fn copied_paths(&self) -> &[PathBuf] {
        &self.copied_paths
    }

    pub fn include_globs(&self) -> &HashMap<String, Match> {
        &self.include_globs
    }

    fn include_globs_mut(&mut self) -> &mut HashMap<String, Match> {
        &mut self.include_globs
    }

    #[allow(unused)]
    pub fn any_include_glob_matched(&self) -> bool {
        self.include_globs.values().any(|m| m.get_matched())
    }

    #[allow(unused)]
    pub fn exclude_globs(&self) -> &HashMap<String, Match> {
        &self.exclude_globs
    }

    fn exclude_globs_mut(&mut self) -> &mut HashMap<String, Match> {
        &mut self.exclude_globs
    }

    #[allow(unused)]
    pub fn any_exclude_glob_matched(&self) -> bool {
        self.exclude_globs.values().any(|m| m.get_matched())
    }
}

fn make_glob_match_map(globs: &[GlobWithSource]) -> Result<HashMap<String, Match>, SourceError> {
    globs
        .iter()
        .map(|glob| {
            let matcher = Match::new(glob.glob());
            Ok((glob.source().to_string(), matcher))
        })
        .collect()
}

pub(crate) struct Match {
    matcher: globset::GlobMatcher,
    matched: bool,
}

impl Match {
    fn new(glob: &Glob) -> Self {
        Self {
            matcher: glob.compile_matcher(),
            matched: false,
        }
    }

    #[inline]
    fn set_matched(&mut self, b: bool) {
        self.matched = b;
    }

    #[inline]
    pub(crate) fn get_matched(&self) -> bool {
        self.matched
    }

    #[inline]
    fn is_match<P: AsRef<Path>>(&self, p: P) -> bool {
        self.matcher.is_match(p)
    }
}

#[cfg(test)]
mod test {
    use fs_err::{self as fs, File};
    use std::collections::HashSet;

    use crate::recipe::parser::GlobVec;

    #[test]
    fn test_copy_dir() {
        let tmp_dir = tempfile::TempDir::new().unwrap();
        let tmp_dir_path = tmp_dir.keep();
        let dir = tmp_dir_path.as_path().join("test_copy_dir");

        fs_err::create_dir_all(&dir).unwrap();

        // test.txt
        // test_dir/test.md
        // test_dir/test_dir2/

        fs::write(dir.join("test.txt"), "test").unwrap();
        fs::create_dir(dir.join("test_dir")).unwrap();
        fs::write(dir.join("test_dir").join("test.md"), "test").unwrap();
        fs::create_dir(dir.join("test_dir").join("test_dir2")).unwrap();

        let dest_dir = tmp_dir_path.as_path().join("test_copy_dir_dest");
        let _copy_dir = super::CopyDir::new(&dir, &dest_dir)
            .use_gitignore(false)
            .run()
            .unwrap();

        assert!(dest_dir.exists());
        assert!(dest_dir.is_dir());
        assert!(dest_dir.join("test.txt").exists());
        assert!(dest_dir.join("test_dir").exists());
        assert!(dest_dir.join("test_dir").join("test.md").exists());
        assert!(dest_dir.join("test_dir").join("test_dir2").exists());

        let dest_dir_2 = tmp_dir_path.as_path().join("test_copy_dir_dest_2");
        // ignore all txt files
        let copy_dir = super::CopyDir::new(&dir, &dest_dir_2)
            .with_globvec(&GlobVec::from_vec(vec!["*.txt"], None))
            .use_gitignore(false)
            .run()
            .unwrap();

        assert_eq!(copy_dir.copied_paths().len(), 1);
        assert_eq!(copy_dir.copied_paths()[0], dest_dir_2.join("test.txt"));

        let dest_dir_3 = tmp_dir_path.as_path().join("test_copy_dir_dest_3");

        // ignore all txt files
        let copy_dir = super::CopyDir::new(&dir, &dest_dir_3)
            .with_globvec(&GlobVec::from_vec(vec![], Some(vec!["*.txt"])))
            .use_gitignore(false)
            .run()
            .unwrap();

        assert_eq!(copy_dir.copied_paths().len(), 2);
        let expected = [
            dest_dir_3.join("test_dir/test.md"),
            dest_dir_3.join("test_dir/test_dir2"),
        ];
        let expected = expected.iter().collect::<HashSet<_>>();
        let result = copy_dir.copied_paths().iter().collect::<HashSet<_>>();
        assert_eq!(result, expected);
    }

    #[test]
    fn copy_a_bunch_of_files() {
        let tmp_dir = tempfile::TempDir::new().unwrap();
        let dir = tmp_dir.path().join("test_copy_dir");

        fs::create_dir_all(&dir).unwrap();
        File::create(dir.join("test_1.txt")).unwrap();
        File::create(dir.join("test_2.rst")).unwrap();

        let dest_dir = tempfile::TempDir::new().unwrap();

        let copy_dir = super::CopyDir::new(tmp_dir.path(), dest_dir.path())
            .with_globvec(&GlobVec::from_vec(vec!["test_copy_dir/"], None))
            .use_gitignore(false)
            .run()
            .unwrap();
        assert_eq!(copy_dir.copied_paths().len(), 2);

        fs_err::remove_dir_all(&dest_dir).unwrap();
        fs_err::create_dir_all(&dest_dir).unwrap();
        let copy_dir = super::CopyDir::new(tmp_dir.path(), dest_dir.path())
            .with_globvec(&GlobVec::from_vec(
                vec!["test_copy_dir/test_1.txt"],
                Some(vec!["*.rst"]),
            ))
            .use_gitignore(false)
            .run()
            .unwrap();
        assert_eq!(copy_dir.copied_paths().len(), 1);
        assert_eq!(
            copy_dir.copied_paths()[0],
            dest_dir.path().join("test_copy_dir/test_1.txt")
        );

        fs_err::remove_dir_all(&dest_dir).unwrap();
        fs_err::create_dir_all(&dest_dir).unwrap();
        let copy_dir = super::CopyDir::new(tmp_dir.path(), dest_dir.path())
            .with_globvec(&GlobVec::from_vec(vec!["test_copy_dir/test_1.txt"], None))
            .use_gitignore(false)
            .run()
            .unwrap();
        assert_eq!(copy_dir.copied_paths().len(), 1);
        assert_eq!(
            copy_dir.copied_paths()[0],
            dest_dir.path().join("test_copy_dir/test_1.txt")
        );
    }

    #[test]
    fn copydir_with_broken_symlink() {
        #[cfg(windows)]
        {
            // check if we have permissions to create symlinks
            let tmp_dir = tempfile::TempDir::new().unwrap();
            let broken_symlink = tmp_dir.path().join("random_symlink");
            if std::os::windows::fs::symlink_file("does_not_exist", &broken_symlink).is_err() {
                return;
            }
        }

        let tmp_dir = tempfile::TempDir::new().unwrap();
        let dir = tmp_dir.path().join("test_copy_dir");

        fs::create_dir_all(&dir).unwrap();
        File::create(dir.join("test_1.txt")).unwrap();
        File::create(dir.join("test_2.rst")).unwrap();

        let broken_symlink = tmp_dir.path().join("broken_symlink");
        #[cfg(unix)]
        std::os::unix::fs::symlink("/does/not/exist", broken_symlink).unwrap();
        #[cfg(windows)]
        std::os::windows::fs::symlink_file("/does/not/exist", &broken_symlink).unwrap();

        let dest_dir = tempfile::TempDir::new().unwrap();

        let copy_dir = super::CopyDir::new(tmp_dir.path(), dest_dir.path())
            .use_gitignore(false)
            .run()
            .unwrap();
        assert_eq!(copy_dir.copied_paths().len(), 3);

        let broken_symlink_dest = dest_dir.path().join("broken_symlink");
        assert_eq!(
            fs::read_link(broken_symlink_dest).unwrap(),
            std::path::PathBuf::from("/does/not/exist")
        );
    }

    #[test]
    fn test_copy_symlinked_directory() {
        #[cfg(windows)]
        {
            // check if we have permissions to create symlinks
            let tmp_dir = tempfile::TempDir::new().unwrap();
            let test_symlink = tmp_dir.path().join("test_symlink");
            if std::os::windows::fs::symlink_dir("does_not_exist", &test_symlink).is_err() {
                return;
            }
        }

        let tmp_dir = tempfile::TempDir::new().unwrap();
        let dir = tmp_dir.path().join("test_copy_dir");
        fs::create_dir_all(&dir).unwrap();

        // Create a target directory with some content
        let target_dir = tmp_dir.path().join("target_dir");
        fs::create_dir_all(&target_dir).unwrap();
        fs::write(target_dir.join("file_in_target.txt"), "content").unwrap();

        // Create a symlink to the directory
        let symlinked_dir = tmp_dir.path().join("symlinked_dir");
        #[cfg(unix)]
        std::os::unix::fs::symlink(&target_dir, &symlinked_dir).unwrap();
        #[cfg(windows)]
        std::os::windows::fs::symlink_dir(&target_dir, &symlinked_dir).unwrap();

        // Add a regular file as well
        fs::write(tmp_dir.path().join("regular_file.txt"), "regular content").unwrap();

        let dest_dir = tempfile::TempDir::new().unwrap();

        let _copy_dir = super::CopyDir::new(tmp_dir.path(), dest_dir.path())
            .use_gitignore(false)
            .run()
            .unwrap();

        // Check that the symlinked directory was copied as a symlink
        let dest_symlinked_dir = dest_dir.path().join("symlinked_dir");
        assert!(dest_symlinked_dir.exists());
        assert!(dest_symlinked_dir.is_symlink());

        // The symlink should point to the same relative path
        let link_target = fs::read_link(&dest_symlinked_dir).unwrap();
        assert_eq!(link_target, target_dir);

        // Verify other files were copied
        assert!(dest_dir.path().join("regular_file.txt").exists());
        assert!(dest_dir.path().join("target_dir").exists());
        assert!(
            dest_dir
                .path()
                .join("target_dir")
                .join("file_in_target.txt")
                .exists()
        );
    }
}
