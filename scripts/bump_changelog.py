"""Update CHANGELOG.md with unreleased changes using git-cliff.

Usage:
    pixi run bump-changelog

Requires RELEASE_VERSION to be set (e.g. "0.60.0").
"""

import os
import shutil
import subprocess
import sys
from pathlib import Path

ROOT = Path(__file__).resolve().parent.parent
CHANGELOG = ROOT / "CHANGELOG.md"


def fail(msg: str) -> None:
    print(f"Error: {msg}", file=sys.stderr)
    sys.exit(1)


def get_github_token() -> str:
    """Try multiple sources for a GitHub token."""
    # 1. Explicit environment variable
    token = os.environ.get("GITHUB_TOKEN") or os.environ.get("GH_TOKEN")
    if token:
        return token

    # 2. gh CLI auth
    gh = shutil.which("gh")
    if gh:
        result = subprocess.run([gh, "auth", "token"], capture_output=True, text=True)
        if result.returncode == 0 and result.stdout.strip():
            return result.stdout.strip()

    fail(
        "no GitHub token found. Either:\n"
        "  - Run 'gh auth login'\n"
        "  - Set GITHUB_TOKEN or GH_TOKEN environment variable"
    )
    return ""  # unreachable, but makes the type checker happy


def main() -> None:
    version = os.environ.get("RELEASE_VERSION")
    if not version:
        fail("RELEASE_VERSION environment variable is not set")

    token = get_github_token()
    cmd = [
        "git-cliff",
        "--unreleased",
        "--prepend",
        str(CHANGELOG),
        "--github-token",
        token,
        "--tag",
        f"v{version}",
    ]
    result = subprocess.run(cmd, cwd=ROOT)
    sys.exit(result.returncode)


if __name__ == "__main__":
    main()
