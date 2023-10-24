use std::ops::Deref;
use std::path::{Path, PathBuf};

// use crate::metadata::GitUrl;
use git2::{Cred, FetchOptions, ObjectType, RemoteCallbacks, Repository, ResetType};

use crate::recipe::parser::{GitSource, GitUrl};

// use super::super::metadata::GitSrc;
use super::SourceError;

use fs_extra::dir::remove;

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
pub(crate) fn git_src<'a>(
    source: &'a GitSource,
    cache_dir: &'a Path,
    recipe_dir: &'a Path,
) -> Result<PathBuf, SourceError> {
    // Create cache path based on given cache dir and name of the source package.
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
    let repo = match &source.url() {
        GitUrl::Url(_) => {
            // If the cache_path exists, initialize the repo and fetch the specified revision.
            if cache_path.exists() {
                let repo = Repository::init(&cache_path).unwrap();
                fetch_repo(&repo, &[source.rev().to_string()])?;
                repo
            } else {
                // TODO: Make configure the clone more so git_depth is also used.
                if source.depth().is_some() {
                    tracing::warn!("No git depth implemented yet, will continue with full clone");
                }

                // Clone the repository recursively to include all submodules.
                match Repository::clone_recurse(&source.url().to_string(), &cache_path) {
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

            let repo = Repository::clone_recurse(
                recipe_dir
                    .join(path)
                    .canonicalize()?
                    .to_string_lossy()
                    .deref(),
                &cache_path,
            )?;

            if source.rev() == "HEAD" {
                // If the source is a path and the revision is HEAD, return the path to avoid git actions.
                return Ok(PathBuf::from(&cache_path));
            }
            repo
        }
    };

    // Resolve the reference and set the head to the specified revision.
    // let ref_git = format!("refs/remotes/origin/{}", source.git_rev.to_string());
    // let reference = match repo.find_reference(&ref_git) {
    let reference = match repo.resolve_reference_from_short_name(source.rev()) {
        Ok(reference) => reference,
        Err(_) => {
            match repo.resolve_reference_from_short_name(&format!("origin/{}", source.rev())) {
                Ok(reference) => reference,
                Err(e) => {
                    return Err(SourceError::GitError(e));
                }
            }
        }
    };
    let object = reference.peel(ObjectType::Commit).unwrap();
    repo.set_head(reference.name().unwrap())?;
    repo.reset(&object, ResetType::Hard, None)?;
    tracing::info!("Checked out reference: '{}'", &source.rev());

    // TODO: Implement support for pulling Git LFS files, as git2 does not support it.
    Ok(cache_path)
}

#[cfg(test)]
mod tests {
    use std::env;

    use git2::Repository;

    use crate::{
        recipe::parser::{GitSource, GitUrl},
        source::git_source::git_src,
    };

    #[test]
    fn test_git_source() {
        let cache_dir = "/tmp/rattler-build-test-git-source";
        let cases = vec![
            (
                GitSource::create(
                    GitUrl::Url(
                        "https://github.com/prefix-dev/rattler-build"
                            .parse()
                            .unwrap(),
                    ),
                    "v0.1.3".to_owned(),
                    None,
                    vec![],
                    None,
                ),
                "rattler-build",
            ),
            (
                GitSource::create(
                    GitUrl::Url(
                        "https://github.com/prefix-dev/rattler-build"
                            .parse()
                            .unwrap(),
                    ),
                    "v0.1.2".to_owned(),
                    None,
                    vec![],
                    None,
                ),
                "rattler-build",
            ),
            // (
            //     GitSrc {
            //         git_rev: GitRev::from_str("main").unwrap(),
            //         git_depth: None,
            //         patches: None,
            //         git_url: GitUrl::Url(
            //             "https://github.com/prefix-dev/rattler-build"
            //                 .parse()
            //                 .unwrap(),
            //         ),
            //         folder: None,
            //     },
            //     "rattler-build",
            // ),
            (
                GitSource::create(
                    GitUrl::Path("../rattler-build".parse().unwrap()),
                    "".to_owned(),
                    None,
                    vec![],
                    None,
                ),
                "rattler-build",
            ),
        ];
        for (source, repo_name) in cases {
            let path = git_src(
                &source,
                cache_dir.as_ref(),
                env::current_dir().unwrap().as_ref(),
            )
            .unwrap();
            Repository::init(&path).expect("Could not create repo with the path speciefied.");
            assert_eq!(
                path.to_string_lossy(),
                (cache_dir.to_owned() + "/" + repo_name)
            );
        }
    }
}
