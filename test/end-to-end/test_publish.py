"""Tests for the publish command."""

import json
from pathlib import Path
from subprocess import CalledProcessError

import pytest
from helpers import RattlerBuild, get_package


def test_publish_to_new_local_channel(
    rattler_build: RattlerBuild, recipes: Path, tmp_path: Path
):
    """Test publishing to a new local channel that doesn't exist yet.

    The publish command should automatically initialize the channel with
    an empty noarch/repodata.json.
    """
    output_dir = tmp_path / "output"
    channel_dir = tmp_path / "channel"

    # Build a simple package first
    rattler_build.build(recipes / "globtest", output_dir)
    package = get_package(output_dir, "globtest")

    # Channel doesn't exist yet
    assert not channel_dir.exists()

    # Publish to the new channel - should auto-initialize
    rattler_build(
        "publish",
        str(package),
        "--to",
        f"file://{channel_dir}",
    )

    # Check that channel was created and initialized
    assert channel_dir.exists()
    noarch_repodata = channel_dir / "noarch" / "repodata.json"
    assert noarch_repodata.exists()

    # Check that package was uploaded to the correct subdir
    # The package subdir is determined from the package itself
    repodata_files = list(channel_dir.glob("*/repodata.json"))
    assert len(repodata_files) >= 1  # At least noarch

    # Find the subdir where the package was uploaded
    package_found = False
    for repodata_file in repodata_files:
        packages_in_subdir = list(repodata_file.parent.glob("*.tar.bz2")) + list(
            repodata_file.parent.glob("*.conda")
        )
        if packages_in_subdir:
            package_found = True
            # Verify repodata.json contains the package
            repodata = json.loads(repodata_file.read_text())
            assert "packages" in repodata or "packages.conda" in repodata
            break

    assert package_found, "Package was not found in any subdir"


def test_publish_to_existing_local_channel(
    rattler_build: RattlerBuild, recipes: Path, tmp_path: Path
):
    """Test publishing to an existing initialized local channel."""
    output_dir = tmp_path / "output"
    channel_dir = tmp_path / "channel"

    # Build a simple package first
    rattler_build.build(recipes / "globtest", output_dir)
    package = get_package(output_dir, "globtest")

    # Pre-initialize the channel
    noarch_dir = channel_dir / "noarch"
    noarch_dir.mkdir(parents=True)
    (noarch_dir / "repodata.json").write_text('{"packages": {}, "packages.conda": {}}')

    # Publish to the existing channel
    rattler_build(
        "publish",
        str(package),
        "--to",
        f"file://{channel_dir}",
    )

    # Check that package was uploaded
    package_found = any(list(channel_dir.glob(f"*/{package.name}"))) or any(
        p.name == package.name for p in channel_dir.rglob("*.tar.bz2")
    )
    assert package_found


def test_publish_to_uninitialized_existing_channel_fails(
    rattler_build: RattlerBuild, recipes: Path, tmp_path: Path
):
    """Test that publishing to an existing but uninitialized channel fails with a helpful error."""
    from subprocess import STDOUT

    output_dir = tmp_path / "output"
    channel_dir = tmp_path / "channel"

    # Build a simple package first
    rattler_build.build(recipes / "globtest", output_dir)
    package = get_package(output_dir, "globtest")

    # Create channel dir but don't initialize it (no noarch/repodata.json)
    channel_dir.mkdir(parents=True)

    # Publish should fail with a helpful error
    with pytest.raises(CalledProcessError) as exc_info:
        rattler_build(
            "publish",
            str(package),
            "--to",
            f"file://{channel_dir}",
            stderr=STDOUT,
        )

    # The error message should mention that the channel is not initialized
    assert (
        "not initialized" in str(exc_info.value.output).lower()
        or "missing" in str(exc_info.value.output).lower()
    )


def test_publish_recipe_to_local_channel(
    rattler_build: RattlerBuild, recipes: Path, tmp_path: Path
):
    """Test publishing directly from a recipe to a local channel."""
    channel_dir = tmp_path / "channel"

    # Publish directly from recipe - should build and upload
    rattler_build(
        "publish",
        str(recipes / "globtest"),
        "--to",
        f"file://{channel_dir}",
    )

    # Check that channel was created and has packages
    assert channel_dir.exists()
    noarch_repodata = channel_dir / "noarch" / "repodata.json"
    assert noarch_repodata.exists()

    # Find packages in any subdir
    packages = list(channel_dir.glob("**/*.tar.bz2")) + list(
        channel_dir.glob("**/*.conda")
    )
    assert len(packages) > 0


def test_publish_with_force_overwrites(
    rattler_build: RattlerBuild, recipes: Path, tmp_path: Path
):
    """Test that --force allows overwriting existing packages."""
    output_dir = tmp_path / "output"
    channel_dir = tmp_path / "channel"

    # Build a simple package
    rattler_build.build(recipes / "globtest", output_dir)
    package = get_package(output_dir, "globtest")

    # Publish first time
    rattler_build(
        "publish",
        str(package),
        "--to",
        f"file://{channel_dir}",
    )

    # Publishing again without --force should fail
    with pytest.raises(CalledProcessError):
        rattler_build(
            "publish",
            str(package),
            "--to",
            f"file://{channel_dir}",
        )

    # Publishing with --force should succeed
    rattler_build(
        "publish",
        str(package),
        "--to",
        f"file://{channel_dir}",
        "--force",
    )


def test_publish_with_path_syntax(
    rattler_build: RattlerBuild, recipes: Path, tmp_path: Path
):
    """Test publishing using path syntax instead of file:// URL."""
    output_dir = tmp_path / "output"
    channel_dir = tmp_path / "channel"

    # Build a simple package first
    rattler_build.build(recipes / "globtest", output_dir)
    package = get_package(output_dir, "globtest")

    # Publish using path syntax (not file:// URL)
    rattler_build(
        "publish",
        str(package),
        "--to",
        str(channel_dir),
    )

    # Check that channel was created
    assert channel_dir.exists()
    noarch_repodata = channel_dir / "noarch" / "repodata.json"
    assert noarch_repodata.exists()
