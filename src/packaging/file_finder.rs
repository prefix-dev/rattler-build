use content_inspector::ContentType;
use fs_err as fs;
use globset::GlobSet;
use rattler_conda_types::PrefixRecord;
use std::{
    collections::{HashMap, HashSet},
    io::{self, Read},
    path::{Path, PathBuf},
};
use tempfile::TempDir;
use walkdir::WalkDir;

use crate::metadata::Output;

use super::{file_mapper, PackagingError};

/// This struct keeps a record of all the files that are new in the prefix (i.e. not present in the previous
/// conda environment).
pub struct Files {
    /// The files that are new in the prefix
    pub new_files: HashSet<PathBuf>,
    /// The prefix that we are dealing with
    pub prefix: PathBuf,
}

/// This struct keeps a record of all the files that are moved into a temporary directory
/// for further post-processing (before they are packaged into a tarball).
pub struct TempFiles {
    /// The files that are copied to the temporary directory
    pub files: HashSet<PathBuf>,
    /// The temporary directory where the files are copied to
    pub temp_dir: tempfile::TempDir,
    /// The prefix which is encoded in the files (the long placeholder for the actual prefix, e.g. /home/user/bld_placeholder...)
    pub encoded_prefix: PathBuf,
    /// The content type of the files
    pub content_type_map: HashMap<PathBuf, ContentType>,
}

pub fn content_type(path: &Path) -> Result<ContentType, io::Error> {
    // read first 1024 bytes to determine file type
    let mut file = fs::File::open(path)?;
    let mut buffer = [0; 1024];
    let n = file.read(&mut buffer)?;
    let buffer = &buffer[..n];

    Ok(content_inspector::inspect(buffer))
}

/// This function returns a HashSet of (recursively) all the files in the given directory.
pub fn record_files(directory: &Path) -> Result<HashSet<PathBuf>, io::Error> {
    let mut res = HashSet::new();
    for entry in WalkDir::new(directory) {
        res.insert(entry?.path().to_owned());
    }
    Ok(res)
}

impl Files {
    /// Find all files in the given (host) prefix and remove all previously installed files (based on the PrefixRecord
    /// of the conda environment). If always_include is Some, then all files matching the glob pattern will be included
    /// in the new_files set.
    pub fn from_prefix(prefix: &Path, always_include: Option<&GlobSet>) -> Result<Self, io::Error> {
        if !prefix.exists() {
            return Ok(Files {
                new_files: HashSet::new(),
                prefix: prefix.to_owned(),
            });
        }

        let previous_files = if prefix.join("conda-meta").exists() {
            let prefix_records = PrefixRecord::collect_from_prefix(prefix)?;
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

        let mut difference = current_files
            .difference(&previous_files)
            .cloned()
            .collect::<HashSet<_>>();

        if let Some(always_include) = always_include {
            for file in current_files {
                let file_without_prefix =
                    file.strip_prefix(prefix).expect("File should be in prefix");
                if always_include.is_match(file_without_prefix) {
                    tracing::info!("Forcing inclusion of file: {:?}", file);
                    difference.insert(file);
                }
            }
        }

        Ok(Files {
            new_files: difference,
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
            if file_mapper::filter_pyc(f, &self.new_files) {
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
            self.content_type_map.insert(
                f.clone(),
                content_type(&f).expect("Could not determine content type"),
            );
            self.files.insert(f);
        }
    }
}
