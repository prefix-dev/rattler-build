use content_inspector::ContentType;
use fs_err as fs;
use rattler_conda_types::PrefixRecord;
use std::{
    collections::{HashMap, HashSet},
    io::{self, Read},
    path::{Path, PathBuf},
};
use tempfile::TempDir;
use walkdir::WalkDir;

use crate::metadata::Output;

use super::{PackagingError, file_mapper, normalize_path_for_comparison};
use rattler_build_recipe::stage1::GlobVec;

/// Read the list of paths written by the build script into the file pointed at
/// by the `RATTLER_BUILD_PACKAGE_FILES` environment variable.
///
/// Returns `Ok(None)` if the file does not exist or contains no non-empty
/// lines (in which case the caller should fall back to the default file
/// discovery mechanism). Returns `Ok(Some(paths))` if the file exists and
/// contains at least one path.
///
/// Lines are trimmed of whitespace; empty lines are skipped to make
/// `echo path >> $RATTLER_BUILD_PACKAGE_FILES` and similar shell idioms work
/// reliably.
pub fn read_package_files_list(path: &Path) -> Result<Option<Vec<PathBuf>>, io::Error> {
    if !path.exists() {
        return Ok(None);
    }

    let contents = fs::read_to_string(path)?;
    let paths: Vec<PathBuf> = contents
        .lines()
        .map(|line| line.trim())
        .filter(|line| !line.is_empty())
        .map(PathBuf::from)
        .collect();

    if paths.is_empty() {
        Ok(None)
    } else {
        Ok(Some(paths))
    }
}

/// A wrapper around PathBuf that implements case-insensitive hashing and equality
/// when the filesystem is case-insensitive
#[derive(Debug, Clone)]
struct CaseInsensitivePath {
    path: String,
}

impl CaseInsensitivePath {
    fn new(path: &Path) -> Self {
        Self {
            path: normalize_path_for_comparison(path, true).unwrap(),
        }
    }
}

impl std::hash::Hash for CaseInsensitivePath {
    fn hash<H: std::hash::Hasher>(&self, state: &mut H) {
        // Convert to lowercase string for case-insensitive hashing
        self.path.hash(state);
    }
}

impl PartialEq for CaseInsensitivePath {
    fn eq(&self, other: &Self) -> bool {
        // Case-insensitive comparison
        self.path == other.path
    }
}

impl Eq for CaseInsensitivePath {}

/// This struct keeps a record of all the files that are new in the prefix (i.e. not present in the previous
/// conda environment).
#[derive(Debug)]
pub struct Files {
    /// The files that are new in the prefix
    pub new_files: HashSet<PathBuf>,
    /// The files that were present in the original conda environment
    pub old_files: HashSet<PathBuf>,
    /// The prefix that we are dealing with
    pub prefix: PathBuf,
}

/// This struct keeps a record of all the files that are moved into a temporary directory
/// for further post-processing (before they are packaged into a tarball).
#[derive(Debug)]
pub struct TempFiles {
    /// The files that are copied to the temporary directory
    pub files: HashSet<PathBuf>,
    /// The temporary directory where the files are copied to
    pub temp_dir: tempfile::TempDir,
    /// The prefix which is encoded in the files (the long placeholder for the actual prefix, e.g. /home/user/bld_placeholder...)
    pub encoded_prefix: PathBuf,
    /// The content type of the files
    content_type_map: HashMap<PathBuf, Option<ContentType>>,
}

/// Determine the content type of a path by reading the first 1024 bytes of the file
/// and checking for a BOM or NULL-byte.
pub fn content_type(path: &Path) -> Result<Option<ContentType>, io::Error> {
    if path.is_dir() || path.is_symlink() {
        return Ok(None);
    }

    // read first 1024 bytes to determine file type
    let mut file = fs::File::open(path)?;
    let mut buffer = [0; 1024];
    let n = file.read(&mut buffer)?;
    let buffer = &buffer[..n];

    Ok(Some(content_inspector::inspect(buffer)))
}

/// Collect the set of files contributed by packages already installed in the
/// given prefix. These are the files that should be considered "old" and that
/// would normally be excluded from the package contents.
fn collect_previous_files(prefix: &Path) -> Result<HashSet<PathBuf>, io::Error> {
    if !prefix.exists() || !prefix.join("conda-meta").exists() {
        return Ok(HashSet::new());
    }

    let prefix_records: Vec<PrefixRecord> = PrefixRecord::collect_from_prefix(prefix)?;
    let mut previous_files = prefix_records
        .into_iter()
        .fold(HashSet::new(), |mut acc, record| {
            acc.extend(record.files.iter().map(|f| prefix.join(f)));
            acc
        });

    // Also include the existing conda-meta (PrefixRecord) files themselves
    previous_files.extend(record_files(&prefix.join("conda-meta"))?);
    Ok(previous_files)
}

/// This function returns a HashSet of (recursively) all the files in the given directory.
pub fn record_files(directory: &Path) -> Result<HashSet<PathBuf>, io::Error> {
    let mut res = HashSet::new();
    for entry in WalkDir::new(directory) {
        res.insert(entry?.path().to_owned());
    }
    Ok(res)
}

// Check if the filesystem is case-sensitive by creating a file with a different case
// and checking if it exists.
fn check_is_case_sensitive() -> Result<bool, io::Error> {
    // Check if the filesystem is case insensitive
    let tempdir = TempDir::new()?;
    let file1 = tempdir.path().join("testfile.txt");
    let file2 = tempdir.path().join("TESTFILE.txt");
    fs::File::create(&file1)?;
    Ok(!file2.exists() && file1.exists())
}

/// Helper function to find files that exist in current_files but not in previous_files,
/// taking into account case sensitivity
fn find_new_files(
    current_files: &HashSet<PathBuf>,
    previous_files: &HashSet<PathBuf>,
    prefix: &Path,
    is_case_sensitive: bool,
) -> HashSet<PathBuf> {
    if is_case_sensitive {
        // On case-sensitive filesystems, use normal set difference
        current_files.difference(previous_files).cloned().collect()
    } else {
        // On case-insensitive filesystems, use case-aware comparison
        let previous_case_aware: HashSet<CaseInsensitivePath> = previous_files
            .iter()
            .map(|p| {
                CaseInsensitivePath::new(p.strip_prefix(prefix).expect("File should be in prefix"))
            })
            .collect();

        current_files
            .clone()
            .into_iter()
            .filter(|p| {
                // Only include files that are not in the previous set
                !previous_case_aware.contains(&CaseInsensitivePath::new(
                    p.strip_prefix(prefix).expect("File should be in prefix"),
                ))
            })
            .collect::<HashSet<_>>()
    }
}

impl Files {
    /// Build a [`Files`] struct from an explicit list of paths that should end
    /// up in the package. Each entry must be either an absolute path that lives
    /// inside `prefix`, or a relative path that will be resolved against
    /// `prefix`.
    ///
    /// Paths that do not exist on disk, that escape the prefix, or that resolve
    /// to a directory are skipped (with a warning) so that the caller can keep
    /// the input file format simple. The set of "old files" (i.e. files
    /// contributed by the installed host environment) is still computed so
    /// that downstream packaging logic that depends on it (e.g. the `pyc`
    /// filter) keeps working.
    pub fn from_paths(
        prefix: &Path,
        paths: impl IntoIterator<Item = PathBuf>,
    ) -> Result<Self, io::Error> {
        let old_files = collect_previous_files(prefix)?;

        let mut new_files = HashSet::new();
        for path in paths {
            let resolved = if path.is_absolute() {
                path
            } else {
                prefix.join(path)
            };

            // Make sure the path is inside the prefix.
            if resolved.strip_prefix(prefix).is_err() {
                tracing::warn!(
                    "Ignoring package file `{}`: not inside the prefix `{}`",
                    resolved.display(),
                    prefix.display()
                );
                continue;
            }

            if !resolved.exists() {
                tracing::warn!(
                    "Ignoring package file `{}`: file does not exist",
                    resolved.display()
                );
                continue;
            }

            new_files.insert(resolved);
        }

        Ok(Files {
            new_files,
            old_files,
            prefix: prefix.to_owned(),
        })
    }

    /// Find all files in the given (host) prefix and remove all previously installed files (based on the PrefixRecord
    /// of the conda environment). If always_include is Some, then all files matching the glob pattern will be included
    /// in the new_files set.
    pub fn from_prefix(
        prefix: &Path,
        always_include: &GlobVec,
        files: &GlobVec,
        post_install_files: Option<&HashSet<PathBuf>>,
    ) -> Result<Self, io::Error> {
        if !prefix.exists() {
            return Ok(Files {
                new_files: HashSet::new(),
                old_files: HashSet::new(),
                prefix: prefix.to_owned(),
            });
        }

        let fs_is_case_sensitive = check_is_case_sensitive()?;

        let mut previous_files = collect_previous_files(prefix)?;

        // If we have a snapshot of files taken after dependency installation,
        // treat those as "already existing" so that post-link script artifacts
        // are not attributed to the package being built.
        if let Some(extra) = post_install_files {
            previous_files.extend(extra.iter().cloned());
        }

        let current_files = record_files(prefix)?;

        // Use case-aware difference calculation
        let mut difference = find_new_files(
            &current_files,
            &previous_files,
            prefix,
            fs_is_case_sensitive,
        );

        // Filter by files glob if specified
        if !files.is_empty() {
            difference.retain(|f| {
                files.is_match(f.strip_prefix(prefix).expect("File should be in prefix"))
            });
        }

        // Handle always_include files
        if !always_include.is_empty() {
            for file in current_files {
                let file_without_prefix =
                    file.strip_prefix(prefix).expect("File should be in prefix");
                if always_include.is_match(file_without_prefix) {
                    tracing::info!("Forcing inclusion of file: {:?}", file_without_prefix);
                    difference.insert(file);
                }
            }
        }

        Ok(Files {
            new_files: difference,
            old_files: previous_files,
            prefix: prefix.to_owned(),
        })
    }

    /// Copy the new files to a temporary directory and return the temporary directory and the files that were copied.
    pub fn to_temp_folder(&self, output: &Output) -> Result<TempFiles, PackagingError> {
        let temp_dir = TempDir::with_prefix(output.name().as_normalized())?;
        let mut files = HashSet::new();
        let mut content_type_map = HashMap::new();
        for f in &self.new_files {
            // temporary measure to remove pyc files that are not supposed to be there
            if file_mapper::filter_pyc(f, &self.old_files) {
                continue;
            }

            if let Some(dest_file) = output.write_to_dest(f, &self.prefix, temp_dir.path())? {
                content_type_map.insert(dest_file.clone(), content_type(f)?);
                files.insert(dest_file);
            }
        }

        Ok(TempFiles {
            files,
            temp_dir,
            encoded_prefix: self.prefix.clone(),
            content_type_map,
        })
    }
}

impl TempFiles {
    /// Add files to the TempFiles struct
    pub fn add_files<I>(&mut self, files: I)
    where
        I: IntoIterator<Item = PathBuf>,
    {
        for f in files {
            self.content_type_map
                .insert(f.clone(), content_type(&f).unwrap_or(None));
            self.files.insert(f);
        }
    }

    /// Return the content type map
    pub const fn content_type_map(&self) -> &HashMap<PathBuf, Option<ContentType>> {
        &self.content_type_map
    }
}

#[cfg(test)]
mod test {
    use std::{collections::HashSet, path::PathBuf};

    use crate::packaging::file_finder::{
        Files, check_is_case_sensitive, find_new_files, read_package_files_list,
    };

    #[test]
    fn test_find_new_files_case_sensitive() {
        let current_files: HashSet<PathBuf> = [
            PathBuf::from("/test/File.txt"),
            PathBuf::from("/test/file.txt"),
            PathBuf::from("/test/common.txt"),
        ]
        .into_iter()
        .collect();

        let previous_files: HashSet<PathBuf> = [
            PathBuf::from("/test/File.txt"),
            PathBuf::from("/test/common.txt"),
        ]
        .into_iter()
        .collect();

        let prefix = PathBuf::from("/test");
        let new_files = find_new_files(&current_files, &previous_files, &prefix, true);

        // On case-sensitive filesystem, file.txt should be considered new
        assert_eq!(new_files.len(), 1);
        assert!(new_files.contains(&PathBuf::from("/test/file.txt")));
    }

    #[test]
    fn test_find_new_files_case_insensitive() {
        let current_files: HashSet<PathBuf> = [
            PathBuf::from("/test/File.txt"),
            PathBuf::from("/test/file.txt"),
            PathBuf::from("/test/common.txt"),
            PathBuf::from("/test/NEW.txt"),
        ]
        .into_iter()
        .collect();

        let previous_files: HashSet<PathBuf> = [
            PathBuf::from("/test/FILE.TXT"), // Different case of File.txt
            PathBuf::from("/test/common.txt"),
        ]
        .into_iter()
        .collect();

        let prefix = PathBuf::from("/test");
        let new_files = find_new_files(&current_files, &previous_files, &prefix, false);

        // On case-insensitive filesystem, only NEW.txt should be considered new
        // Both File.txt and file.txt should be considered as existing (matching FILE.TXT)
        assert_eq!(new_files.len(), 1);
        assert!(new_files.contains(&PathBuf::from("/test/NEW.txt")));
        assert!(!new_files.contains(&PathBuf::from("/test/File.txt")));
        assert!(!new_files.contains(&PathBuf::from("/test/file.txt")));
    }

    #[test]
    fn test_check_is_case_sensitive() {
        // This test will behave differently on different filesystems
        let result = check_is_case_sensitive();
        assert!(result.is_ok());

        // We can't assert the specific value since it depends on the filesystem,
        // but we can verify the function doesn't panic and returns a boolean
        let _is_case_sensitive = result.unwrap();
    }

    #[test]
    fn test_read_package_files_list_missing() {
        let tempdir = tempfile::TempDir::new().unwrap();
        let path = tempdir.path().join("does_not_exist.txt");
        assert!(read_package_files_list(&path).unwrap().is_none());
    }

    #[test]
    fn test_read_package_files_list_empty() {
        let tempdir = tempfile::TempDir::new().unwrap();
        let path = tempdir.path().join("package_files.txt");
        std::fs::write(&path, "\n   \n\n").unwrap();
        assert!(read_package_files_list(&path).unwrap().is_none());
    }

    #[test]
    fn test_read_package_files_list_paths() {
        let tempdir = tempfile::TempDir::new().unwrap();
        let path = tempdir.path().join("package_files.txt");
        std::fs::write(&path, "  bin/foo\n\nbin/bar\n/abs/path  \n").unwrap();
        let parsed = read_package_files_list(&path).unwrap().unwrap();
        assert_eq!(
            parsed,
            vec![
                PathBuf::from("bin/foo"),
                PathBuf::from("bin/bar"),
                PathBuf::from("/abs/path"),
            ]
        );
    }

    #[test]
    fn test_files_from_paths_resolves_and_filters() {
        let tempdir = tempfile::TempDir::new().unwrap();
        let prefix = tempdir.path();

        // Create some files inside the prefix
        std::fs::create_dir_all(prefix.join("bin")).unwrap();
        std::fs::write(prefix.join("bin/foo"), b"foo").unwrap();
        std::fs::write(prefix.join("bin/bar"), b"bar").unwrap();

        // And one outside the prefix
        let outside = tempfile::TempDir::new().unwrap();
        let outside_file = outside.path().join("escape.txt");
        std::fs::write(&outside_file, b"nope").unwrap();

        let inputs = vec![
            // relative path -> should resolve against prefix
            PathBuf::from("bin/foo"),
            // absolute path inside prefix
            prefix.join("bin/bar"),
            // path outside the prefix should be skipped
            outside_file.clone(),
            // missing path should be skipped
            prefix.join("bin/missing"),
        ];

        let files = Files::from_paths(prefix, inputs).unwrap();
        let new_files: HashSet<PathBuf> = files.new_files.iter().cloned().collect();

        assert_eq!(new_files.len(), 2);
        assert!(new_files.contains(&prefix.join("bin/foo")));
        assert!(new_files.contains(&prefix.join("bin/bar")));
        assert!(!new_files.contains(&outside_file));
        assert!(!new_files.contains(&prefix.join("bin/missing")));
        assert_eq!(files.prefix, prefix);
    }
}
