use std::{
    fs,
    path::{Path, PathBuf, StripPrefixError},
    process::Command,
    sync::Arc,
};

use fs_extra::dir::{create_all, CopyOptions};
use ignore::WalkBuilder;

use rattler_recipe::stage2::Source;

pub mod git_source;
pub mod patch;
pub mod url_source;

#[derive(Debug, thiserror::Error)]
pub enum SourceError {
    #[error("IO Error: {0}")]
    Io(#[from] std::io::Error),

    #[error("Failed to download source from url: {0}")]
    Url(#[from] reqwest::Error),

    #[error("WalkDir Error: {0}")]
    WalkDir(#[from] walkdir::Error),

    #[error("FileSystem error: '{0}'")]
    FileSystemError(fs_extra::error::Error),

    #[error("StripPrefixError Error: {0}")]
    StripPrefixError(#[from] StripPrefixError),

    #[error("Download could not be validated with checksum!")]
    ValidationFailed,

    #[error("File not found!")]
    FileNotFound,

    #[error("Failed to apply patch: {0}")]
    PatchFailed(String),

    #[error("Failed to run git command: {0}")]
    GitError(#[from] git2::Error),

    #[error("Could not walk dir")]
    IgnoreError(#[from] ignore::Error),

    #[error("Failed to parse glob pattern")]
    Glob(#[from] globset::Error),
}

/// Fetches all sources in a list of sources and applies specified patches
pub async fn fetch_sources(
    sources: &[Source],
    work_dir: &Path,
    recipe_dir: &Path,
    cache_dir: &Path,
) -> Result<(), SourceError> {
    let cache_src = cache_dir.join("src_cache");
    fs::create_dir_all(&cache_src)?;

    for src in sources {
        match &src {
            Source::Git(src) => {
                tracing::info!("Fetching source from GIT: {}", src.git_url);
                let result = match git_source::git_src(src, &cache_src, recipe_dir) {
                    Ok(path) => path,
                    Err(e) => return Err(e),
                };
                let dest_dir = if let Some(folder) = &src.folder {
                    work_dir.join(folder)
                } else {
                    work_dir.to_path_buf()
                };
                copy_dir(&result, &dest_dir, &[], &[], false)?;

                if let Some(patches) = &src.patches {
                    patch::apply_patches(patches, work_dir, recipe_dir)?;
                }
            }
            Source::Url(src) => {
                tracing::info!("Fetching source from URL: {}", src.url);
                let res = url_source::url_src(src, &cache_src, &src.checksum).await?;
                let dest_dir = if let Some(folder) = &src.folder {
                    work_dir.join(folder)
                } else {
                    work_dir.to_path_buf()
                };
                extract(&res, &dest_dir)?;
                tracing::info!("Extracted to {:?}", work_dir);

                if let Some(patches) = &src.patches {
                    patch::apply_patches(patches, work_dir, recipe_dir)?;
                }
            }
            Source::Path(src) => {
                let src_path = recipe_dir.join(&src.path).canonicalize()?;
                tracing::info!("Copying source from path: {:?}", src_path);

                let dest_dir = if let Some(folder) = &src.folder {
                    work_dir.join(folder)
                } else {
                    work_dir.to_path_buf()
                };
                copy_dir(&src_path, &dest_dir, &[], &[], true)?;

                if let Some(patches) = &src.patches {
                    patch::apply_patches(patches, work_dir, recipe_dir)?;
                }
            }
        }
    }
    Ok(())
}

/// Extracts a tar archive to the specified target directory
fn extract(
    archive: &Path,
    target_directory: &Path,
) -> Result<std::process::Output, std::io::Error> {
    let output = Command::new("tar")
        .arg("-xf")
        .arg(String::from(archive.to_string_lossy()))
        .arg("--preserve-permissions")
        .arg("--strip-components=1")
        .arg("-C")
        .arg(String::from(target_directory.to_string_lossy()))
        .output();

    output
}

/// The copy_dir function accepts additionally a list of globs to ignore or include in the copy process.
/// It uses the `ignore` crate to read the `.gitignore` file in the source directory and uses the globs
/// to filter the files and directories to copy.
///
/// The copy process also ignores hidden files and directories by default.
///
/// # Return
///
/// The returned `Vec<PathBuf>` contains the pathes of the copied files.
/// If a directory is created in this function, the path to the directory is _not_ returned.
pub(crate) fn copy_dir(
    from: &Path,
    to: &Path,
    include_globs: &[&str],
    exclude_globs: &[&str],
    use_gitignore: bool,
) -> Result<Vec<PathBuf>, SourceError> {
    // Create the to path because we're going to copy the contents only
    create_all(to, true).unwrap();

    // Setup copy options, overwrite if needed, only copy the contents as we want to specify the dir name manually
    let mut options = CopyOptions::new();
    options.overwrite = true;
    options.content_only = true;

    // We need an Arc for the glob lists bcause WalkBuilder::filter_entry does not
    // catch its environment, so we need to move the globs in there.
    // Because it also needs `Send` (because it uses some Arc machinery internally)
    // we cannot use a normal Rc here, so we use an Arc
    fn mkglobset(globs: &[&str]) -> Result<Arc<globset::GlobSet>, globset::Error> {
        let mut globset = globset::GlobSetBuilder::new();
        for glob in globs {
            globset.add(globset::Glob::new(glob)?);
        }
        globset.build().map(Arc::new)
    }
    let (folders, globs) = include_globs
        .iter()
        .partition::<Vec<_>, _>(|glob| glob.ends_with('/') && !glob.contains('*'));

    let folders = Arc::new(folders.iter().map(PathBuf::from).collect::<Vec<_>>());

    let include_globs = mkglobset(&globs)?;
    let include_globs_copy = include_globs.clone();

    let mut any_include_glob_matched = false;
    let exclude_globs = mkglobset(exclude_globs)?;

    let result = WalkBuilder::new(from)
        // disregard global gitignore
        .git_global(false)
        .git_ignore(use_gitignore)
        .hidden(true)
        .filter_entry(move |entry| {
            // if the entry is a directory, we always want to include it to make sure that we
            // recurse into all the subdirs
            if let Some(ft) = entry.file_type().as_ref() {
                if ft.is_dir() {
                    return true;
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

            let include = include_globs.len() == 0;
            let include = include || include_globs.is_match(&stripped_path);
            let include = include || folders.iter().any(|f| stripped_path.starts_with(f));
            let exclude = exclude_globs.is_match(entry.path());

            include && !exclude
        })
        .build()
        .map(|entry| {
            let entry = entry?;
            let path = entry.path();

            any_include_glob_matched =
                any_include_glob_matched || include_globs_copy.is_match(path);

            let stripped_path = path.strip_prefix(from)?;
            let dest_path = to.join(stripped_path);

            if path.is_dir() {
                create_all(&dest_path, true)
                    .map(|_| None) // We do not return pathes to directories that are created
                    .map_err(SourceError::FileSystemError)
            } else {
                let file_options = fs_extra::file::CopyOptions {
                    overwrite: options.overwrite,
                    skip_exist: options.skip_exist,
                    buffer_size: options.buffer_size,
                };
                fs_extra::file::copy(path, &dest_path, &file_options)
                    .map_err(SourceError::FileSystemError)?;

                tracing::debug!(
                    "Copied {} to {}",
                    path.to_string_lossy(),
                    dest_path.to_string_lossy()
                );
                Ok(Some(dest_path))
            }
        })
        .filter_map(|res| res.transpose())
        .collect();

    if !any_include_glob_matched {
        tracing::warn!("No glob matched");
    }

    result
}

#[cfg(test)]
mod test {
    #[test]
    fn test_copy_dir() {
        let tmp_dir = tempfile::TempDir::new().unwrap();
        let tmp_dir_path = tmp_dir.into_path();
        let dir = tmp_dir_path.as_path().join("test_copy_dir");

        fs_extra::dir::create_all(&dir, true).unwrap();
        std::fs::write(dir.join("test.txt"), "test").unwrap();
        std::fs::create_dir(dir.join("test_dir")).unwrap();
        std::fs::write(dir.join("test_dir").join("test.md"), "test").unwrap();
        std::fs::create_dir(dir.join("test_dir").join("test_dir2")).unwrap();

        let dest_dir = tmp_dir_path.as_path().join("test_copy_dir_dest");
        super::copy_dir(&dir, &dest_dir, &[], &[], false).unwrap();

        for entry in walkdir::WalkDir::new(dest_dir) {
            tracing::info!("{}", entry.unwrap().path().display());
        }

        let dest_dir_2 = tmp_dir_path.as_path().join("test_copy_dir_dest_2");
        // ignore all txt files
        super::copy_dir(&dir, &dest_dir_2, &["*.txt"], &[], false).unwrap();
        tracing::info!("---------------------");
        for entry in walkdir::WalkDir::new(dest_dir_2) {
            tracing::info!("{}", entry.unwrap().path().display());
        }

        let dest_dir_2 = tmp_dir_path.as_path().join("test_copy_dir_dest_2");
        // ignore all txt files
        super::copy_dir(&dir, &dest_dir_2, &[], &["*.txt"], false).unwrap();
        tracing::info!("---------------------");
        for entry in walkdir::WalkDir::new(dest_dir_2) {
            tracing::info!("{}", entry.unwrap().path().display());
        }
    }
}
