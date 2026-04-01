"""Test --build-num and --build-string-prefix CLI flags."""

from pathlib import Path

from helpers import RattlerBuild


SIMPLE_RECIPE = """\
package:
  name: test-override
  version: 1.0.0

build:
  number: 0
"""

RECIPE_WITH_BUILD_NUMBER = """\
package:
  name: test-override
  version: 1.0.0

build:
  number: 5
"""

RECIPE_WITH_CUSTOM_BUILD_STRING = """\
package:
  name: test-override
  version: 1.0.0

build:
  number: 0
  string: custom_${{ hash }}_${{ build_number }}
"""

RECIPE_WITH_VARIANTS = """\
package:
  name: test-override
  version: 1.0.0

build:
  number: 0

requirements:
  host:
    - python ${{ python }}
  run:
    - python
"""

VARIANT_CONFIG = """\
python:
  - "3.11.*"
  - "3.12.*"
"""


def _write_recipe(tmp_path: Path, content: str) -> Path:
    recipe_dir = tmp_path / "recipe"
    recipe_dir.mkdir()
    (recipe_dir / "recipe.yaml").write_text(content)
    return recipe_dir


def _render(
    rattler_build: RattlerBuild, tmp_path: Path, recipe_dir: Path, extra_args=None
):
    return rattler_build.render(recipe_dir, tmp_path, extra_args=extra_args)


# -- --build-num tests -------------------------------------------------------


def test_build_num_overrides_recipe(rattler_build: RattlerBuild, tmp_path: Path):
    """--build-num overrides the build number defined in the recipe."""
    recipe_dir = _write_recipe(tmp_path, RECIPE_WITH_BUILD_NUMBER)
    output = _render(
        rattler_build, tmp_path, recipe_dir, extra_args=["--build-num", "42"]
    )

    assert len(output) == 1
    recipe = output[0]["recipe"]
    assert recipe["build"]["number"] == 42
    assert recipe["build"]["string"].endswith("_42")


def test_build_num_overrides_default(rattler_build: RattlerBuild, tmp_path: Path):
    """--build-num works when the recipe uses the default build number (0)."""
    recipe_dir = _write_recipe(tmp_path, SIMPLE_RECIPE)
    output = _render(
        rattler_build, tmp_path, recipe_dir, extra_args=["--build-num", "7"]
    )

    recipe = output[0]["recipe"]
    assert recipe["build"]["number"] == 7
    assert recipe["build"]["string"].endswith("_7")


def test_build_num_with_custom_build_string(
    rattler_build: RattlerBuild, tmp_path: Path
):
    """--build-num works with a custom build string template."""
    recipe_dir = _write_recipe(tmp_path, RECIPE_WITH_CUSTOM_BUILD_STRING)
    output = _render(
        rattler_build, tmp_path, recipe_dir, extra_args=["--build-num", "99"]
    )

    recipe = output[0]["recipe"]
    assert recipe["build"]["number"] == 99
    assert recipe["build"]["string"].startswith("custom_")
    assert recipe["build"]["string"].endswith("_99")


def test_build_num_applied_to_all_variants(rattler_build: RattlerBuild, tmp_path: Path):
    """--build-num is applied consistently to every variant."""
    recipe_dir = _write_recipe(tmp_path, RECIPE_WITH_VARIANTS)
    variant_config = tmp_path / "variants.yaml"
    variant_config.write_text(VARIANT_CONFIG)

    output = rattler_build.render(
        recipe_dir,
        tmp_path,
        variant_config=variant_config,
        extra_args=["--build-num", "3"],
    )

    assert len(output) == 2
    for variant_output in output:
        assert variant_output["recipe"]["build"]["number"] == 3
        assert variant_output["recipe"]["build"]["string"].endswith("_3")


# -- --build-string-prefix tests ---------------------------------------------


def test_build_string_prefix(rattler_build: RattlerBuild, tmp_path: Path):
    """--build-string-prefix prepends a prefix to the default build string."""
    recipe_dir = _write_recipe(tmp_path, SIMPLE_RECIPE)
    output = _render(
        rattler_build,
        tmp_path,
        recipe_dir,
        extra_args=["--build-string-prefix", "myprefix"],
    )

    build_string = output[0]["recipe"]["build"]["string"]
    assert build_string.startswith("myprefix_")


def test_build_string_prefix_with_variants(rattler_build: RattlerBuild, tmp_path: Path):
    """--build-string-prefix is applied to all variants and they remain distinct."""
    recipe_dir = _write_recipe(tmp_path, RECIPE_WITH_VARIANTS)
    variant_config = tmp_path / "variants.yaml"
    variant_config.write_text(VARIANT_CONFIG)

    output = rattler_build.render(
        recipe_dir,
        tmp_path,
        variant_config=variant_config,
        extra_args=["--build-string-prefix", "ci"],
    )

    assert len(output) == 2
    build_strings = set()
    for variant_output in output:
        bs = variant_output["recipe"]["build"]["string"]
        assert bs.startswith("ci_"), f"expected prefix 'ci_', got '{bs}'"
        build_strings.add(bs)

    assert len(build_strings) == 2, "variants should produce distinct build strings"


def test_build_string_prefix_absent_gives_default(
    rattler_build: RattlerBuild, tmp_path: Path
):
    """Without --build-string-prefix the build string has no extra prefix."""
    recipe_dir = _write_recipe(tmp_path, SIMPLE_RECIPE)

    with_prefix = _render(
        rattler_build,
        tmp_path,
        recipe_dir,
        extra_args=["--build-string-prefix", "pfx"],
    )
    without_prefix = _render(rattler_build, tmp_path, recipe_dir)

    bs_with = with_prefix[0]["recipe"]["build"]["string"]
    bs_without = without_prefix[0]["recipe"]["build"]["string"]

    assert bs_with.startswith("pfx_")
    assert bs_with.endswith(bs_without), (
        f"prefixed '{bs_with}' should end with default '{bs_without}'"
    )


# -- combined tests -----------------------------------------------------------


def test_build_num_and_prefix_combined(rattler_build: RattlerBuild, tmp_path: Path):
    """--build-num and --build-string-prefix work together."""
    recipe_dir = _write_recipe(tmp_path, SIMPLE_RECIPE)
    output = _render(
        rattler_build,
        tmp_path,
        recipe_dir,
        extra_args=["--build-num", "10", "--build-string-prefix", "release"],
    )

    recipe = output[0]["recipe"]
    bs = recipe["build"]["string"]
    assert recipe["build"]["number"] == 10
    assert bs.startswith("release_")
    assert bs.endswith("_10")


def test_build_num_and_prefix_combined_with_variants(
    rattler_build: RattlerBuild, tmp_path: Path
):
    """Both flags applied to every variant."""
    recipe_dir = _write_recipe(tmp_path, RECIPE_WITH_VARIANTS)
    variant_config = tmp_path / "variants.yaml"
    variant_config.write_text(VARIANT_CONFIG)

    output = rattler_build.render(
        recipe_dir,
        tmp_path,
        variant_config=variant_config,
        extra_args=["--build-num", "5", "--build-string-prefix", "nightly"],
    )

    assert len(output) == 2
    for variant_output in output:
        bs = variant_output["recipe"]["build"]["string"]
        assert bs.startswith("nightly_"), f"expected prefix 'nightly_', got '{bs}'"
        assert bs.endswith("_5"), f"expected suffix '_5', got '{bs}'"
        assert variant_output["recipe"]["build"]["number"] == 5
