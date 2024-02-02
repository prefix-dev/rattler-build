use std::{
    collections::HashSet,
    io,
    path::{Path, PathBuf},
};

use globset::GlobSet;
use rattler_conda_types::PrefixRecord;
use walkdir::WalkDir;

pub struct Files {
    pub new_files: HashSet<PathBuf>,
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
        })
    }
}
