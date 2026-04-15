"""Codesign a macOS binary in place.

Expects the following environment variables:
    CODESIGN_CERTIFICATE          - Base64-encoded .p12 certificate
    CODESIGN_CERTIFICATE_PASSWORD - Certificate password
    CODESIGN_IDENTITY             - Signing identity

Usage:
    pixi run -e release sign-macos --binary target/aarch64-apple-darwin/release/rattler-build
"""

import argparse
import base64
import os
import subprocess
import sys
import tempfile
from pathlib import Path

KEYCHAIN_NAME = "release-signing.keychain-db"
KEYCHAIN_PASSWORD = "release-signing-password"


def run(cmd: list[str], **kwargs) -> subprocess.CompletedProcess[str]:
    print(f"  -> {' '.join(cmd)}")
    return subprocess.run(cmd, check=True, text=True, **kwargs)


def setup_keychain(cert_path: Path, cert_password: str) -> None:
    run(["security", "create-keychain", "-p", KEYCHAIN_PASSWORD, KEYCHAIN_NAME])
    run(["security", "set-keychain-settings", "-lut", "21600", KEYCHAIN_NAME])
    run(["security", "unlock-keychain", "-p", KEYCHAIN_PASSWORD, KEYCHAIN_NAME])

    run(
        [
            "security",
            "import",
            str(cert_path),
            "-k",
            KEYCHAIN_NAME,
            "-P",
            cert_password,
            "-T",
            "/usr/bin/codesign",
        ]
    )

    run(
        [
            "security",
            "set-key-partition-list",
            "-S",
            "apple-tool:,apple:,codesign:",
            "-s",
            "-k",
            KEYCHAIN_PASSWORD,
            KEYCHAIN_NAME,
        ]
    )

    result = run(
        ["security", "list-keychains", "-d", "user"],
        capture_output=True,
    )
    existing = [
        line.strip().strip('"') for line in result.stdout.splitlines() if line.strip()
    ]
    run(["security", "list-keychains", "-d", "user", "-s", KEYCHAIN_NAME, *existing])


def cleanup_keychain() -> None:
    try:
        run(["security", "delete-keychain", KEYCHAIN_NAME])
    except subprocess.CalledProcessError:
        print("Warning: failed to delete keychain", file=sys.stderr)


def codesign(binary: Path, identity: str) -> None:
    print(f"\nSigning {binary}...")
    run(
        [
            "codesign",
            "--force",
            "--options",
            "runtime",
            "--sign",
            identity,
            str(binary),
        ]
    )


def main() -> None:
    parser = argparse.ArgumentParser(description="Codesign a macOS binary")
    parser.add_argument("--binary", required=True, type=Path)
    args = parser.parse_args()

    binary: Path = args.binary
    if not binary.is_file():
        print(f"Error: {binary} is not a file", file=sys.stderr)
        sys.exit(1)

    cert_b64 = os.environ["CODESIGN_CERTIFICATE"]
    cert_password = os.environ["CODESIGN_CERTIFICATE_PASSWORD"]
    identity = os.environ["CODESIGN_IDENTITY"]

    with tempfile.NamedTemporaryFile(suffix=".p12", delete=False) as f:
        f.write(base64.b64decode(cert_b64))
        cert_path = Path(f.name)

    try:
        print("Setting up signing keychain...")
        setup_keychain(cert_path, cert_password)
        codesign(binary, identity)
    finally:
        cert_path.unlink(missing_ok=True)
        cleanup_keychain()

    print(f"\nSigned {binary}")


if __name__ == "__main__":
    main()
