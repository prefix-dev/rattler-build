#!/usr/bin/env python3
"""Install rattler-build binary to a custom location with a custom name."""

import argparse
import os
import shutil
import sys
from pathlib import Path


def main() -> None:
    parser = argparse.ArgumentParser(
        description="Install rattler-build binary to a custom location"
    )
    parser.add_argument("name", help="Name of the executable (e.g., 'rattler-build-dev')")
    parser.add_argument(
        "--dest",
        type=Path,
        default=Path.home() / ".pixi" / "bin",
        help="Destination directory (default: ~/.pixi/bin)",
    )
    parser.add_argument(
        "--debug",
        action="store_true",
        help="Install debug build instead of release",
    )
    args = parser.parse_args()

    target_dir = Path(os.environ.get("CARGO_TARGET_DIR", "target"))
    build_type = "debug" if args.debug else "release"
    exe_name = "rattler-build.exe" if sys.platform == "win32" else "rattler-build"

    source = target_dir / build_type / exe_name
    if not source.exists():
        print(f"Error: {source} does not exist. Run the build first.", file=sys.stderr)
        sys.exit(1)

    dest_name = f"{args.name}.exe" if sys.platform == "win32" else args.name
    dest = args.dest / dest_name

    args.dest.mkdir(parents=True, exist_ok=True)
    shutil.copy(source, dest)
    print(f"Installed {source} -> {dest}")


if __name__ == "__main__":
    main()
