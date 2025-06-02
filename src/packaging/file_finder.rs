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

use crate::{metadata::Output, recipe::parser::GlobVec};

use super::{PackagingError, file_mapper, normalize_path_for_comparison};

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

        let current_files = current_files
            .clone()
            .into_iter()
            .filter(|p| {
                // Only include files that are not in the previous set
                !previous_case_aware.contains(&CaseInsensitivePath::new(
                    p.strip_prefix(prefix).expect("File should be in prefix"),
                ))
            })
            .collect::<HashSet<_>>();

        current_files
    }
}

impl Files {
    /// Find all files in the given (host) prefix and remove all previously installed files (based on the PrefixRecord
    /// of the conda environment). If always_include is Some, then all files matching the glob pattern will be included
    /// in the new_files set.
    pub fn from_prefix(
        prefix: &Path,
        always_include: &GlobVec,
        files: &GlobVec,
    ) -> Result<Self, io::Error> {
        if !prefix.exists() {
            return Ok(Files {
                new_files: HashSet::new(),
                old_files: HashSet::new(),
                prefix: prefix.to_owned(),
            });
        }

        let fs_is_case_sensitive = check_is_case_sensitive()?;

        let previous_files = if prefix.join("conda-meta").exists() {
            let prefix_records: Vec<PrefixRecord> = PrefixRecord::collect_from_prefix(prefix)?;
            let mut previous_files =
                prefix_records
                    .into_iter()
                    .fold(HashSet::new(), |mut acc, record| {
                        acc.extend(record.files.iter().map(|f| prefix.join(f)));
                        acc
                    });

            // Also include the existing conda-meta (PrefixRecord) files themselves
            previous_files.extend(record_files(&prefix.join("conda-meta"))?);
            previous_files
        } else {
            HashSet::new()
        };

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

    use crate::packaging::file_finder::{check_is_case_sensitive, find_new_files};

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
}
