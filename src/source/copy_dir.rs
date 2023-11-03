use std::{
    collections::HashMap,
    fs::create_dir_all,
    path::{Path, PathBuf},
    sync::Arc,
};

use fs_extra::dir::CopyOptions;
use ignore::WalkBuilder;

use super::SourceError;

/// The copy_dir function accepts additionally a list of globs to ignore or include in the copy process.
/// It uses the `ignore` crate to read the `.gitignore` file in the source directory and uses the globs
/// to filter the files and directories to copy.
///
/// The copy process also ignores hidden files and directories by default.
///
/// # Return
///
/// The returned `Vec<PathBuf>` contains the pathes of the copied files.
/// The `bool` flag indicates whether any of the _include_ globs matched.
/// If a directory is created in this function, the path to the directory is _not_ returned.
pub(crate) struct CopyDir<'a> {
    from_path: &'a Path,
    to_path: &'a Path,
    include_globs: Vec<&'a str>,
    exclude_globs: Vec<&'a str>,
    use_gitignore: bool,
    use_git_global: bool,
    hidden: bool,
    copy_options: CopyOptions,
}

impl<'a> CopyDir<'a> {
    pub fn new(from_path: &'a Path, to_path: &'a Path) -> Self {
        Self {
            from_path,
            to_path,
            include_globs: Vec::new(),
            exclude_globs: Vec::new(),
            use_gitignore: false,
            use_git_global: false,
            hidden: false,
            copy_options: CopyOptions::new(),
        }
    }

    /// Parse the iterator of &str as globs
    ///
    /// This is a conveniance helper for parsing an iterator of &str as include and exclude globs.
    ///
    /// # Note
    ///
    /// Uses '~' as negation character (exclude globs)
    pub fn with_parse_globs<I>(mut self, globs: I) -> Self
    where
        I: IntoIterator<Item = &'a str>,
    {
        let (include_globs, exclude_globs): (Vec<_>, Vec<_>) = globs
            .into_iter()
            .partition(|g| g.trim_start().starts_with('~'));

        self.include_globs.extend(include_globs);
        self.exclude_globs
            .extend(exclude_globs.into_iter().map(|g| g.trim_start_matches('~')));

        self
    }

    #[allow(unused)]
    pub fn with_include_glob(mut self, include: &'a str) -> Self {
        self.include_globs.push(include);
        self
    }

    pub fn with_include_globs<I>(mut self, includes: I) -> Self
    where
        I: IntoIterator<Item = &'a str>,
    {
        self.include_globs.extend(includes);
        self
    }

    #[allow(unused)]
    pub fn with_exclude_glob(mut self, exclude: &'a str) -> Self {
        self.exclude_globs.push(exclude);
        self
    }

    pub fn with_exclude_globs<I>(mut self, excludes: I) -> Self
    where
        I: IntoIterator<Item = &'a str>,
    {
        self.exclude_globs.extend(excludes);
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
    pub fn hidden(mut self, b: bool) -> Self {
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

    #[allow(unused)]
    pub fn content_only(mut self, b: bool) -> Self {
        self.copy_options.content_only = b;
        self
    }

    pub fn run(self) -> Result<CopyDirResult<'a>, SourceError> {
        // Create the to path because we're going to copy the contents only
        create_dir_all(self.to_path).unwrap();

        let (folders, globs) = self
            .include_globs
            .into_iter()
            .partition::<Vec<_>, _>(|glob| glob.ends_with('/') && !glob.contains('*'));

        let folders = Arc::new(folders.into_iter().map(PathBuf::from).collect::<Vec<_>>());

        let mut result = CopyDirResult {
            copied_pathes: Vec::with_capacity(0), // do not allocate as we overwrite this anyways
            include_globs: make_glob_match_map(globs)?,
            exclude_globs: make_glob_match_map(self.exclude_globs)?,
        };

        let copied_pathes = WalkBuilder::new(self.from_path)
            // disregard global gitignore
            .git_global(self.use_git_global)
            .git_ignore(self.use_gitignore)
            .hidden(self.hidden)
            .build()
            .filter_map(|entry| {
                let entry = match entry {
                    Ok(entry) => entry,
                    Err(e) => return Some(Err(e)),
                };

                // if the entry is a directory, ignore it for the final output
                if entry
                    .file_type()
                    .as_ref()
                    .map(|ft| ft.is_dir())
                    .unwrap_or(false)
                {
                    // if the dir is empty, check if we should create it anyways
                    if !entry.path().read_dir().unwrap().next().is_none() {
                        return None;
                    } else {
                        if !result.include_globs().is_empty() {
                            return None;
                        }
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

                let include = result.include_globs().is_empty();

                let include = include
                    || result
                        .include_globs_mut()
                        .iter_mut()
                        .filter(|(_, m)| m.is_match(&stripped_path))
                        .map(|(_, g)| g.set_matched(true))
                        .count()
                        != 0;

                let include =
                    include || folders.clone().iter().any(|f| stripped_path.starts_with(f));

                let exclude = result
                    .exclude_globs_mut()
                    .iter_mut()
                    .filter(|(_, m)| m.is_match(&stripped_path))
                    .map(|(_, g)| g.set_matched(true))
                    .count()
                    != 0;

                (include && !exclude).then_some(Ok(entry))
            })
            .map(|entry| {
                let entry = entry?;
                let path = entry.path();

                let stripped_path = path.strip_prefix(self.from_path)?;
                let dest_path = self.to_path.join(stripped_path);

                if path.is_dir() {
                    // create the empty dir
                    create_dir_all(&dest_path)?;
                    Ok(Some(dest_path))
                } else {
                    // create dir if parent does not exist
                    if let Some(parent) = dest_path.parent() {
                        if !parent.exists() {
                            create_dir_all(parent)?;
                        }
                    }

                    let file_options = fs_extra::file::CopyOptions {
                        overwrite: self.copy_options.overwrite,
                        skip_exist: self.copy_options.skip_exist,
                        buffer_size: self.copy_options.buffer_size,
                    };
                    fs_extra::file::copy(path, &dest_path, &file_options)
                        .map_err(SourceError::FileSystemError)?;

                    tracing::info!(
                        "Copied {} to {}",
                        path.to_string_lossy(),
                        dest_path.to_string_lossy()
                    );
                    Ok(Some(dest_path))
                }
            })
            .filter_map(|res| res.transpose())
            .collect::<Result<Vec<_>, SourceError>>()?;

        result.copied_pathes = copied_pathes;
        Ok(result)
    }
}

pub(crate) struct CopyDirResult<'a> {
    copied_pathes: Vec<PathBuf>,
    include_globs: HashMap<Glob<'a>, Match>,
    exclude_globs: HashMap<Glob<'a>, Match>,
}

impl<'a> CopyDirResult<'a> {
    pub fn copied_pathes(&self) -> &[PathBuf] {
        &self.copied_pathes
    }

    pub fn include_globs(&self) -> &HashMap<Glob<'a>, Match> {
        &self.include_globs
    }

    fn include_globs_mut(&mut self) -> &mut HashMap<Glob<'a>, Match> {
        &mut self.include_globs
    }

    pub fn any_include_glob_matched(&self) -> bool {
        self.include_globs.values().any(|m| m.get_matched())
    }

    #[allow(unused)]
    pub fn exclude_globs(&self) -> &HashMap<Glob<'a>, Match> {
        &self.exclude_globs
    }

    fn exclude_globs_mut(&mut self) -> &mut HashMap<Glob<'a>, Match> {
        &mut self.exclude_globs
    }

    #[allow(unused)]
    pub fn any_exclude_glob_matched(&self) -> bool {
        self.exclude_globs.values().any(|m| m.get_matched())
    }
}

fn make_glob_match_map(globs: Vec<&str>) -> Result<HashMap<Glob, Match>, SourceError> {
    globs
        .into_iter()
        .map(|gl| {
            let glob = Glob::new(gl)?;
            let match_ = Match::new(&glob);
            Ok((glob, match_))
        })
        .collect()
}

#[derive(Hash, Eq, PartialEq)]
pub(crate) struct Glob<'a> {
    s: &'a str,
    g: globset::Glob,
}

impl<'a> Glob<'a> {
    fn new(s: &'a str) -> Result<Self, SourceError> {
        Ok(Self {
            s,
            g: globset::Glob::new(s)?,
        })
    }
}

pub(crate) struct Match {
    matcher: globset::GlobMatcher,
    matched: bool,
}

impl Match {
    fn new(glob: &Glob) -> Self {
        Self {
            matcher: glob.g.compile_matcher(),
            matched: false,
        }
    }

    #[inline]
    fn set_matched(&mut self, b: bool) {
        self.matched = b;
    }

    #[inline]
    fn get_matched(&self) -> bool {
        self.matched
    }

    #[inline]
    fn is_match<P: AsRef<Path>>(&self, p: P) -> bool {
        self.matcher.is_match(p)
    }
}

#[cfg(test)]
mod test {
    use std::{collections::HashSet, fs, fs::File};

    #[test]
    fn test_copy_dir() {
        let tmp_dir = tempfile::TempDir::new().unwrap();
        let tmp_dir_path = tmp_dir.into_path();
        let dir = tmp_dir_path.as_path().join("test_copy_dir");

        fs_extra::dir::create_all(&dir, true).unwrap();

        // test.txt
        // test_dir/test.md
        // test_dir/test_dir2/

        std::fs::write(dir.join("test.txt"), "test").unwrap();
        std::fs::create_dir(dir.join("test_dir")).unwrap();
        std::fs::write(dir.join("test_dir").join("test.md"), "test").unwrap();
        std::fs::create_dir(dir.join("test_dir").join("test_dir2")).unwrap();

        let dest_dir = tmp_dir_path.as_path().join("test_copy_dir_dest");
        let _copy_dir = super::CopyDir::new(&dir, &dest_dir)
            .use_gitignore(false)
            .run()
            .unwrap();

        assert_eq!(dest_dir.exists(), true);
        assert_eq!(dest_dir.is_dir(), true);
        assert_eq!(dest_dir.join("test.txt").exists(), true);
        assert_eq!(dest_dir.join("test_dir").exists(), true);
        assert_eq!(dest_dir.join("test_dir").join("test.md").exists(), true);
        assert_eq!(dest_dir.join("test_dir").join("test_dir2").exists(), true);

        let dest_dir_2 = tmp_dir_path.as_path().join("test_copy_dir_dest_2");
        // ignore all txt files
        let copy_dir = super::CopyDir::new(&dir, &dest_dir_2)
            .with_include_glob("*.txt")
            .use_gitignore(false)
            .run()
            .unwrap();

        assert_eq!(copy_dir.copied_pathes().len(), 1);
        assert_eq!(copy_dir.copied_pathes()[0], dest_dir_2.join("test.txt"));

        let dest_dir_3 = tmp_dir_path.as_path().join("test_copy_dir_dest_3");
        // ignore all txt files
        let copy_dir = super::CopyDir::new(&dir, &dest_dir_3)
            .with_exclude_glob("*.txt")
            .use_gitignore(false)
            .run()
            .unwrap();

        assert_eq!(copy_dir.copied_pathes().len(), 2);
        let expected = [
            dest_dir_3.join("test_dir/test.md"),
            dest_dir_3.join("test_dir/test_dir2"),
        ];
        let expected = expected.iter().collect::<HashSet<_>>();
        let result = copy_dir.copied_pathes().iter().collect::<HashSet<_>>();
        assert_eq!(result, expected);
    }

    #[test]
    fn copy_a_bunch_of_files() {
        let tmp_dir = tempfile::TempDir::new().unwrap();
        let dir = tmp_dir.path().join("test_copy_dir");

        fs::create_dir_all(&dir).unwrap();
        File::create(&dir.join("test_1.txt")).unwrap();
        File::create(&dir.join("test_2.rst")).unwrap();

        let dest_dir = tempfile::TempDir::new().unwrap();

        let copy_dir = super::CopyDir::new(tmp_dir.path(), dest_dir.path())
            .with_include_glob("test_copy_dir/")
            .use_gitignore(false)
            .run()
            .unwrap();
        assert_eq!(copy_dir.copied_pathes().len(), 2);

        fs_extra::dir::create_all(&dest_dir, true).unwrap();
        let copy_dir = super::CopyDir::new(tmp_dir.path(), dest_dir.path())
            .with_include_glob("test_copy_dir/")
            .with_exclude_glob("*.rst")
            .use_gitignore(false)
            .run()
            .unwrap();
        assert_eq!(copy_dir.copied_pathes().len(), 1);
        assert_eq!(
            copy_dir.copied_pathes()[0],
            dest_dir.path().join("test_copy_dir/test_1.txt")
        );
    }
}
