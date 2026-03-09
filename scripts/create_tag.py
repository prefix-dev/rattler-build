"""Create and push a git tag idempotently.

If the tag already exists and points to HEAD, skips gracefully.
If the tag exists but points elsewhere, fails with an error.

Usage:
    pixi run -e release create-tag --tag v0.59.0
"""

import argparse
import subprocess
import sys


def run(cmd: list[str], **kwargs) -> subprocess.CompletedProcess[str]:
    print(f"  → {' '.join(cmd)}")
    return subprocess.run(cmd, check=True, text=True, **kwargs)


def rev_parse(ref: str) -> str | None:
    """Resolve a git ref to a commit hash, or None if it doesn't exist."""
    result = subprocess.run(
        ["git", "rev-parse", f"{ref}^{{commit}}"],
        capture_output=True,
        text=True,
    )
    if result.returncode == 0:
        return result.stdout.strip()
    return None


def main() -> None:
    parser = argparse.ArgumentParser(
        description="Create and push a git tag idempotently"
    )
    parser.add_argument("--tag", required=True, help="Tag name (e.g. v0.59.0)")
    args = parser.parse_args()

    tag: str = args.tag

    run(["git", "fetch", "origin", "--tags"])

    tag_commit = rev_parse(tag)
    head_commit = rev_parse("HEAD")

    if tag_commit is not None:
        if tag_commit == head_commit:
            print(f"Tag {tag} already exists and points to HEAD, skipping.")
            return
        else:
            print(
                f"Error: tag {tag} already exists but points to {tag_commit}, "
                f"not HEAD ({head_commit})",
                file=sys.stderr,
            )
            sys.exit(1)

    run(["git", "config", "user.email", "hi@prefix.dev"])
    run(["git", "config", "user.name", "Prefix.dev Release CI"])
    run(["git", "tag", "-m", f"Release {tag}", tag])
    run(["git", "push", "origin", tag])

    print(f"Tag {tag} created and pushed.")


if __name__ == "__main__":
    main()
