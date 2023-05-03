use std::ops::Deref;
use std::path::StripPrefixError;
use std::{
    fs,
    io::Cursor,
    path::{Path, PathBuf},
    process::Command,
};

use crate::metadata::GitUrl;
use git2::{Cred, FetchOptions, ObjectType, RemoteCallbacks, Repository, ResetType};
use rattler_digest::compute_file_digest;

use super::metadata::{Checksum, GitSrc, Source, UrlSrc};

use fs_extra::dir::{copy, create, remove, CopyOptions};
use fs_extra::error::ErrorKind::PermissionDenied;

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

    #[error("Failed to apply patch: {0}")]
    PatchFailed(String),

    #[error("Failed to run git command: {0}")]
    GitError(#[from] git2::Error),
}

fn validate_checksum(path: &Path, checksum: &Checksum) -> bool {
    match checksum {
        Checksum::Sha256(value) => {
            let digest =
                compute_file_digest::<sha2::Sha256>(path).expect("Could not compute SHA256");
            let computed_sha = hex::encode(digest);
            if !computed_sha.eq(value) {
                tracing::error!(
                    "SHA256 values of downloaded file not matching!\nDownloaded = {}, should be {}",
                    computed_sha,
                    value
                );
                false
            } else {
                tracing::info!("Validated SHA256 values of the downloaded file!");
                true
            }
        }
        Checksum::Md5(_value) => {
            todo!("MD5 not implemented yet!");
        }
    }
}

fn split_filename(filename: &str) -> (String, String) {
    let stem = Path::new(filename)
        .file_stem()
        .and_then(|os_str| os_str.to_str())
        .unwrap_or("")
        .to_string();

    let stem_without_tar = stem.trim_end_matches(".tar");

    let extension = Path::new(filename)
        .extension()
        .and_then(|os_str| os_str.to_str())
        .unwrap_or("")
        .to_string();

    let full_extension = if stem != stem_without_tar {
        format!(".tar.{}", extension)
    } else if !extension.is_empty() {
        format!(".{}", extension)
    } else {
        "".to_string()
    };

    (stem_without_tar.to_string(), full_extension)
}

fn cache_name_from_url(url: &url::Url, checksum: &Checksum) -> String {
    let filename = url.path_segments().unwrap().last().unwrap();
    let (stem, extension) = split_filename(filename);
    let checksum = match checksum {
        Checksum::Sha256(value) => value,
        Checksum::Md5(value) => value,
    };
    format!("{}_{}{}", stem, &checksum[0..8], extension)
}

async fn url_src(
    source: &UrlSrc,
    cache_dir: &Path,
    checksum: &Checksum,
) -> Result<PathBuf, SourceError> {
    let cache_name = PathBuf::from(cache_name_from_url(&source.url, checksum));
    let cache_name = cache_dir.join(cache_name);

    let metadata = fs::metadata(&cache_name);
    if metadata.is_ok() && metadata?.is_file() && validate_checksum(&cache_name, checksum) {
        tracing::info!("Found valid source cache file.");
        return Ok(cache_name.clone());
    }

    let response = reqwest::get(source.url.clone()).await?;

    let mut file = std::fs::File::create(&cache_name)?;

    let mut content = Cursor::new(response.bytes().await?);
    std::io::copy(&mut content, &mut file)?;

    if !validate_checksum(&cache_name, checksum) {
        tracing::error!("Checksum validation failed!");
        fs::remove_file(&cache_name)?;
        return Err(SourceError::ValidationFailed);
    }

    Ok(cache_name)
}

/// Fetch the repo and its specific refspecs.
fn fetch_repo(repo: &Repository, refspecs: &[String]) -> Result<(), git2::Error> {
    let mut remote = repo.find_remote("origin")?;

    let mut callbacks = RemoteCallbacks::new();
    callbacks.credentials(|_url, username_from_url, _allowed_types| {
        Cred::ssh_key_from_agent(username_from_url.unwrap_or("git"))
    });

    let mut fetch_options = FetchOptions::new();
    fetch_options.remote_callbacks(callbacks);

    remote.fetch(refspecs, Some(&mut fetch_options), None)?;

    tracing::debug!("Repository fetched successfully!");

    Ok(())
}

/// Find or create the cache for a git repository.
///
/// This function has multiple paths depending on the source of the Git repository:
/// 1. The source is a git URL:
/// a. If the cache for the package exists, fetch and checkout the specified revision.
/// b. If there is no cache, perform a recursive clone.
/// 2. The source is a local path:
/// a. If the specified revision is HEAD, do a local clone and return the cache path, because no action on the repo is needed.
/// b. If any other revision is specified, clone the repo to the cache path and perform a checkout of the specified revision.
///
/// # Arguments
/// - source: The GitSrc struct containing information about the source git repository.
/// - cache_dir: The base cache directory where the repository will be stored.
///
/// # Returns
/// - A Result containing the PathBuf to the cache, or a SourceError if an error occurs during the process.
fn git_src<'a>(source: &'a GitSrc, cache_dir: &'a Path) -> Result<PathBuf, SourceError> {
    // Create cache path based on given cache dir and name of the source package.
    let filename = match &source.git_url {
        GitUrl::Url(url) => url.path_segments().unwrap().last().unwrap().to_string(),
        GitUrl::Path(path) => path.file_name().unwrap().to_string_lossy().to_string(),
    };
    let cache_name = PathBuf::from(filename);
    let cache_path = cache_dir.join(cache_name);

    // Initialize or clone the repository depending on the source's git_url.
    let repo = match &source.git_url {
        GitUrl::Url(_) => {
            // If the cache_path exists, initialize the repo and fetch the specified revision.
            if cache_path.exists() {
                let repo = Repository::init(&cache_path).unwrap();
                fetch_repo(&repo, &[source.git_rev.to_string()])?;
                repo
            } else {
                // TODO: Make configure the clone more so git_depth is also used.
                if source.git_depth.is_some() {
                    tracing::warn!("No git depth implemented yet, will continue with full clone");
                }

                // Clone the repository recursively to include all submodules.
                match Repository::clone_recurse(&source.git_url.to_string(), &cache_path) {
                    Ok(repo) => repo,
                    Err(e) => return Err(SourceError::GitError(e)),
                }
            }
        }
        GitUrl::Path(path) => {
            if cache_path.exists() {
                // Remove old cache so it can be overwritten.
                if let Err(remove_error) = remove(&cache_path) {
                    tracing::error!("Failed to remove old cache directory: {}", remove_error);
                    return Err(SourceError::FileSystemError(remove_error));
                }
            }

            let repo =
                Repository::clone_recurse(path.to_string_lossy().deref(), &cache_path).unwrap();

            if source.git_rev.to_string() == "HEAD" {
                // If the source is a path and the revision is HEAD, return the path to avoid git actions.
                return Ok(PathBuf::from(&cache_path));
            }
            repo
        }
    };

    // Resolve the reference and set the head to the specified revision.
    let reference = match repo.resolve_reference_from_short_name(&source.git_rev.to_string()) {
        Ok(reference) => reference,
        Err(e) => {
            return Err(SourceError::GitError(e));
        }
    };
    let object = reference.peel(ObjectType::Commit).unwrap();
    repo.set_head(reference.name().unwrap())?;
    repo.reset(&object, ResetType::Hard, None)?;
    tracing::info!("Checked out reference: '{}'", &source.git_rev);

    // TODO: Implement support for pulling Git LFS files, as git2 does not support it.
    Ok(cache_path)
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

/// Applies all patches in a list of patches to the specified work directory
/// Currently only supports patching with the `patch` command.
fn apply_patches(
    patches: &[PathBuf],
    work_dir: &Path,
    recipe_dir: &Path,
) -> Result<(), SourceError> {
    for patch in patches {
        let patch = recipe_dir.join(patch);
        let output = Command::new("patch")
            .arg("-p1")
            .arg("-i")
            .arg(String::from(patch.to_string_lossy()))
            .arg("-d")
            .arg(String::from(work_dir.to_string_lossy()))
            .output()?;

        if !output.status.success() {
            tracing::error!("Failed to apply patch: {}", patch.to_string_lossy());
            tracing::error!("Stdout: {}", String::from_utf8_lossy(&output.stdout));
            tracing::error!("Stderr: {}", String::from_utf8_lossy(&output.stderr));
            return Err(SourceError::PatchFailed(
                patch.to_string_lossy().to_string(),
            ));
        }
    }
    Ok(())
}

fn copy_dir(from: &PathBuf, to: &PathBuf) -> Result<(), SourceError> {
    // Create the to path because we're going to copy the contents only
    create(to, true).unwrap();

    // Setup copy options, overwrite if needed, only copy the contents as we want to specify the dir name manually
    let mut options = CopyOptions::new();
    options.overwrite = true;
    options.content_only = true;

    match copy(from, to, &options) {
        Ok(_) => tracing::info!(
            "Copied {} to {}",
            from.to_string_lossy(),
            to.to_string_lossy()
        ),
        // Use matches as the ErrorKind does not support `==`
        Err(e) if matches!(e.kind, PermissionDenied) => {
            tracing::debug!("Permission error in cache, this often happens when the previous run was exited in a faulty way. Removing the cache and retrying the copy.");
            if let Err(remove_error) = remove(to) {
                tracing::error!("Failed to remove cache directory: {}", remove_error);
                return Err(SourceError::FileSystemError(e));
            } else if let Err(retry_error) = copy(from, to, &options) {
                tracing::error!("Failed to retry the copy operation: {}", retry_error);
                return Err(SourceError::FileSystemError(e));
            } else {
                tracing::debug!(
                    "Successfully retried copying {} to {}",
                    from.to_string_lossy(),
                    to.to_string_lossy()
                );
            }
        }
        Err(e) => return Err(SourceError::FileSystemError(e)),
    }
    Ok(())
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
                let result = match git_src(src, &cache_src) {
                    Ok(path) => path,
                    Err(e) => return Err(e),
                };
                let dest_dir = if let Some(folder) = &src.folder {
                    work_dir.join(folder)
                } else {
                    work_dir.to_path_buf()
                };
                copy_dir(&result, &dest_dir)?;
                if let Some(patches) = &src.patches {
                    apply_patches(patches, work_dir, recipe_dir)?;
                }
            }
            Source::Url(src) => {
                tracing::info!("Fetching source from URL: {}", src.url);
                let res = url_src(src, &cache_src, &src.checksum).await?;
                let dest_dir = if let Some(folder) = &src.folder {
                    work_dir.join(folder)
                } else {
                    work_dir.to_path_buf()
                };
                extract(&res, &dest_dir)?;
                tracing::info!("Extracted to {:?}", work_dir);
                if let Some(patches) = &src.patches {
                    apply_patches(patches, work_dir, recipe_dir)?;
                }
            }
            Source::Path(src) => {
                tracing::info!("Copying source from path: {:?}", src.path);
                let src_path = recipe_dir.join(&src.path);

                let dest_dir = if let Some(folder) = &src.folder {
                    work_dir.join(folder)
                } else {
                    work_dir.to_path_buf()
                };
                copy_dir(&src_path, &dest_dir)?;
            }
        }
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::metadata::GitRev;
    use git2::Repository;
    use std::str::FromStr;
    use url::Url;

    #[test]
    fn test_split_filename() {
        let test_cases = vec![
            ("example.tar.gz", ("example", ".tar.gz")),
            ("example.tar.bz2", ("example", ".tar.bz2")),
            ("example.zip", ("example", ".zip")),
            ("example.tar", ("example", ".tar")),
            ("example", ("example", "")),
            (".hidden.tar.gz", (".hidden", ".tar.gz")),
        ];

        for (filename, expected) in test_cases {
            let (name, ending) = split_filename(filename);
            assert_eq!(
                (name.as_str(), ending.as_str()),
                expected,
                "Failed for filename: {}",
                filename
            );
        }
    }

    #[test]
    fn test_cache_name() {
        let cases = vec![
            (
                "https://example.com/example.tar.gz",
                Checksum::Sha256(
                    "1234567890abcdef1234567890abcdef1234567890abcdef1234567890abcdef".to_string(),
                ),
                "example_12345678.tar.gz",
            ),
            (
                "https://github.com/mamba-org/mamba/archive/refs/tags/micromamba-12.23.12.tar.gz",
                Checksum::Sha256(
                    "63fd8a1dbec811e63d4f9b5e27757af45d08a219d0900c7c7a19e0b177a576b8".to_string(),
                ),
                "micromamba-12.23.12_63fd8a1d.tar.gz",
            ),
        ];

        for (url, checksum, expected) in cases {
            let url = Url::parse(url).unwrap();
            let name = cache_name_from_url(&url, &checksum);
            assert_eq!(name, expected);
        }
    }

    #[test]
    fn test_git_source() {
        let cache_dir = "/tmp/rattler-build-test-git-source";
        let cases = vec![
            (
                GitSrc {
                    git_rev: GitRev::from_str("v0.1.3").unwrap(),
                    git_depth: None,
                    patches: None,
                    git_url: GitUrl::Url(
                        "https://github.com/prefix-dev/rattler-build"
                            .parse()
                            .unwrap(),
                    ),
                    folder: None,
                },
                "rattler-build",
            ),
            (
                GitSrc {
                    git_rev: GitRev::from_str("v0.1.2").unwrap(),
                    git_depth: None,
                    patches: None,
                    git_url: GitUrl::Url(
                        "https://github.com/prefix-dev/rattler-build"
                            .parse()
                            .unwrap(),
                    ),
                    folder: None,
                },
                "rattler-build",
            ),
            (
                GitSrc {
                    git_rev: GitRev::from_str("main").unwrap(),
                    git_depth: None,
                    patches: None,
                    git_url: GitUrl::Url(
                        "https://github.com/prefix-dev/rattler-build"
                            .parse()
                            .unwrap(),
                    ),
                    folder: None,
                },
                "rattler-build",
            ),
        ];
        for (source, repo_name) in cases {
            let path = git_src(&source, cache_dir.as_ref()).unwrap();
            Repository::init(&path).expect("Could not create repo with the path speciefied.");
            assert_eq!(
                path.to_string_lossy(),
                (cache_dir.to_owned() + "/" + repo_name)
            );
        }
    }
}
