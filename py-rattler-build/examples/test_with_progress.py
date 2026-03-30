#!/usr/bin/env python3
"""
Example: Running package tests with streaming progress output.

This example demonstrates how to use rattler-build's progress reporting
when running tests on a built package, so you can see test output in
real-time instead of only after completion.

Usage:
    python test_with_progress.py path/to/package.conda
"""

import sys
from pathlib import Path

from rattler_build import Package
from rattler_build.progress import SimpleProgressCallback


def test_package_with_progress(package_path: Path) -> None:
    """Run tests on a package with streaming progress output.

    Args:
        package_path: Path to a .conda or .tar.bz2 package file
    """
    print(f"Loading package: {package_path}")
    pkg = Package.from_file(package_path)
    print(f"  {pkg.name} {pkg.version} ({pkg.archive_type})")
    print(f"  Tests: {pkg.test_count}")

    if pkg.test_count == 0:
        print("No tests found in package.")
        return

    # Run all tests with a progress callback for real-time output
    callback = SimpleProgressCallback()
    print("\nRunning tests with streaming output:\n")
    results = pkg.run_tests(progress_callback=callback)

    # Print summary
    print("\n" + "=" * 60)
    print("Test Summary:")
    print("=" * 60)
    for r in results:
        status = "PASS" if r.success else "FAIL"
        print(f"  Test {r.test_index}: {status}")
        if not r.success:
            print("  Output:")
            for line in r.output:
                print(f"    {line}")

    passed = sum(1 for r in results if r.success)
    total = len(results)
    print(f"\n{passed}/{total} tests passed.")


def main() -> None:
    """Main entry point."""
    if len(sys.argv) < 2:
        print("Usage: python test_with_progress.py <package.conda>")
        print("\nExamples:")
        print("  python test_with_progress.py output/noarch/mypackage-1.0-py_0.conda")
        sys.exit(1)

    package_path = Path(sys.argv[1])
    if not package_path.exists():
        print(f"Error: Package file not found: {package_path}")
        sys.exit(1)

    try:
        test_package_with_progress(package_path)
    except Exception as e:
        print(f"Error: {e}")
        import traceback

        traceback.print_exc()
        sys.exit(1)


if __name__ == "__main__":
    main()
