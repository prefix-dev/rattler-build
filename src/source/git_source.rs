//! This module contains the implementation of the fetching of `GitSource` struct.

use std::{
    io::IsTerminal,
    path::{Path, PathBuf},
    process::{Command, Output},
};

use crate::system_tools::{SystemTools, Tool};
use crate::{
    recipe::parser::{GitRev, GitSource, GitUrl},
    system_tools::ToolError,
};

use super::SourceError;

/// Fetch the given repository using the host `git` executable.
pub fn fetch_repo(
    system_tools: &SystemTools,
    repo_path: &Path,
    url: &str,
    rev: &GitRev,
) -> Result<(), SourceError> {
    tracing::info!(
        "Fetching repository from {} at {} into {}",
        url,
        rev,
        repo_path.display()
    );

    if !repo_path.exists() {
        return Err(SourceError::GitErrorStr("repository path does not exist"));
    }

    let mut command = git_command(system_tools, "fetch")?;
    let refspec = match rev {
        GitRev::Branch(_) => format!("{0}:{0}", rev),
        GitRev::Tag(_) => format!("{0}:{0}", rev),
        _ => format!("{}", rev),
    };
    let output = command
        .args([
            // Allow non-fast-forward fetches.
            "--force",
            // Allow update a branch even if we currently have it checked out.
            // This should be safe, as we do a `git checkout` below to refresh
            // the working copy.
            "--update-head-ok",
            // Avoid overhead of fetching unused tags.
            "--no-tags",
            url,
            refspec.as_str(),
        ])
        .current_dir(repo_path)
        .output()
        .map_err(|_err| SourceError::ValidationFailed)?;

    if !output.status.success() {
        tracing::debug!("Repository fetch for revision {:?} failed!", rev);
        return Err(SourceError::GitError(format!(
            "failed to git fetch refs from origin: {}",
            std::str::from_utf8(&output.stderr).unwrap()
        )));
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
        return Err(SourceError::GitError(format!(
            "failed to checkout FETCH_HEAD: {}",
            std::str::from_utf8(&output.stderr).unwrap()
        )));
    }

    let output = git_command(system_tools, "checkout")?
        .arg(rev.to_string())
        .current_dir(repo_path)
        .output()
        .map_err(|_err| SourceError::ValidationFailed)?;

    if !output.status.success() {
        tracing::debug!("Repository checkout for revision {:?} failed!", rev);
        return Err(SourceError::GitError(format!(
            "failed to checkout FETCH_HEAD: {}",
            std::str::from_utf8(&output.stderr).unwrap()
        )));
    }

    // Update submodules
    let output = git_command(system_tools, "submodule")?
        .args(["update", "--init", "--recursive"])
        .current_dir(repo_path)
        .output()?;

    if !output.status.success() {
        tracing::debug!("Submodule update failed!");
        return Err(SourceError::GitError(format!(
            "failed to update submodules: {}",
            std::str::from_utf8(&output.stderr).unwrap()
        )));
    }

    tracing::debug!("Repository fetched successfully!");
    Ok(())
}

/// Create a `git` command with the given subcommand.
fn git_command(system_tools: &SystemTools, sub_cmd: &str) -> Result<Command, ToolError> {
    let mut command = system_tools.call(Tool::Git)?;
    command.arg(sub_cmd);

    if std::io::stdin().is_terminal() {
        command.stdout(std::process::Stdio::inherit());
        command.stderr(std::process::Stdio::inherit());
        if sub_cmd != "submodule" {
            command.arg("--progress");
        }
    }

    Ok(command)
}

/// Run a git command and log precisely what went wrong.
fn run_git_command(command: &mut Command) -> Result<Output, SourceError> {
    let output = command
        .output()
        .map_err(|_err| SourceError::GitErrorStr("could not execute git"))?;

    if !output.status.success() {
        tracing::error!("Command failed: {:?}", command);
        tracing::error!(
            "Command output: {}",
            String::from_utf8_lossy(&output.stdout)
        );
        tracing::error!(
            "Command stderr: {}",
            String::from_utf8_lossy(&output.stderr)
        );

        return Err(SourceError::GitError(format!(
            "failed to run command: {:?}",
            command
        )));
    }

    Ok(output)
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
        GitUrl::Url(url) => (|| {
            Some(
                url.path_segments()?
                    .filter(|x| !x.is_empty())
                    .next_back()?
                    .to_string(),
            )
        })()
        .ok_or_else(|| SourceError::GitErrorStr("failed to get filename from url"))?,
        GitUrl::Ssh(url) => (|| {
            Some(
                url.trim_end_matches(".git")
                    .split('/')
                    .filter(|x| !x.is_empty())
                    .next_back()?
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

    if filename.is_empty() {
        return Err(SourceError::GitErrorStr("failed to get filename from url"));
    }

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
                let mut command = git_command(system_tools, "clone")?;
                command
                    .args([
                        // Avoid overhead of fetching unused tags.
                        "--no-tags",
                        "-n",
                        source.url().to_string().as_str(),
                    ])
                    .arg(cache_path.as_os_str());

                let _ = run_git_command(&mut command)?;
            }

            assert!(cache_path.exists());
            fetch_repo(system_tools, &cache_path, &url.to_string(), source.rev())?;
        }
        GitUrl::Path(path) => {
            if cache_path.exists() {
                // Remove old cache so it can be overwritten.
                if let Err(remove_error) = fs_err::remove_dir_all(&cache_path) {
                    tracing::error!("Failed to remove old cache directory: {}", remove_error);
                    return Err(SourceError::FileSystemError(remove_error));
                }
            }
            // git doesn't support UNC paths, hence we can't use std::fs::canonicalize
            let path = dunce::canonicalize(recipe_dir.join(path)).map_err(|e| {
                tracing::error!("Path not found on system: {}", e);
                SourceError::GitError(format!("{}: Path not found on system", e))
            })?;

            let path = path.to_string_lossy();
            let mut command = git_command(system_tools, "clone")?;

            command
                .arg("--recursive")
                .arg(format!("file://{}/.git", path).as_str())
                .arg(cache_path.as_os_str());

            if let Some(depth) = source.depth() {
                command.args(["--depth", depth.to_string().as_str()]);
            }

            let _ = run_git_command(&mut command)?;
        }
    }

    // Resolve the reference and set the head to the specified revision.
    let output = run_git_command(
        Command::new("git")
            .current_dir(&cache_path)
            // make sure that we get the commit, not the annotated tag
            .args(["rev-parse", &format!("{}^0", rev)]),
    )?;

    let ref_git = String::from_utf8(output.stdout)
        .map_err(|_| SourceError::GitErrorStr("failed to parse git rev as utf-8"))?
        .trim()
        .to_owned();

    // only do lfs pull if a requirement!
    if source.lfs() {
        git_lfs_pull(&ref_git)?;
    }

    tracing::info!(
        "Checked out revision: '{}' at '{}'",
        &rev,
        ref_git.as_str().trim()
    );

    Ok((cache_path, ref_git))
}

fn git_lfs_pull(git_ref: &str) -> Result<(), SourceError> {
    // verify git-lfs is installed
    let output = Command::new("git")
        .args(["lfs", "ls-files"])
        .output()
        .map_err(|_| SourceError::GitErrorStr("failed to execute command"))?;

    if !output.status.success() {
        return Err(SourceError::GitErrorStr(
            "git-lfs not installed, but required",
        ));
    }

    // git lfs fetch
    run_git_command(Command::new("git").args(["lfs", "fetch", "origin", git_ref]))?;

    // git lfs checkout
    run_git_command(Command::new("git").args(["lfs", "checkout"]))?;

    Ok(())
}

#[cfg(test)]
#[cfg(not(all(
    any(target_arch = "aarch64", target_arch = "powerpc64"),
    target_os = "linux"
)))]
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
        let recipe_dir = temp_dir.path().join("recipe");
        fs_err::create_dir_all(&recipe_dir).unwrap();

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
        ];
        let system_tools = crate::system_tools::SystemTools::new();

        for (source, repo_name) in cases {
            let res = git_src(&system_tools, &source, cache_dir.as_ref(), &recipe_dir).unwrap();
            assert_eq!(
                res.0.to_string_lossy(),
                cache_dir.join(repo_name).to_string_lossy()
            );
        }
    }
}
