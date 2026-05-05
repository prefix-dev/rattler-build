"""Package a release binary into a tarball/zip and stage artifacts for upload.

The binary at `target/<target>/release/rattler-build[.exe]` is expected to
already be codesigned (macOS) or Azure-signed (Windows) when this script runs.

Creates:
    staging/<archive>         - .zip (windows) or .tar.gz (linux/macos) with binary + README + LICENSE
    staging/<binary>          - raw binary named rattler-build-<target>[.exe]
    staging/<archive>.sha256  - sha256 of the archive
    staging/<binary>.sha256   - sha256 of the raw binary

Outputs:
    pkg-name - archive filename (e.g. rattler-build-x86_64-unknown-linux-musl.tar.gz)

Usage:
    pixi run -e release package-binary --target x86_64-unknown-linux-musl
"""

import argparse
import hashlib
import os
import shutil
import subprocess
import tarfile
from pathlib import Path

ROOT = Path(__file__).resolve().parent.parent


def write_sha256(path: Path) -> None:
    h = hashlib.sha256()
    with open(path, "rb") as f:
        for chunk in iter(lambda: f.read(1024 * 1024), b""):
            h.update(chunk)
    (path.parent / f"{path.name}.sha256").write_text(f"{h.hexdigest()}  {path.name}\n")


def main() -> None:
    parser = argparse.ArgumentParser(description="Package release binary")
    parser.add_argument("--target", required=True, help="Rust target triple")
    args = parser.parse_args()

    target: str = args.target
    windows = "pc-windows" in target
    ext = ".exe" if windows else ""
    archive_ext = ".zip" if windows else ".tar.gz"

    pkg_basename = f"rattler-build-{target}"
    pkg_name = f"{pkg_basename}{archive_ext}"

    archive_dir = ROOT / "pkg" / pkg_basename
    archive_dir.mkdir(parents=True, exist_ok=True)

    binary_src = ROOT / "target" / target / "release" / f"rattler-build{ext}"
    shutil.copy2(binary_src, archive_dir / f"rattler-build{ext}")
    shutil.copy2(ROOT / "README.md", archive_dir / "README.md")
    shutil.copy2(ROOT / "LICENSE", archive_dir / "LICENSE")

    archive_path = ROOT / "pkg" / pkg_name
    if windows:
        subprocess.run(
            ["7z", "-y", "a", str(archive_path), f"{pkg_basename}/*"],
            check=True,
            cwd=ROOT / "pkg",
        )
    else:
        with tarfile.open(archive_path, "w:gz") as tf:
            for item in sorted(archive_dir.iterdir()):
                tf.add(item, arcname=f"{pkg_basename}/{item.name}")

    staging = ROOT / "staging"
    staging.mkdir(parents=True, exist_ok=True)

    staged_archive = staging / pkg_name
    shutil.copy2(archive_path, staged_archive)
    write_sha256(staged_archive)

    binary_name = f"rattler-build-{target}{ext}"
    staged_binary = staging / binary_name
    shutil.copy2(binary_src, staged_binary)
    write_sha256(staged_binary)

    print(f"Archive: {pkg_name}")
    print(f"Binary: {binary_name}")

    github_output = os.environ.get("GITHUB_OUTPUT")
    if github_output:
        with open(github_output, "a") as f:
            f.write(f"pkg-name={pkg_name}\n")


if __name__ == "__main__":
    main()
