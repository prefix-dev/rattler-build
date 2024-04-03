//! This module contains the implementation of the fetching of `GitSource` struct.

use std::{
    io::IsTerminal,
    path::{Path, PathBuf},
    process::Command,
};

use fs_extra::dir::remove;

use crate::recipe::parser::{GitSource, GitUrl};
use crate::system_tools::{SystemTools, Tool};

use super::SourceError;

/// Fetch the given repository using the host `git` executable.
pub fn fetch_repo(repo_path: &Path, url: &str, rev: &str) -> Result<(), SourceError> {
    tracing::info!("Fetching repository from {} at {} into {}", url, rev, repo_path.display());

    if !repo_path.exists() {
        return Err(SourceError::GitErrorStr("repository path does not exist"));
    }

    let mut command = git_command("fetch");
    let output = command
        .args([url, rev, "--recurse-submodules"])
        .current_dir(repo_path)
        .output()
        .map_err(|_err| SourceError::ValidationFailed)?;

    if !output.status.success() {
        tracing::debug!("Repository fetch for revision {:?} failed!", rev);
        return Err(SourceError::GitErrorStr(
            "failed to git fetch refs from origin",
        ));
    }

    // try to suppress detached head warning
    let _ = Command::new("git")
        .current_dir(repo_path)
        .args(["config", "--local", "advice.detachedHead", "false"])
        .status();

    // checkout fetch_head
    let mut command = Command::new("git");
    let output = command
        .args(["reset", "--hard", "FETCH_HEAD"])
        .current_dir(repo_path)
        .output()
        .map_err(|_err| SourceError::ValidationFailed)?;

    if !output.status.success() {
        tracing::debug!("Repository fetch for revision {:?} failed!", rev);
        return Err(SourceError::GitErrorStr("failed to checkout FETCH_HEAD"));
    }

    let mut command = Command::new("git");
    let output = command
        .args(["checkout", rev])
        .current_dir(repo_path)
        .output()
        .map_err(|_err| SourceError::ValidationFailed)?;

    if !output.status.success() {
        tracing::debug!("Repository checkout for revision {:?} failed!", rev);
        return Err(SourceError::GitErrorStr("failed to checkout FETCH_HEAD"));
    }

    tracing::debug!("Repository fetched successfully!");
    Ok(())
}

/// Create a `git` command with the given subcommand.
fn git_command(sub_cmd: &str) -> Command {
    let mut command = Command::new("git");
    command.arg(sub_cmd);

    if std::io::stdin().is_terminal() {
        command.stdout(std::process::Stdio::inherit());
        command.stderr(std::process::Stdio::inherit());
        command.arg("--progress");
    }
    command
}

/// Fetch the git repository specified by the given source and place it in the cache directory.
pub fn git_src(
    system_tools: &SystemTools,
    source: &GitSource,
    cache_dir: &Path,
    recipe_dir: &Path,
) -> Result<(PathBuf, String), SourceError> {
    // depth == -1, fetches the entire git history
    if !source.rev().is_head() && (source.depth().is_some() && source.depth() != Some(-1)) {
        return Err(SourceError::GitErrorStr(
            "use of `depth` with `rev` is invalid, they are mutually exclusive",
        ));
    }

    let filename = match &source.url() {
        GitUrl::Url(url) => (|| Some(url.path_segments()?.last()?.to_string()))()
            .ok_or_else(|| SourceError::GitErrorStr("failed to get filename from url"))?,
        GitUrl::Ssh(url) => (|| {
            Some(
                url.trim_end_matches(".git")
                    .split(std::path::MAIN_SEPARATOR)
                    .last()?
                    .to_string(),
            )
        })()
        .ok_or_else(|| SourceError::GitErrorStr("failed to get filename from SSH url"))?,
        GitUrl::Path(path) => recipe_dir
            .join(path)
            .canonicalize()?
            .file_name()
            .expect("unreachable, canonicalized paths shouldn't end with ..")
            .to_string_lossy()
            .to_string(),
    };

    let cache_name = PathBuf::from(filename);
    let cache_path = cache_dir.join(cache_name);

    let rev = source.rev().to_string();

    // Initialize or clone the repository depending on the source's git_url.
    match &source.url() {
        GitUrl::Url(_) | GitUrl::Ssh(_) => {
            let url = match &source.url() {
                GitUrl::Url(url) => url.to_string(),
                GitUrl::Ssh(url) => url.to_string(),
                _ => unreachable!(),
            };
            // If the cache_path exists, initialize the repo and fetch the specified revision.
            if !cache_path.exists() {
                let mut command = system_tools
                    .call(Tool::Git)
                    .map_err(|_| SourceError::GitErrorStr("Failed to execute command"))?;

                command
                    .args(["clone", "-n", source.url().to_string().as_str()])
                    .arg(cache_path.as_os_str());

                let output = command
                    .output()
                    .map_err(|_e| SourceError::GitErrorStr("Failed to execute clone command"))?;

                if !output.status.success() {
                    return Err(SourceError::GitError(format!(
                        "Git clone failed for source: {}",
                        String::from_utf8_lossy(&output.stderr)
                    )));
                }
            }

            assert!(cache_path.exists());
            fetch_repo(&cache_path, &url.to_string(), &rev)?;
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
            let mut command = git_command("clone");

            command
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
        }
    }

    // Resolve the reference and set the head to the specified revision.
    let output = Command::new("git")
        .current_dir(&cache_path)
        .args(["rev-parse", rev.as_str()])
        .output()
        .map_err(|_| SourceError::GitErrorStr("git rev-parse failed"))?;

    if !output.status.success() {
        tracing::error!("Command failed: `git rev-parse \"{}\"`", &rev);
        return Err(SourceError::GitErrorStr("failed to get valid hash for rev"));
    }

    let ref_git = String::from_utf8(output.stdout)
        .map_err(|_| SourceError::GitErrorStr("failed to parse git rev as utf-8"))?
        .trim()
        .to_owned();

    // only do lfs pull if a requirement!
    if source.lfs() {
        git_lfs_pull()?;
    }

    tracing::info!(
        "Checked out revision: '{}' at '{}'",
        &rev,
        ref_git.as_str().trim()
    );

    Ok((cache_path, ref_git))
}

fn git_lfs_pull() -> Result<(), SourceError> {
    // verify lfs install
    let mut command = Command::new("git");
    command.args(["lfs", "install"]);
    let output = command
        .output()
        .map_err(|_| SourceError::GitErrorStr("failed to execute command"))?;
    if !output.status.success() {
        return Err(SourceError::GitErrorStr(
            "git-lfs not installed, but required",
        ));
    }

    // git lfs pull
    let mut command = Command::new("git");
    command.args(["lfs", "pull"]);
    let output = command
        .output()
        .map_err(|_| SourceError::GitErrorStr("failed to execute command"))?;
    if !output.status.success() {
        return Err(SourceError::GitErrorStr("`git lfs pull` failed!"));
    }

    Ok(())
}

#[cfg(test)]
#[cfg(not(all(target_arch = "aarch64", target_os = "linux")))]
mod tests {
    use crate::{
        recipe::parser::{GitRev, GitSource, GitUrl},
        source::git_source::git_src,
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
                    GitRev::Branch("main".to_owned()),
                    None,
                    vec![],
                    None,
                    false,
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
                    GitRev::Tag("v0.1.3".to_owned()),
                    None,
                    vec![],
                    None,
                    false,
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
                    GitRev::Tag("v0.1.2".to_owned()),
                    None,
                    vec![],
                    None,
                    false,
                ),
                "rattler-build",
            ),
            (
                GitSource::create(
                    GitUrl::Path("../rattler-build".parse().unwrap()),
                    GitRev::default(),
                    None,
                    vec![],
                    None,
                    false,
                ),
                "rattler-build",
            ),
        ];
        let system_tools = crate::system_tools::SystemTools::new();

        for (source, repo_name) in cases {
            let res = git_src(
                &system_tools,
                &source,
                cache_dir.as_ref(),
                // TODO: this test assumes current dir is the root folder of the project which may
                // not be necessary for local runs.
                std::env::current_dir().unwrap().as_ref(),
            )
            .unwrap();
            assert_eq!(
                res.0.to_string_lossy(),
                cache_dir.join(repo_name).to_string_lossy()
            );
        }
    }
}
