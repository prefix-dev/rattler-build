"""Determine cargo build/test options for a given target and write to GITHUB_OUTPUT.

Outputs:
    cargo-build-options - extra flags for cargo build (e.g. --no-default-features --features rustls-tls)
    cargo-test-options  - extra flags for cargo test (e.g. --lib --bin rattler-build)

Usage:
    pixi run -e release build-options --target x86_64-unknown-linux-musl
"""

import argparse
import os


def main() -> None:
    parser = argparse.ArgumentParser(
        description="Determine cargo build options for a target"
    )
    parser.add_argument("--target", required=True, help="Rust target triple")
    args = parser.parse_args()

    target: str = args.target

    # musl targets need rustls instead of native TLS
    if "-musl" in target:
        build_options = "--no-default-features --features rustls-tls"
    else:
        build_options = ""

    # ARM/aarch64 targets only run lib + binary tests
    if target.startswith(("arm-", "aarch64-")):
        test_options = "--lib --bin rattler-build"
    else:
        test_options = ""

    print(f"Build options: {build_options or '(none)'}")
    print(f"Test options: {test_options or '(none)'}")

    github_output = os.environ.get("GITHUB_OUTPUT")
    if github_output:
        with open(github_output, "a") as f:
            f.write(f"cargo-build-options={build_options}\n")
            f.write(f"cargo-test-options={test_options}\n")


if __name__ == "__main__":
    main()
