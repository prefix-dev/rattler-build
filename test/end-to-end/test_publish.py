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
        "--output-dir",
        str(tmp_path / "output"),
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


def test_publish_with_recipe_flag(
    rattler_build: RattlerBuild, recipes: Path, tmp_path: Path
):
    """Test publishing using the --recipe flag instead of positional argument."""
    channel_dir = tmp_path / "channel"

    # Publish using --recipe flag
    rattler_build(
        "publish",
        "--recipe",
        str(recipes / "globtest"),
        "--to",
        f"file://{channel_dir}",
        "--output-dir",
        str(tmp_path / "output"),
    )

    # Check that channel was created and has packages
    assert channel_dir.exists()
    packages = list(channel_dir.glob("**/*.tar.bz2")) + list(
        channel_dir.glob("**/*.conda")
    )
    assert len(packages) > 0


def test_publish_with_recipe_dir_flag(
    rattler_build: RattlerBuild, recipes: Path, tmp_path: Path
):
    """Test publishing using the --recipe-dir flag to scan a directory for recipes."""
    channel_dir = tmp_path / "channel"
    recipe_dir = tmp_path / "recipes"

    # Create a directory with multiple recipe subdirs
    recipe_dir.mkdir()
    globtest_dir = recipe_dir / "globtest"
    globtest_dir.mkdir()

    # Copy the globtest recipe
    import shutil

    for item in (recipes / "globtest").iterdir():
        if item.is_file():
            shutil.copy(item, globtest_dir / item.name)

    # Publish using --recipe-dir flag
    rattler_build(
        "publish",
        "--recipe-dir",
        str(recipe_dir),
        "--to",
        f"file://{channel_dir}",
        "--output-dir",
        str(tmp_path / "output"),
    )

    # Check that channel was created and has packages
    assert channel_dir.exists()
    packages = list(channel_dir.glob("**/*.tar.bz2")) + list(
        channel_dir.glob("**/*.conda")
    )
    assert len(packages) > 0


def test_publish_recipe_and_recipe_dir_conflict(
    rattler_build: RattlerBuild, recipes: Path, tmp_path: Path
):
    """Test that --recipe and --recipe-dir flags conflict with each other."""
    from subprocess import STDOUT

    channel_dir = tmp_path / "channel"

    # Using both --recipe and --recipe-dir should fail
    with pytest.raises(CalledProcessError) as exc_info:
        rattler_build(
            "publish",
            "--recipe",
            str(recipes / "globtest"),
            "--recipe-dir",
            str(recipes),
            "--to",
            f"file://{channel_dir}",
            stderr=STDOUT,
        )

    # The error message should mention the conflict
    assert "cannot be used with" in str(exc_info.value.output)


# -- Helper for publish render-only ------------------------------------------

SIMPLE_RECIPE = """\
package:
  name: test-publish-override
  version: 1.0.0

build:
  number: 0
  noarch: generic
  script: echo "hello"
"""

RECIPE_WITH_BUILD_NUMBER = """\
package:
  name: test-publish-override
  version: 1.0.0

build:
  number: 5
  noarch: generic
  script: echo "hello"
"""


def _write_recipe(tmp_path: Path, content: str) -> Path:
    recipe_dir = tmp_path / "recipe"
    recipe_dir.mkdir()
    (recipe_dir / "recipe.yaml").write_text(content)
    return recipe_dir


def _publish_render(
    rattler_build: RattlerBuild,
    tmp_path: Path,
    recipe_dir: Path,
    extra_args: list[str] | None = None,
):
    """Run `publish --render-only` and return parsed JSON output."""
    channel_dir = tmp_path / "channel"
    args = [
        "publish",
        str(recipe_dir),
        "--to",
        f"file://{channel_dir}",
        "--render-only",
    ]
    if extra_args:
        args.extend(extra_args)
    output = rattler_build(*args)
    return json.loads(output)


# -- --build-number tests (absolute) -----------------------------------------


def test_publish_build_number_absolute(rattler_build: RattlerBuild, tmp_path: Path):
    """--build-number with an absolute value overrides the recipe build number."""
    recipe_dir = _write_recipe(tmp_path, RECIPE_WITH_BUILD_NUMBER)
    output = _publish_render(
        rattler_build,
        tmp_path,
        recipe_dir,
        extra_args=["--build-number", "42"],
    )

    assert len(output) == 1
    recipe = output[0]["recipe"]
    assert recipe["build"]["number"] == 42
    assert recipe["build"]["string"].endswith("_42")


def test_publish_build_number_absolute_overrides_default(
    rattler_build: RattlerBuild, tmp_path: Path
):
    """--build-number with an absolute value works when recipe uses default (0)."""
    recipe_dir = _write_recipe(tmp_path, SIMPLE_RECIPE)
    output = _publish_render(
        rattler_build,
        tmp_path,
        recipe_dir,
        extra_args=["--build-number", "7"],
    )

    recipe = output[0]["recipe"]
    assert recipe["build"]["number"] == 7
    assert recipe["build"]["string"].endswith("_7")


# -- --build-number tests (relative) -----------------------------------------


def test_publish_build_number_relative_on_empty_channel(
    rattler_build: RattlerBuild, tmp_path: Path
):
    """--build-number=+1 on an empty channel bumps from 0 to 1."""
    recipe_dir = _write_recipe(tmp_path, SIMPLE_RECIPE)
    channel_dir = tmp_path / "channel"

    output_text = rattler_build(
        "publish",
        str(recipe_dir),
        "--to",
        f"file://{channel_dir}",
        "--build-number",
        "+1",
        "--render-only",
    )
    output = json.loads(output_text)

    recipe = output[0]["recipe"]
    assert recipe["build"]["number"] == 1
    assert recipe["build"]["string"].endswith("_1")


def test_publish_build_number_relative_bumps_from_channel(
    rattler_build: RattlerBuild, recipes: Path, tmp_path: Path
):
    """--build-number=+1 bumps from the highest build number in the channel."""
    recipe_dir = _write_recipe(tmp_path, SIMPLE_RECIPE)
    channel_dir = tmp_path / "channel"

    # First publish with build number 3
    rattler_build(
        "publish",
        str(recipe_dir),
        "--to",
        f"file://{channel_dir}",
        "--build-number",
        "3",
        "--output-dir",
        str(tmp_path / "output"),
    )

    # Now render with +1 — should bump from 3 to 4
    output_text = rattler_build(
        "publish",
        str(recipe_dir),
        "--to",
        f"file://{channel_dir}",
        "--build-number",
        "+1",
        "--render-only",
    )
    output = json.loads(output_text)

    recipe = output[0]["recipe"]
    assert recipe["build"]["number"] == 4
    assert recipe["build"]["string"].endswith("_4")


# -- --build-string-prefix tests ---------------------------------------------


def test_publish_build_string_prefix(rattler_build: RattlerBuild, tmp_path: Path):
    """--build-string-prefix prepends a prefix to the build string in publish."""
    recipe_dir = _write_recipe(tmp_path, SIMPLE_RECIPE)
    output = _publish_render(
        rattler_build,
        tmp_path,
        recipe_dir,
        extra_args=["--build-string-prefix", "release"],
    )

    bs = output[0]["recipe"]["build"]["string"]
    assert bs.startswith("release_")


def test_publish_build_string_prefix_absent_gives_default(
    rattler_build: RattlerBuild, tmp_path: Path
):
    """Without --build-string-prefix the build string has no extra prefix."""
    recipe_dir = _write_recipe(tmp_path, SIMPLE_RECIPE)

    with_prefix = _publish_render(
        rattler_build,
        tmp_path,
        recipe_dir,
        extra_args=["--build-string-prefix", "pfx"],
    )
    without_prefix = _publish_render(rattler_build, tmp_path, recipe_dir)

    bs_with = with_prefix[0]["recipe"]["build"]["string"]
    bs_without = without_prefix[0]["recipe"]["build"]["string"]

    assert bs_with.startswith("pfx_")
    assert bs_with.endswith(bs_without), (
        f"prefixed '{bs_with}' should end with default '{bs_without}'"
    )


# -- combined tests -----------------------------------------------------------


def test_publish_build_number_and_prefix_combined(
    rattler_build: RattlerBuild, tmp_path: Path
):
    """--build-number and --build-string-prefix work together in publish."""
    recipe_dir = _write_recipe(tmp_path, SIMPLE_RECIPE)
    output = _publish_render(
        rattler_build,
        tmp_path,
        recipe_dir,
        extra_args=["--build-number", "10", "--build-string-prefix", "ci"],
    )

    recipe = output[0]["recipe"]
    bs = recipe["build"]["string"]
    assert recipe["build"]["number"] == 10
    assert bs.startswith("ci_")
    assert bs.endswith("_10")


def test_publish_build_number_relative_and_prefix_combined(
    rattler_build: RattlerBuild, tmp_path: Path
):
    """--build-number=+1 and --build-string-prefix work together."""
    recipe_dir = _write_recipe(tmp_path, SIMPLE_RECIPE)
    channel_dir = tmp_path / "channel"

    # First publish with build number 5
    rattler_build(
        "publish",
        str(recipe_dir),
        "--to",
        f"file://{channel_dir}",
        "--build-number",
        "5",
        "--output-dir",
        str(tmp_path / "output"),
    )

    # Render with relative bump and prefix
    output_text = rattler_build(
        "publish",
        str(recipe_dir),
        "--to",
        f"file://{channel_dir}",
        "--build-number",
        "+1",
        "--build-string-prefix",
        "nightly",
        "--render-only",
    )
    output = json.loads(output_text)

    recipe = output[0]["recipe"]
    bs = recipe["build"]["string"]
    assert recipe["build"]["number"] == 6
    assert bs.startswith("nightly_")
    assert bs.endswith("_6")
