"""Extract the version from Cargo.toml and print GITHUB_OUTPUT lines.

Outputs:
    version - e.g. 0.59.0
    tag     - e.g. v0.59.0

Usage:
    pixi run -e release extract-version
"""

import sys
import tomllib
from pathlib import Path

ROOT = Path(__file__).resolve().parent.parent


def main() -> None:
    cargo_toml = ROOT / "Cargo.toml"
    with cargo_toml.open("rb") as f:
        data = tomllib.load(f)

    version = data.get("package", {}).get("version")
    if version is None:
        print("Error: could not find package.version in Cargo.toml", file=sys.stderr)
        sys.exit(1)

    tag = f"v{version}"
    print(f"version={version}")
    print(f"tag={tag}")


if __name__ == "__main__":
    main()
