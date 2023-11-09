use std::{
    borrow::Cow,
    path::{Path, PathBuf},
    process::Command,
};

use fs_extra::dir::remove;
use itertools::Itertools;

use crate::recipe::parser::{GitSource, GitUrl};

use super::SourceError;

// git clone file://C:/Users/user/../repo
type RepoPath<'a> = &'a Path;

pub fn fetch_repo(repo_path: RepoPath<'a>, refspecs: &[String]) -> Result<(), SourceError> {
    // might break on some platforms due to auth and ssh
    // especially ssh with password
    let refspecs_str = refspecs.into_iter().join(" ");
    let output = Command::new("git")
        .args(["fetch", "origin", refspecs_str.as_str()])
        .output()
        .map_err(|err| SourceError::ValidationFailed)?;

    tracing::debug!("Repository fetched successfully!");
    Ok(())
}

pub fn git_src(
    source: &GitSource,
    cache_dir: &Path,
    recipe_dir: &Path,
) -> Result<PathBuf, SourceError> {
    // on windows there exist some path conversion issue, conda seems to have a solution for it, check
    // it out
    // figure out more: https://github.com/conda/conda-build/blob/c71c4abee1c85f5a36733c461f224941ab3ebbd1/conda_build/source.py#L38C1-L39C59
    // ---
    // tool used: https://cygwin.com/cygwin-ug-net/cygpath.html
    // to windows path: cygpath -w unix_path
    // to unix path: cyppath -u win_path
    // ---
    // note: rust on windows handles some of these

    let filename = match &source.url() {
        GitUrl::Url(url) => url.path_segments().unwrap().last().unwrap().to_string(),
        GitUrl::Path(path) => recipe_dir
            .join(path)
            .canonicalize()?
            .file_name()
            .unwrap()
            .to_string_lossy()
            .to_string(),
    };

    let cache_name = PathBuf::from(filename);
    let cache_path = cache_dir.join(cache_name);

    // Initialize or clone the repository depending on the source's git_url.
    let fetched = match &source.url() {
        GitUrl::Url(_) => {
            // If the cache_path exists, initialize the repo and fetch the specified revision.
            if cache_path.exists() {
                let path = fetch_repo(&cache_path, &[source.rev().to_string()])?;
                true
            } else {
                // TODO: Make configure the clone more so git_depth is also used.
                if source.depth().is_some() {
                    tracing::warn!("No git depth implemented yet, will continue with full clone");
                }

                let out = Command::new("git")
                    .args(["clone", "--recursive", source.url().to_string().as_str()])
                    .output()
                    .map_err(|_| SourceError::ValidationFailed)?;
                if !out.status.success() {
                    return Err(SourceError::ValidationFailed);
                }
                let repo_path = String::from_utf8_lossy(&out.stdout);

                if source.rev() == "HEAD" {
                    // If the source is a path and the revision is HEAD, return the path to avoid git actions.
                    return Ok(PathBuf::from(&cache_path));
                }
                true
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

            let out = Command::new("git")
                .args(["clone", "--recursive", format!("file://{}", path.display()).as_str(), cache_path.display().to_string().as_str()])
                .output()
                .map_err(|_| SourceError::ValidationFailed)?;
            if !out.status.success() {
                return Err(SourceError::ValidationFailed);
            }
            let repo_path = String::from_utf8_lossy(&out.stdout);

            if source.rev() == "HEAD" {
                // If the source is a path and the revision is HEAD, return the path to avoid git actions.
                return Ok(PathBuf::from(&cache_path));
            }
            true
        }
    };

    if !fetched {
        return Err(SourceError::GitError("Failed to fetch git repo".to_string()));
    }

    // Resolve the reference and set the head to the specified revision.
    // let ref_git = format!("refs/remotes/origin/{}", source.git_rev.to_string());
    // let reference = match repo.find_reference(&ref_git) {
    let output = Command::new("git")
        .args(["checkout", ref_git])
        .output()
        .map_err(|_| SourceError::GitError("git checkout".to_string()))?;

    let output = Command::new("git")
        .args(["reset", "--hard"])
        .output()
        .map_err(|_| SourceError::GitError("git reset --hard".to_string()))?;

    tracing::info!("Checked out reference: '{}'", &source.rev());

    Ok(cache_path)
}
