"""Tests for the DebugSession API."""

from __future__ import annotations

from pathlib import Path

from rattler_build import Stage0Recipe, VariantConfig
from rattler_build.debug import DebugSession


def test_debug_session_resolves_patches_relative_to_recipe_dir(tmp_path: Path) -> None:
    """Regression test for https://github.com/prefix-dev/rattler-build/issues/2501.

    ``DebugSession.create`` must forward the recipe path of the rendered
    variant to the Rust binding, otherwise patches are looked up in a
    fabricated ``<output_dir>/_no_recipe`` directory and the setup fails
    with ``Patch file not found``.
    """
    recipe_dir = tmp_path / "recipe"
    recipe_dir.mkdir()

    source_dir = recipe_dir / "src"
    source_dir.mkdir()
    (source_dir / "somefile").write_text("abc\n")

    (recipe_dir / "fix.patch").write_text(
        "diff --git a/somefile b/somefile\n"
        "index 8baef1b..cd470e6 100644\n"
        "--- a/somefile\n"
        "+++ b/somefile\n"
        "@@ -1 +1 @@\n"
        "-abc\n"
        "+xyz\n"
    )

    (recipe_dir / "recipe.yaml").write_text(
        """
package:
  name: debug-patch-test
  version: 1.0.0

source:
  path: ./src
  patches:
    - fix.patch

build:
  noarch: generic
  script:
    - if: unix
      then:
        - cp somefile $PREFIX/somefile
      else:
        - copy somefile %PREFIX%\\somefile
""".lstrip()
    )

    recipe = Stage0Recipe.from_file(recipe_dir / "recipe.yaml")
    rendered = recipe.render(VariantConfig())

    session = DebugSession.create(
        variant=rendered[0],
        output_dir=tmp_path / "output",
    )

    assert session.paths.recipe_dir.resolve() == recipe_dir.resolve()
