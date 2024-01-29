use std::{collections::HashSet, io::Read, path::PathBuf};
use anyhow::Ok;
use content_inspector::ContentType;
use fs_err as fs;
use tempfile::TempDir;
use walkdir::WalkDir;

/// This function returns a HashSet of (recursively) all the files in the given directory.
pub fn record_files(directory: &PathBuf) -> Result<HashSet<PathBuf>, std::io::Error> {
    let mut res = HashSet::new();
    for entry in WalkDir::new(directory) {
        res.insert(entry?.path().to_owned());
    }
    Ok(res)
}

fn determine_content_type(file_path: &PathBuf) -> Result<ContentType, std::io::Error> {
    // read first 1024 bytes to determine file type
    let mut file = fs::File::open(file_path)?;
    let mut buffer = [0; 1024];
    let n = file.read(&mut buffer)?;
    let buffer = &buffer[..n];

    let content_type = content_inspector::inspect(buffer);

    Ok(content_type)
}


#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub struct PackageFiles {
    files: HashSet<(PathBuf, ContentType)>,
    temp_dir: TempDir,
    encoded_prefix: PathBuf,
}

impl PackageFiles {
    pub from_folder(files_before: &HashSet<PathBuf>, always_include_files: Option<&GlobSet>) -> Result<Self, GlobError> {

        let files_after = record_files(&directories.host_prefix).expect("Could not record files");

        let mut difference = files_after
            .difference(&files_before)
            .cloned()
            .collect::<HashSet<_>>();
    
        if let Some(always_include_files) = output.recipe.build().always_include_files() {
            for file in files_after {
                let file_without_prefix = file
                    .strip_prefix(&directories.host_prefix)
                    .into_diagnostic()?;
                if always_include_files.is_match(file_without_prefix) {
                    tracing::info!("Forcing inclusion of file: {:?}", file);
                    difference.insert(file.clone());
                }
            }
        }

        Ok(
            Self {
                files: difference,
                temp_dir: temp_dir,
                encoded_prefix: directories.encoded_prefix.clone(),
            }
        
        )
    }

    pub fn as_relative(&self) -> HashSet<PathBuf> {
        self.files.iter().map(|p| p.strip_prefix(&self.temp_dir).unwrap().to_owned()).collect()
    }

    pub fn files
}