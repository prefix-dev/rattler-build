use std::{
    path::{Path, PathBuf},
    process::Command,
};

use fs_extra::dir::remove;
use itertools::Itertools;

use crate::recipe::parser::{GitSource, GitUrl};

use super::SourceError;

type RepoPath<'a> = &'a Path;

pub fn fetch_repo<'a>(repo_path: RepoPath<'a>, refspecs: &[String]) -> Result<(), SourceError> {
    // might break on some platforms due to auth and ssh
    // especially ssh with password
    let refspecs_str = refspecs.into_iter().join(" ");
    let cd = std::env::current_dir().ok();
    _ = std::env::set_current_dir(repo_path);
    let output = Command::new("git")
        .args(["fetch", "origin", refspecs_str.as_str()])
        .output()
        .map_err(|_err| SourceError::ValidationFailed)?;
    // TODO(swarnimarun): get rid of assert
    assert!(output.status.success());
    _ = cd.map(|cd| std::env::set_current_dir(cd));
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

    tracing::info!(
        "git src:\n\tsource: {:?}\n\tcache_dir: {}\n\trecipe_dir: {}",
        source,
        cache_dir.display(),
        recipe_dir.display()
    );

    let filename = match &source.url() {
        GitUrl::Url(url) => url.path_segments().unwrap().last().unwrap().to_string(),
        GitUrl::Path(path) => recipe_dir
            .join(path)
            .canonicalize()?
            .file_name()
            .ok_or_else(|| SourceError::GitErrorStr("Failed to parse "))?
            .to_string_lossy()
            .to_string(),
    };

    let cache_name = PathBuf::from(filename);
    let cache_path = cache_dir.join(cache_name);

    // Initialize or clone the repository depending on the source's git_url.
    match &source.url() {
        GitUrl::Url(_) => {
            // If the cache_path exists, initialize the repo and fetch the specified revision.
            if cache_path.exists() {
                fetch_repo(&cache_path, &[source.rev().to_string()])?;
            } else {
                let mut command = Command::new("git");
                command.args([
                    "clone",
                    "--recursive",
                    source.url().to_string().as_str(),
                    cache_path.to_str().unwrap(),
                ]);
                if let Some(depth) = source.depth() {
                    command.args(["--depth", depth.to_string().as_str()]);
                }
                let output = command
                    .output()
                    .map_err(|_e| SourceError::GitErrorStr("Failed to execute clone command"))?;
                if !output.status.success() {
                    return Err(SourceError::GitErrorStr("Git clone failed for source"));
                }
                if source.rev() == "HEAD" || source.rev().trim().is_empty() {
                    // If the source is a path and the revision is HEAD, return the path to avoid git actions.
                    return Ok(PathBuf::from(&cache_path));
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
            let path = std::fs::canonicalize(path).map_err(|e| {
                tracing::error!("Path not found on system: {}", e);
                SourceError::GitError(format!("{}: Path not found on system", e.to_string()))
            })?;

            let mut command = Command::new("git");
            command
                .arg("clone")
                .arg("--recursive")
                .arg(format!("file://{}/.git", path.display()).as_str())
                .arg(cache_path.as_os_str());
            if let Some(depth) = source.depth() {
                command.args(["--depth", depth.to_string().as_str()]);
            }
            let output = command
                .output()
                .map_err(|_| SourceError::ValidationFailed)?;
            if !output.status.success() {
                tracing::error!("Command failed: {:?}", command);
                return Err(SourceError::GitErrorStr(
                    "failed to execute clone from file",
                ));
            }

            if source.rev() == "HEAD" || source.rev().trim().is_empty() {
                // If the source is a path and the revision is HEAD, return the path to avoid git actions.
                return Ok(PathBuf::from(&cache_path));
            }
        }
    }

    // Resolve the reference and set the head to the specified revision.
    // let ref_git = format!("refs/remotes/origin/{}", source.git_rev.to_string());
    // let reference = match repo.find_reference(&ref_git) {
    let output = Command::new("git")
        .current_dir(&cache_path)
        .args(["rev-parse", source.rev()])
        .output()
        .map_err(|_| SourceError::GitErrorStr("git rev-parse failed"))?;
    if !output.status.success() {
        tracing::error!("Command failed: \"git\" \"rev-parse\" \"{}\"", source.rev());
        return Err(SourceError::GitErrorStr("failed to get valid hash for rev"));
    }
    let ref_git = String::from_utf8(output.stdout)
        .map_err(|_| SourceError::GitErrorStr("failed to parse git rev as utf-8"))?;
    tracing::info!("cache_path = {}", cache_path.display());

    let mut command = Command::new("git");
    command
        .current_dir(&cache_path)
        .arg("checkout")
        .arg(ref_git.as_str().trim());

    let output = command
        .output()
        .map_err(|_| SourceError::GitErrorStr("failed to execute git checkout"))?;

    if !output.status.success() {
        tracing::error!("Command failed: {:?}", command);
        return Err(SourceError::GitErrorStr("failed to checkout for ref"));
    }

    let output = Command::new("git")
        .current_dir(&cache_path)
        .args(["reset", "--hard"])
        .output()
        .map_err(|_| SourceError::GitErrorStr("failed to execute git reset"))?;

    if !output.status.success() {
        tracing::error!("Command failed: \"git\" \"reset\" \"--hard\"");
        return Err(SourceError::GitErrorStr("failed to git reset"));
    }

    tracing::info!("Checked out reference: '{}'", &source.rev());

    Ok(cache_path)
}

#[cfg(test)]
mod tests {
    use std::env;

    use crate::{
        recipe::parser::{GitSource, GitUrl},
        source::host_git_source::git_src,
    };

    #[test]
    fn test_host_git_source() {
        let cache_dir = std::env::temp_dir().join("rattler-build-test-git-source");
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
            assert_eq!(
                path.to_string_lossy(),
                cache_dir.join(repo_name).to_string_lossy()
            );
        }
    }
}
