"""Package a release binary into a tarball/zip and stage artifacts for upload.

Creates:
    staging/<archive>  - .tar.gz (unix) or .zip (windows) with binary + README + LICENSE
    staging/<binary>   - raw binary named rattler-build-<target>[.exe]

Outputs:
    pkg-name - archive filename (e.g. rattler-build-x86_64-unknown-linux-musl.tar.gz)
    prefix   - artifact prefix: "release" for Linux, "build" for macOS/Windows (needs signing)

Usage:
    pixi run -e release package-binary --target x86_64-unknown-linux-musl
"""

import argparse
import os
import shutil
import subprocess
import tarfile
from pathlib import Path

ROOT = Path(__file__).resolve().parent.parent


def is_windows_target(target: str) -> bool:
    return "pc-windows" in target


def needs_signing(target: str) -> bool:
    return "apple-darwin" in target or "pc-windows" in target


def main() -> None:
    parser = argparse.ArgumentParser(description="Package release binary")
    parser.add_argument("--target", required=True, help="Rust target triple")
    args = parser.parse_args()

    target: str = args.target
    windows = is_windows_target(target)
    ext = ".exe" if windows else ""
    archive_ext = ".zip" if windows else ".tar.gz"

    pkg_basename = f"rattler-build-{target}"
    pkg_name = f"{pkg_basename}{archive_ext}"

    # Create archive directory with binary + docs
    archive_dir = ROOT / "pkg" / pkg_basename
    archive_dir.mkdir(parents=True, exist_ok=True)

    binary_src = ROOT / "target" / target / "release" / f"rattler-build{ext}"
    shutil.copy2(binary_src, archive_dir / f"rattler-build{ext}")
    shutil.copy2(ROOT / "README.md", archive_dir / "README.md")
    shutil.copy2(ROOT / "LICENSE", archive_dir / "LICENSE")

    # Create archive
    archive_path = ROOT / "pkg" / pkg_name
    if windows:
        # Use 7z on Windows for zip creation (handles paths better)
        subprocess.run(
            ["7z", "-y", "a", str(archive_path), f"{pkg_basename}/*"],
            check=True,
            cwd=ROOT / "pkg",
        )
    else:
        with tarfile.open(archive_path, "w:gz") as tf:
            for item in sorted(archive_dir.iterdir()):
                tf.add(item, arcname=f"{pkg_basename}/{item.name}")

    # Stage everything flat for upload
    staging = ROOT / "staging"
    staging.mkdir(parents=True, exist_ok=True)
    shutil.copy2(archive_path, staging / pkg_name)

    binary_name = f"rattler-build-{target}{ext}"
    shutil.copy2(binary_src, staging / binary_name)

    # Determine artifact prefix
    prefix = "build" if needs_signing(target) else "release"

    print(f"Archive: {pkg_name}")
    print(f"Binary: {binary_name}")
    print(f"Prefix: {prefix}")

    github_output = os.environ.get("GITHUB_OUTPUT")
    if github_output:
        with open(github_output, "a") as f:
            f.write(f"pkg-name={pkg_name}\n")
            f.write(f"prefix={prefix}\n")


if __name__ == "__main__":
    main()
