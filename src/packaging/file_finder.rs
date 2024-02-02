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
    pub fn from_prefix(prefix: &Path, always_include: Option<&GlobSet>) -> Result<Self, io::Error> {
        let prefix_records = PrefixRecord::collect_from_prefix(prefix)?;
        let previous_files = prefix_records
            .into_iter()
            .fold(HashSet::new(), |mut acc, record| {
                acc.extend(record.files);
                acc
            });

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
