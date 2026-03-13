"""Run release-plz release-pr then sync py-rattler-build/rust/Cargo.lock.

Creates or updates a release PR via release-plz, then checks out the PR branch,
runs `cargo update --workspace` for the py-rattler-build lockfile, and pushes a
fixup commit if the lockfile changed.

Requires:
    GIT_TOKEN       - GitHub token with contents:write and pull-requests:write
    GITHUB_REPO_URL - full repo URL (e.g. https://github.com/prefix-dev/rattler-build)

Usage:
    pixi run -e release release-plz-pr
"""

import json
import os
import subprocess
import sys
from pathlib import Path

ROOT = Path(__file__).resolve().parent.parent
PY_MANIFEST = ROOT / "py-rattler-build" / "rust" / "Cargo.toml"
PY_CARGO_LOCK = ROOT / "py-rattler-build" / "rust" / "Cargo.lock"


def run(cmd: list[str], **kwargs) -> subprocess.CompletedProcess[str]:
    print(f"  → {' '.join(cmd)}")
    return subprocess.run(cmd, check=True, text=True, cwd=ROOT, **kwargs)


def release_pr(git_token: str, repo_url: str) -> list[dict]:
    """Run release-plz release-pr and return the list of created PRs."""
    cmd = [
        "release-plz",
        "release-pr",
        "--git-token",
        git_token,
        "--output",
        "json",
    ]
    if repo_url:
        cmd += ["--repo-url", repo_url]

    print(
        "  → release-plz release-pr --git-token *** --output json"
        + (f" --repo-url {repo_url}" if repo_url else "")
    )
    result = subprocess.run(cmd, check=True, text=True, cwd=ROOT, capture_output=True)

    if result.stderr:
        print(result.stderr, end="", file=sys.stderr)

    if not result.stdout.strip():
        return []

    data = json.loads(result.stdout)
    return data.get("prs", [])


def sync_py_cargo_lock(git_token: str, repo_url: str, branch: str) -> None:
    """Check out the PR branch, update the py-rattler-build lockfile, and push."""
    if not PY_MANIFEST.exists():
        print(
            f"Warning: {PY_MANIFEST.relative_to(ROOT)} not found, skipping lockfile sync",
            file=sys.stderr,
        )
        return

    run(["git", "checkout", branch])
    run(["cargo", "update", "--workspace", "--manifest-path", str(PY_MANIFEST)])

    diff = subprocess.run(
        ["git", "diff", "--exit-code", str(PY_CARGO_LOCK)],
        cwd=ROOT,
    )
    if diff.returncode == 0:
        print("py-rattler-build/rust/Cargo.lock is already up to date.")
        return

    run(["git", "add", str(PY_CARGO_LOCK)])
    run(
        [
            "git",
            "-c",
            "user.name=prefix-dev-release-bot[bot]",
            "-c",
            "user.email=prefix-dev-release-bot[bot]@users.noreply.github.com",
            "commit",
            "--message",
            "chore: sync py-rattler-build Cargo.lock",
        ],
    )

    # Set up credentials for push (checkout uses persist-credentials: false)
    authenticated_url = repo_url.replace(
        "https://", f"https://x-access-token:{git_token}@"
    )
    subprocess.run(
        ["git", "remote", "set-url", "origin", authenticated_url],
        check=True,
        cwd=ROOT,
    )
    run(["git", "push", "origin", branch])


def main() -> None:
    git_token = os.environ.get("GIT_TOKEN", "")
    if not git_token:
        print("Error: GIT_TOKEN environment variable is required", file=sys.stderr)
        sys.exit(1)

    repo_url = os.environ.get("GITHUB_REPO_URL", "")

    prs = release_pr(git_token, repo_url)

    if not prs:
        print("No PRs created.")
        return

    for pr in prs:
        branch = pr.get("head_branch", "")
        if not branch:
            print("No PR branch found, skipping lockfile sync.")
            continue

        print(f"PR created on branch {branch}, syncing py-rattler-build Cargo.lock...")
        sync_py_cargo_lock(git_token, repo_url, branch)


if __name__ == "__main__":
    main()
