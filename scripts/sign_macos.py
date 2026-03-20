"""Sign and notarize macOS binaries.

Expects the following environment variables:
    CODESIGN_CERTIFICATE          - Base64-encoded .p12 certificate
    CODESIGN_CERTIFICATE_PASSWORD - Certificate password
    CODESIGN_IDENTITY             - Signing identity
    APPLEID_USERNAME              - Apple ID for notarization
    APPLEID_PASSWORD              - App-specific password
    APPLEID_TEAMID                - Apple Developer Team ID

Usage:
    pixi run -e release sign-macos --artifacts-dir artifacts/
"""

import argparse
import base64
import os
import subprocess
import sys
import tempfile
import zipfile
from pathlib import Path

KEYCHAIN_NAME = "release-signing.keychain-db"
KEYCHAIN_PASSWORD = "release-signing-password"


def run(cmd: list[str], **kwargs) -> subprocess.CompletedProcess[str]:
    print(f"  → {' '.join(cmd)}")
    return subprocess.run(cmd, check=True, text=True, **kwargs)


def setup_keychain(cert_path: Path, cert_password: str) -> None:
    """Import the signing certificate into a temporary keychain."""
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

    # Allow codesign to access the keychain without prompt
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

    # Prepend our keychain to the search list
    result = run(
        ["security", "list-keychains", "-d", "user"],
        capture_output=True,
    )
    existing = [
        line.strip().strip('"') for line in result.stdout.splitlines() if line.strip()
    ]
    run(["security", "list-keychains", "-d", "user", "-s", KEYCHAIN_NAME, *existing])


def cleanup_keychain() -> None:
    """Remove the temporary keychain."""
    try:
        run(["security", "delete-keychain", KEYCHAIN_NAME])
    except subprocess.CalledProcessError:
        print("Warning: failed to delete keychain", file=sys.stderr)


def sign_zip(archive: Path, identity: str) -> None:
    """Extract zip, codesign the binary, re-create the zip."""
    with tempfile.TemporaryDirectory() as tmpdir:
        tmp = Path(tmpdir)

        # Extract and save original ZIP metadata (external_attr stores Unix permissions)
        original_attrs: dict[str, int] = {}
        with zipfile.ZipFile(archive, "r") as zf:
            for info in zf.infolist():
                original_attrs[info.filename] = info.external_attr
            zf.extractall(tmp)

        # Restore Unix permissions from original zip entries
        # (extractall does not reliably restore permissions across Python versions)
        for filename, attr in original_attrs.items():
            path = tmp / filename
            if path.is_file():
                mode = (attr >> 16) & 0xFFFF  # Unix permission bits from external_attr
                if mode:
                    os.chmod(path, mode)

        # Find the binary (top-level dir contains rattler-build)
        binaries = list(tmp.rglob("rattler-build"))
        if not binaries:
            print(f"  Warning: no rattler-build binary found in {archive.name}")
            return

        for binary in binaries:
            print(f"  Signing {binary.relative_to(tmp)}...")
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

        # Re-create zip (preserving Unix permissions including executable bit)
        archive.unlink()
        with zipfile.ZipFile(archive, "w", zipfile.ZIP_DEFLATED) as zf:
            for item in sorted(tmp.rglob("*")):
                if item.is_file():
                    arcname = str(item.relative_to(tmp))
                    zi = zipfile.ZipInfo(arcname)
                    zi.compress_type = zipfile.ZIP_DEFLATED
                    zi.external_attr = item.stat().st_mode << 16
                    zf.writestr(zi, item.read_bytes())


def notarize_zip(archive: Path, username: str, password: str, team_id: str) -> None:
    """Submit the zip to Apple notary service."""
    print(f"  Notarizing {archive.name}...")
    run(
        [
            "xcrun",
            "notarytool",
            "submit",
            str(archive),
            "--apple-id",
            username,
            "--password",
            password,
            "--team-id",
            team_id,
            "--wait",
        ]
    )


def main() -> None:
    parser = argparse.ArgumentParser(description="Sign and notarize macOS binaries")
    parser.add_argument("--artifacts-dir", required=True, type=Path)
    args = parser.parse_args()

    artifacts_dir: Path = args.artifacts_dir

    # Read environment variables
    cert_b64 = os.environ["CODESIGN_CERTIFICATE"]
    cert_password = os.environ["CODESIGN_CERTIFICATE_PASSWORD"]
    identity = os.environ["CODESIGN_IDENTITY"]
    apple_username = os.environ["APPLEID_USERNAME"]
    apple_password = os.environ["APPLEID_PASSWORD"]
    apple_team_id = os.environ["APPLEID_TEAMID"]

    # Find macOS zips
    archives = sorted(artifacts_dir.glob("*-apple-darwin*.zip"))
    if not archives:
        print("No macOS archives found, nothing to sign.")
        return

    print(f"Found {len(archives)} macOS archive(s) to sign.\n")

    # Write certificate to temp file
    with tempfile.NamedTemporaryFile(suffix=".p12", delete=False) as f:
        f.write(base64.b64decode(cert_b64))
        cert_path = Path(f.name)

    try:
        # Setup keychain
        print("Setting up signing keychain...")
        setup_keychain(cert_path, cert_password)

        # Sign each archive
        for archive in archives:
            print(f"\nSigning {archive.name}...")
            sign_zip(archive, identity)

        # Notarize each archive
        for archive in archives:
            notarize_zip(archive, apple_username, apple_password, apple_team_id)

    finally:
        cert_path.unlink(missing_ok=True)
        cleanup_keychain()

    print("\nAll macOS binaries signed and notarized.")


if __name__ == "__main__":
    main()
