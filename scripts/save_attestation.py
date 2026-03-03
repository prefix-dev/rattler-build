"""Save an attestation bundle to the output directory.

Usage:
    pixi run -e release save-attestation --bundle-path bundle.intoto.jsonl --output-dir artifacts/
"""

import argparse
import shutil
from pathlib import Path


def main() -> None:
    parser = argparse.ArgumentParser(description="Save attestation bundle")
    parser.add_argument(
        "--bundle-path", required=True, type=Path, help="Path to .intoto.jsonl bundle"
    )
    parser.add_argument(
        "--output-dir", required=True, type=Path, help="Output directory"
    )
    args = parser.parse_args()

    bundle: Path = args.bundle_path
    output_dir: Path = args.output_dir

    output_dir.mkdir(parents=True, exist_ok=True)
    dest = output_dir / bundle.name
    shutil.copy2(bundle, dest)
    print(f"Attestation bundle saved to {dest}")


if __name__ == "__main__":
    main()
