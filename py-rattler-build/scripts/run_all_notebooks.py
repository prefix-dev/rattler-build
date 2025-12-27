#!/usr/bin/env python3
"""Run all marimo notebooks in the notebooks directory."""

import subprocess
import sys
from pathlib import Path


def main() -> int:
    """Run all notebook files in order."""
    notebooks_dir = Path(__file__).parent.parent / "notebooks"

    # Find all notebook files and sort them
    notebook_files = sorted(notebooks_dir.glob("*.py"))

    if not notebook_files:
        print("No notebook files found in notebooks/")
        return 1

    print(f"Found {len(notebook_files)} notebook(s) to run:")
    for notebook in notebook_files:
        print(f"  - {notebook.name}")
    print()

    # Run each notebook
    failed = []
    for notebook in notebook_files:
        print(f"Running {notebook.name}...")
        result = subprocess.run(
            ["python", str(notebook)],
            cwd=notebooks_dir.parent,
        )

        if result.returncode != 0:
            failed.append(notebook.name)
            print(f"❌ {notebook.name} failed with exit code {result.returncode}")
        else:
            print(f"✅ {notebook.name} completed successfully")
        print()

    # Summary
    if failed:
        print(f"Failed notebooks: {', '.join(failed)}")
        return 1

    print("All notebooks ran successfully!")
    return 0


if __name__ == "__main__":
    sys.exit(main())
