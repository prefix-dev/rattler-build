from pathlib import Path

from rattler_build.render import render_recipes

RECIPE = """
package:
  name: mypkg
  version: "1.0"
build:
  number: 3
requirements:
  host:
    - python ${{ python }}.*
"""


def test_render_recipes_returns_cli_shaped_outputs(tmp_path: Path) -> None:
    (tmp_path / "recipe.yaml").write_text(RECIPE)

    outputs = render_recipes(
        [tmp_path],
        target_platform="linux-64",
        build_platform="linux-64",
        variant_overrides={"python": ["3.12"]},
    )

    assert len(outputs) == 1
    output = outputs[0]
    assert output["recipe"]["package"]["name"] == "mypkg"
    assert output["recipe"]["build"]["number"] == 3
    assert output["recipe"]["build"]["string"].endswith("_3")
    assert output["build_configuration"]["variant"]["target_platform"] == "linux-64"
    assert output["build_configuration"]["variant"]["python"] == "3.12"
    assert "mypkg" in output["build_configuration"]["subpackages"]


def test_render_recipes_expands_variants(tmp_path: Path) -> None:
    (tmp_path / "recipe.yaml").write_text(RECIPE)
    (tmp_path / "variants.yaml").write_text("python:\n  - '3.12'\n  - '3.13'\n")

    outputs = render_recipes([tmp_path], target_platform="linux-64", build_platform="linux-64")

    pythons = sorted(output["build_configuration"]["variant"]["python"] for output in outputs)
    assert pythons == ["3.12", "3.13"]


def test_render_recipes_filters_skipped_outputs(tmp_path: Path) -> None:
    (tmp_path / "recipe.yaml").write_text(
        """
package:
  name: mypkg
  version: "1.0"
build:
  number: 0
  skip: true
"""
    )

    outputs = render_recipes([tmp_path], target_platform="linux-64", build_platform="linux-64")
    assert outputs == []
