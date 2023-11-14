use std::{
    path::{Path, PathBuf},
    process::Command,
};

use fs_extra::dir::remove;
use itertools::Itertools;

use crate::recipe::parser::{GitSource, GitUrl};

use super::SourceError;

type RepoPath<'a> = &'a Path;

pub fn fetch_repo(repo_path: RepoPath, refspecs: &[String]) -> Result<(), SourceError> {
    // might break on some platforms due to auth and ssh
    // especially ssh with password
    let refspecs_str = refspecs.iter().join(" ");
    let cd = std::env::current_dir().ok();
    _ = std::env::set_current_dir(repo_path);
    let output = Command::new("git")
        .args(["fetch", "origin", refspecs_str.as_str()])
        .output()
        .map_err(|_err| SourceError::ValidationFailed)?;
    // TODO(swarnimarun): get rid of assert
    assert!(output.status.success(), "{:#?}", output);
    _ = cd.map(std::env::set_current_dir);
    tracing::debug!("Repository fetched successfully!");
    Ok(())
}

pub fn git_src(
    source: &GitSource,
    cache_dir: &Path,
    recipe_dir: &Path,
) -> Result<PathBuf, SourceError> {
    tracing::info!(
        "git source: ({:?}) cache_dir: ({}) recipe_dir: ({})",
        source,
        cache_dir.display(),
        recipe_dir.display()
    );

    // TODO: handle reporting for unavailability of git better, or perhaps pointing to git binary manually?
    // currently a solution is to provide a `git` early in PATH with,
    // ```bash
    // export PATH="/path/to/git:$PATH"
    // ```

    let filename = match &source.url() {
        GitUrl::Url(url) => (|| Some(url.path_segments()?.last()?.to_string()))()
            .ok_or_else(|| SourceError::GitErrorStr("failed to get filename from url"))?,
        GitUrl::Path(path) => recipe_dir
            .join(path)
            .canonicalize()?
            .file_name()
            // canonicalized paths shouldn't end with ..
            .unwrap()
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
            // git doesn't support UNC paths, hence we can't use std::fs::canonicalize
            let path = dunce::canonicalize(path).map_err(|e| {
                tracing::error!("Path not found on system: {}", e);
                SourceError::GitError(format!("{}: Path not found on system", e))
            })?;

            let path = path.to_string_lossy();
            let mut command = Command::new("git");
            command
                .arg("clone")
                .arg("--recursive")
                .arg(format!("file://{}/.git", path).as_str())
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

    if git_lfs_required(&cache_path) {
        if !git_lfs_pull()? {
            // failed to do lfs pull, likely lfs not installed
            // TODO: should we consider erroring out?
            // return Err(SourceError::GitErrorStr(
            //     "failed to perform lfs pull, possibly git-lfs not installed",
            // ));
        }
    }

    tracing::info!("Checked out reference: '{}'", &source.rev());

    Ok(cache_path)
}

// TODO: we can use parallelization and work splitting for much faster search
// not sure if it's required though
fn git_lfs_required(repo_path: RepoPath) -> bool {
    // scan `**/.gitattributes`
    walkdir::WalkDir::new(repo_path)
        .follow_links(false)
        .into_iter()
        .filter_entry(|d| {
            // ignore .git folder (or folders in case of submodules)
            (d.file_type().is_dir() && !d.file_name().to_string_lossy().contains(".git"))
                || d.file_name()
                    .to_string_lossy()
                    .starts_with(".gitattributes")
        })
        .filter_map(|d| d.ok())
        .filter(|d| d.file_type().is_file())
        .filter_map(|d| std::fs::read_to_string(d.path()).ok())
        .any(|s| s.lines().any(|l| l.contains("lfs")))
}

fn git_lfs_pull() -> Result<bool, SourceError> {
    // verify lfs install
    let mut command = Command::new("git");
    command.args(["lfs", "install"]);
    let output = command
        .output()
        .map_err(|_| SourceError::GitErrorStr("failed to execute command"))?;
    if !output.status.success() {
        tracing::error!("`git lfs install` failed!");
        return Ok(false);
    }

    // git lfs pull
    let mut command = Command::new("git");
    command.args(["lfs", "pull"]);
    let output = command
        .output()
        .map_err(|_| SourceError::GitErrorStr("failed to execute command"))?;
    if !output.status.success() {
        tracing::error!("`git lfs pull` failed!");
        return Ok(false);
    }

    Ok(true)
}

#[cfg(test)]
mod tests {
    use crate::{
        recipe::parser::{GitSource, GitUrl},
        source::host_git_source::git_src,
    };

    #[tracing_test::traced_test]
    #[test]
    fn test_host_git_source() {
        let temp_dir = tempfile::tempdir().unwrap();
        let cache_dir = temp_dir.path().join("rattler-build-test-git-source");
        let cases = vec![
            (
                GitSource::create(
                    GitUrl::Url(
                        "https://github.com/prefix-dev/rattler-build"
                            .parse()
                            .unwrap(),
                    ),
                    "main".to_owned(),
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
                // TODO: this test assumes current dir is the root folder of the project which may
                // not be necessary for local runs.
                std::env::current_dir().unwrap().as_ref(),
            )
            .unwrap();
            assert_eq!(
                path.to_string_lossy(),
                cache_dir.join(repo_name).to_string_lossy()
            );
        }
    }
}
