"""Create a GitHub release with changelog and all artifacts.

Expects the git tag to already exist (created by the tag-release job).
Expects GH_TOKEN to be set (standard for gh CLI).

Usage:
    pixi run -e release create-release --tag v0.59.0 --assets-dir release-assets/
"""

import argparse
import re
import subprocess
import sys
from pathlib import Path

ROOT = Path(__file__).resolve().parent.parent


def run(cmd: list[str], **kwargs) -> subprocess.CompletedProcess[str]:
    print(f"  → {' '.join(cmd)}")
    return subprocess.run(cmd, check=True, text=True, **kwargs)


def get_changelog(version: str) -> str:
    """Extract changelog for a specific version from CHANGELOG.md."""
    changelog_path = ROOT / "CHANGELOG.md"
    content = changelog_path.read_text()

    # Strip leading 'v' for matching against CHANGELOG headers
    version = version.lstrip("v")

    # Find the section for this version: starts with ## [version]
    # and ends at the next ## [ or --- or end of file
    pattern = rf"(## \[{re.escape(version)}\].*?)(?=\n## \[|\n---|\Z)"
    match = re.search(pattern, content, re.DOTALL)
    if match:
        return match.group(1).strip()
    return ""


def main() -> None:
    parser = argparse.ArgumentParser(description="Create GitHub release")
    parser.add_argument("--tag", required=True, help="Release tag (e.g. v0.59.0)")
    parser.add_argument(
        "--assets-dir", required=True, type=Path, help="Directory with release assets"
    )
    args = parser.parse_args()

    tag: str = args.tag
    assets_dir: Path = args.assets_dir

    # Collect asset files
    assets = sorted(f for f in assets_dir.iterdir() if f.is_file())
    if not assets:
        print(f"Error: no files found in {assets_dir}", file=sys.stderr)
        sys.exit(1)

    print(f"Found {len(assets)} asset(s) for release {tag}:")
    for a in assets:
        print(f"  {a.name}")

    # Read changelog from CHANGELOG.md
    print("\nReading changelog...")
    changelog = get_changelog(tag)
    if not changelog:
        changelog = f"Release {tag}"
    print(f"Changelog:\n{changelog}\n")

    # Create release
    print("Creating GitHub release...")
    run(
        [
            "gh",
            "release",
            "create",
            tag,
            "--title",
            tag,
            "--notes",
            changelog,
            *[str(a) for a in assets],
        ]
    )

    print(f"\nRelease {tag} created successfully.")


if __name__ == "__main__":
    main()
