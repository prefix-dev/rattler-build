"""Run release-plz release to publish crates and create git tags.

Requires:
    GIT_TOKEN - GitHub token with contents:write permission

Usage:
    pixi run -e release release-plz-release
"""

import os
import subprocess
import sys
from pathlib import Path

ROOT = Path(__file__).resolve().parent.parent


def main() -> None:
    git_token = os.environ.get("GIT_TOKEN", "")
    if not git_token:
        print("Error: GIT_TOKEN environment variable is required", file=sys.stderr)
        sys.exit(1)

    repo_url = os.environ.get("GITHUB_REPO_URL", "")

    cmd = [
        "release-plz",
        "release",
        "--git-token",
        git_token,
    ]
    if repo_url:
        cmd += ["--repo-url", repo_url]

    result = subprocess.run(cmd, cwd=ROOT)
    sys.exit(result.returncode)


if __name__ == "__main__":
    main()
