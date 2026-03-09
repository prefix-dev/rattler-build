"""Extract and repackage Windows executables around Azure Trusted Signing.

Subcommands:
    extract   - Extract .exe files from .zip archives for signing
    repackage - Copy signed .exe files back and recreate .zip archives

Usage:
    pixi run -e release sign-windows-extract --artifacts-dir artifacts/ --sign-dir to-sign/
    # Azure trusted-signing-action signs the .exe files in to-sign/
    pixi run -e release sign-windows-repackage --artifacts-dir artifacts/ --sign-dir to-sign/
"""

import argparse
import shutil
import zipfile
from pathlib import Path


def find_windows_zips(artifacts_dir: Path) -> list[Path]:
    return sorted(artifacts_dir.glob("*-pc-windows-*.zip"))


def unique_exe_name(zip_path: Path, exe_name: str) -> str:
    """Create a unique name by prefixing with the zip stem to avoid collisions."""
    # e.g. rattler-build-x86_64-pc-windows-msvc--rattler-build.exe
    return f"{zip_path.stem}--{exe_name}"


def extract(artifacts_dir: Path, sign_dir: Path) -> None:
    """Extract .exe files from Windows .zip archives into sign_dir."""
    sign_dir.mkdir(parents=True, exist_ok=True)
    zips = find_windows_zips(artifacts_dir)

    if not zips:
        print("No Windows .zip archives found, nothing to extract.")
        return

    for zip_path in zips:
        print(f"Extracting from {zip_path.name}...")
        with zipfile.ZipFile(zip_path, "r") as zf:
            for name in zf.namelist():
                if name.endswith(".exe"):
                    exe_name = Path(name).name
                    dest = sign_dir / unique_exe_name(zip_path, exe_name)
                    print(f"  → {dest}")
                    with zf.open(name) as src, open(dest, "wb") as dst:
                        shutil.copyfileobj(src, dst)


def repackage(artifacts_dir: Path, sign_dir: Path) -> None:
    """Copy signed .exe files back into the .zip archives."""
    zips = find_windows_zips(artifacts_dir)

    if not zips:
        print("No Windows .zip archives found, nothing to repackage.")
        return

    for zip_path in zips:
        print(f"Repackaging {zip_path.name}...")

        # Read existing zip contents
        with zipfile.ZipFile(zip_path, "r") as zf:
            names = zf.namelist()

        # Find exe entries and their signed replacements
        exe_entries = [n for n in names if n.endswith(".exe")]

        # Recreate the zip, replacing exe files with signed versions
        old_zip = zip_path.with_suffix(".zip.bak")
        zip_path.rename(old_zip)

        with (
            zipfile.ZipFile(old_zip, "r") as old_zf,
            zipfile.ZipFile(zip_path, "w", zipfile.ZIP_DEFLATED) as new_zf,
        ):
            for name in names:
                if name in exe_entries:
                    exe_name = Path(name).name
                    signed_exe = sign_dir / unique_exe_name(zip_path, exe_name)
                    if signed_exe.exists():
                        print(f"  → replacing {name} with signed version")
                        new_zf.write(signed_exe, name)
                    else:
                        print(
                            f"  Warning: signed {exe_name} not found, keeping original"
                        )
                        new_zf.writestr(name, old_zf.read(name))
                else:
                    new_zf.writestr(name, old_zf.read(name))

        old_zip.unlink()


def main() -> None:
    parser = argparse.ArgumentParser(description="Windows signing helper")
    subparsers = parser.add_subparsers(dest="command", required=True)

    for cmd in ("extract", "repackage"):
        sub = subparsers.add_parser(cmd)
        sub.add_argument("--artifacts-dir", required=True, type=Path)
        sub.add_argument("--sign-dir", required=True, type=Path)

    args = parser.parse_args()

    if args.command == "extract":
        extract(args.artifacts_dir, args.sign_dir)
    elif args.command == "repackage":
        repackage(args.artifacts_dir, args.sign_dir)


if __name__ == "__main__":
    main()
