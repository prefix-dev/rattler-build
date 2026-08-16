"""Cut a rattler-build release.

Branches from prefix-dev/rattler-build@main, bumps the version, updates the
changelog and lock files, commits, pushes the branch to prefix-dev, and opens
a PR.

The branch is pushed with upstream tracking, and in a colocated jj repo the
bookmark is tracked as well, so follow-up commits to the release PR are a
plain `git push` or `jj git push` away.

Because it always branches from the prefix-dev remote's main, it behaves the
same regardless of which branch (or detached HEAD) you happen to be on.

Usage:
    pixi run release

Shows the commits since the last release and asks for a major / minor / patch
bump.
"""

import re
import shutil
import subprocess
import sys
import tomllib
from pathlib import Path

import questionary

ROOT = Path(__file__).resolve().parent.parent
REPO = "prefix-dev/rattler-build"
CHANGELOG = ROOT / "CHANGELOG.md"

Version = tuple[int, int, int]


def run(cmd: list[str], *, cwd: Path = ROOT, env: dict[str, str] | None = None) -> None:
    print(f"  → {' '.join(cmd)}")
    subprocess.run(cmd, check=True, cwd=cwd, text=True, env=env)


def git_out(*args: str) -> str:
    return subprocess.run(
        ["git", *args], cwd=ROOT, text=True, capture_output=True
    ).stdout.strip()


def fail(msg: str) -> None:
    print(f"\nerror: {msg}", file=sys.stderr)
    sys.exit(1)


def is_jj() -> bool:
    """Whether ROOT is a colocated jj repo with the jj binary available."""
    return (ROOT / ".jj").is_dir() and shutil.which("jj") is not None


def prefix_dev_remote() -> str:
    """Name of the first git remote pointing at prefix-dev/rattler-build."""
    for name in git_out("remote").splitlines():
        url = git_out("remote", "get-url", name)
        normalized = url.removesuffix(".git").replace(":", "/")
        if normalized.endswith(f"github.com/{REPO}"):
            print(f"Using remote '{name}' for {REPO}.")
            return name
    fail(f"no git remote points at {REPO}; add one first")
    raise AssertionError("unreachable")


def sync_jj(branch: str, remote: str) -> None:
    """Track the pushed branch in a colocated jj repo and start a change on it.

    Tracking the remote bookmark keeps the release commit mutable (untracked
    remote bookmarks are immutable by default) and makes `jj git push` work.
    """
    if not is_jj():
        return
    print("Importing git refs into jj...")
    run(["jj", "git", "import"])
    run(["jj", "bookmark", "track", f"{branch}@{remote}"])
    run(["jj", "new", branch])


def parse(version: str) -> Version:
    parts = version.split(".")
    if len(parts) != 3 or not all(p.isdigit() for p in parts):
        fail(f"cannot parse version '{version}' as X.Y.Z")
    major, minor, patch = (int(p) for p in parts)
    return major, minor, patch


def fmt(version: Version) -> str:
    return ".".join(str(n) for n in version)


def fetched_version(main_ref: str) -> Version:
    """Version in Cargo.toml on the freshly fetched canonical main."""
    cargo = git_out("show", f"{main_ref}:Cargo.toml")
    return parse(tomllib.loads(cargo)["package"]["version"])


def gh_token() -> str:
    """A GitHub token from gh CLI auth, used to enrich git-cliff output."""
    return subprocess.run(
        ["gh", "auth", "token"], cwd=ROOT, text=True, capture_output=True
    ).stdout.strip()


def cliff_preview(tag: str, main_ref: str) -> str:
    """Render what git-cliff would add for the commits since `tag`.

    The range ends at the resolved commit SHA because git-cliff forwards the
    range end verbatim to the GitHub API as the `sha` parameter, which does
    not know local remote-tracking names like `upstream/main`.

    stderr is left attached to the terminal so git-cliff template or fetch
    errors surface instead of silently collapsing the preview to nothing.
    """
    head = git_out("rev-parse", main_ref)
    result = subprocess.run(
        [
            "git-cliff",
            "--strip",
            "header",
            "--github-token",
            gh_token(),
            f"{tag}..{head}",
        ],
        cwd=ROOT,
        text=True,
        stdout=subprocess.PIPE,
    )
    if result.returncode != 0:
        fail(f"git-cliff failed with exit code {result.returncode}")
    return result.stdout.strip()


def bump_changelog(version: str) -> None:
    """Prepend the unreleased changes to CHANGELOG.md under a new version tag."""
    run(
        [
            "git-cliff",
            "--unreleased",
            "--prepend",
            str(CHANGELOG),
            "--github-token",
            gh_token(),
            "--tag",
            f"v{version}",
        ]
    )


def latest_tag(main_ref: str) -> str:
    tag = git_out("describe", "--tags", "--abbrev=0", "--match", "v*", main_ref)
    if not tag:
        fail("no v* tag reachable from canonical main")
    return tag


def select_version(current: Version) -> str:
    major, minor, patch = current
    options: dict[str, Version] = {
        "major": (major + 1, 0, 0),
        "minor": (major, minor + 1, 0),
        "patch": (major, minor, patch + 1),
    }
    choices = [
        questionary.Choice(f"{kind:<5} → {fmt(version)}", value=fmt(version))
        for kind, version in options.items()
    ]
    answer = questionary.select(
        "Select the bump:", choices=choices, default=choices[-1]
    ).ask()
    if answer is None:
        fail("aborted")
    return answer


def edit_highlights(interactive: bool) -> None:
    """Let the user write the changelog Highlights before committing."""
    if not interactive:
        return
    input("Edit the 'Highlights' section in CHANGELOG.md, then press Enter...")


def changelog_section(version: str) -> str:
    """Extract the section for `version` from CHANGELOG.md for the PR body."""
    content = CHANGELOG.read_text()
    pattern = rf"(## \[{re.escape(version)}\].*?)(?=\n## \[|\n---|\Z)"
    match = re.search(pattern, content, re.DOTALL)
    return match.group(1).strip() if match else f"Release {version}"


def main() -> None:
    interactive = sys.stdin.isatty()

    if git_out("status", "--porcelain"):
        if is_jj():
            # In a colocated jj repo, in-progress work lives committed in @ and
            # shows as dirty to git. Set it aside with `jj new` so git sees a
            # clean tree for the upcoming `git switch`; @- stays as a recoverable
            # loose head.
            print("Working copy is dirty; running `jj new` to set it aside...")
            run(["jj", "new"])
        else:
            fail("working directory is not clean; commit or stash first")

    remote = prefix_dev_remote()
    main_ref = f"{remote}/main"

    print(f"Fetching canonical main from {REPO}...")
    run(["git", "fetch", remote, "main"])

    current = fetched_version(main_ref)
    tag = latest_tag(main_ref)
    if parse(tag.lstrip("v")) != current:
        fail(
            f"Cargo.toml on main ({fmt(current)}) is ahead of the latest tag "
            f"({tag}); a release may already be pending"
        )

    print(f"\nChangelog preview since {tag}:")
    print(cliff_preview(tag, main_ref) or "  (none)")

    version = select_version(current)

    print(f"\n=== Releasing {version} ===\n")

    branch = f"release-{version}"
    run(["git", "switch", "-C", branch, "--no-track", main_ref])

    print("Patching version files...")
    run(["tbump", "--non-interactive", "--only-patch", version])

    print("Updating changelog...")
    bump_changelog(version)
    edit_highlights(interactive)

    print("Updating Cargo.lock (root)...")
    run(["cargo", "update", "--workspace"])
    print("Updating Cargo.lock (py-rattler-build/rust)...")
    run(["cargo", "update", "--workspace"], cwd=ROOT / "py-rattler-build" / "rust")
    print("Updating pixi.lock (root)...")
    run(["pixi", "lock"])
    print("Updating pixi.lock (py-rattler-build)...")
    run(["pixi", "lock"], cwd=ROOT / "py-rattler-build")

    print("Committing...")
    run(["git", "commit", "--all", "--message", f"chore: release {version}"])

    print("Pushing branch...")
    run(["git", "push", "--set-upstream", remote, branch])

    print("Opening pull request...")
    run(
        [
            "gh",
            "pr",
            "create",
            "--repo",
            REPO,
            "--base",
            "main",
            "--head",
            branch,
            "--title",
            f"chore: release {version}",
            "--body",
            changelog_section(version),
        ]
    )

    sync_jj(branch, remote)

    print("\n=== Done ===")
    print(f"Opened release PR for {version}.")
    print("Push follow-ups with `git push` or `jj git push`.")
    print("After merge, run the 'Release artifacts' workflow via workflow_dispatch.")


if __name__ == "__main__":
    main()
