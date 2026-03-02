#!/usr/bin/env python3
"""Check that a forbidden crate is not pulled in as a dependency.

Useful for verifying feature flag configurations (e.g., native-tls vs rustls).
"""

import json
import subprocess
import sys

# Configuration
FORBIDDEN_CRATE = "rustls"
EXCLUDE_FEATURES = {"rustls-tls", "default", "s3"}
SKIP_PACKAGES: set[str] = {"rattler_build_docs"}


def get_metadata() -> dict:
    result = subprocess.run(
        ["cargo", "metadata", "--no-deps", "--format-version", "1"],
        capture_output=True,
        text=True,
        check=True,
    )
    return json.loads(result.stdout)


def get_features(package: dict) -> str:
    """Get comma-separated features to enable, excluding forbidden and dep-only features."""
    features = []
    for name, deps in package.get("features", {}).items():
        if name in EXCLUDE_FEATURES:
            continue
        # Skip auto-generated features for optional deps (dep:X)
        if deps == [f"dep:{name}"]:
            continue
        features.append(name)
    return ",".join(features)


def check_package(name: str, features: str) -> tuple[bool, str]:
    """Run cargo tree to check if the forbidden crate is in the dependency tree."""
    cmd = [
        "cargo",
        "tree",
        "-i",
        FORBIDDEN_CRATE,
        "--no-default-features",
        "--package",
        name,
        "--locked",
        "--edges=normal",
    ]
    if features:
        cmd += ["--features", features]

    result = subprocess.run(cmd, capture_output=True, text=True)
    return result.stdout.startswith(FORBIDDEN_CRATE), result.stdout


def main() -> int:
    metadata = get_metadata()

    failed = 0
    checked = 0
    skipped = 0

    for package in metadata["packages"]:
        name = package["name"]

        if name in SKIP_PACKAGES:
            print(f"SKIP: {name} (known {FORBIDDEN_CRATE} dependency)")
            skipped += 1
            continue

        checked += 1
        features = get_features(package)

        has_forbidden, output = check_package(name, features)

        if has_forbidden:
            print(f"FAIL: {name} has {FORBIDDEN_CRATE} dependency")
            cmd = f'cargo tree -i {FORBIDDEN_CRATE} --no-default-features --package "{name}" --locked --edges=normal'
            if features:
                cmd += f' --features "{features}"'
            print(f"Reproduce: {cmd}")
            for line in output.splitlines()[:20]:
                print(line)
            print()
            failed += 1
        else:
            print(f"OK:   {name}")

    print()
    print(f"Summary: {checked} checked, {failed} failed, {skipped} skipped")

    if failed:
        sys.exit(1)


if __name__ == "__main__":
    main()
