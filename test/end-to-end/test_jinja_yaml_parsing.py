import pytest
from pathlib import Path
import tempfile
import yaml

from helpers import RattlerBuild


def test_scenario_a_int_no_quotes(rattler_build: RattlerBuild):
    """
    Test case 'a':
    var: ${{ 1234 }}
    renders to:
    var: 1234 (int)
    """
    recipe_content = """
context:
  var: ${{ 1234 }}
package:
  name: test_package
  version: 0.1.0
build:
  script: echo "hello"
"""
    with tempfile.TemporaryDirectory() as tmpdir:
        recipe_dir = Path(tmpdir)
        recipe_file = recipe_dir / "recipe.yaml"
        with open(recipe_file, "w") as f:
            f.write(recipe_content)

        output_dir = Path(tmpdir) / "output"
        output_dir.mkdir()

        rendered_recipe = rattler_build.render(recipe_dir, output_dir)
        assert isinstance(rendered_recipe, list)
        assert len(rendered_recipe) > 0
        final_context = rendered_recipe[0].get("recipe", {}).get("context", {})

        assert "var" in final_context
        assert final_context["var"] == 1234
        assert isinstance(final_context["var"], int)


def test_scenario_b_str_with_quotes_around_jinja(rattler_build: RattlerBuild):
    """
    Test case 'b':
    var: "${{ '1234' }}"
    renders to:
    var: "1234" (str)
    """
    recipe_content = """
context:
  var: "${{ '1234' }}"
package:
  name: test_package_b
  version: 0.1.0
build:
  script: echo "hello"
"""
    with tempfile.TemporaryDirectory() as tmpdir:
        recipe_dir = Path(tmpdir)
        recipe_file = recipe_dir / "recipe.yaml"
        with open(recipe_file, "w") as f:
            f.write(recipe_content)

        output_dir = Path(tmpdir) / "output"
        output_dir.mkdir()

        rendered_recipe = rattler_build.render(recipe_dir, output_dir)

        assert isinstance(rendered_recipe, list)
        assert len(rendered_recipe) > 0
        final_context = rendered_recipe[0].get("recipe", {}).get("context", {})

        assert "var" in final_context
        assert final_context["var"] == "1234"
        assert isinstance(final_context["var"], str)


def test_scenario_c_int_with_quotes_around_jinja(rattler_build: RattlerBuild):
    """
    Test case 'c':
    var: "${{ 1234 }}"
    renders to:
    var: 1234 (int)
    """
    recipe_content = """
context:
  var: "${{ 1234 }}"
package:
  name: test_package_c
  version: 0.1.0
build:
  script: echo "hello"
"""
    with tempfile.TemporaryDirectory() as tmpdir:
        recipe_dir = Path(tmpdir)
        recipe_file = recipe_dir / "recipe.yaml"
        with open(recipe_file, "w") as f:
            f.write(recipe_content)

        output_dir = Path(tmpdir) / "output"
        output_dir.mkdir()

        rendered_recipe = rattler_build.render(recipe_dir, output_dir)

        assert isinstance(rendered_recipe, list)
        assert len(rendered_recipe) > 0
        final_context = rendered_recipe[0].get("recipe", {}).get("context", {})

        assert "var" in final_context
        assert final_context["var"] == 1234
        assert isinstance(final_context["var"], int)


def test_scenario_d_str_no_quotes(rattler_build: RattlerBuild):
    """
    Test case 'd':
    var: ${{ "1234" }}
    renders to:
    var: "1234" (str)
    """
    recipe_content = """
context:
  var: ${{ "1234" }}
package:
  name: test_package_d
  version: 0.1.0
build:
  script: echo "hello"
"""
    with tempfile.TemporaryDirectory() as tmpdir:
        recipe_dir = Path(tmpdir)
        recipe_file = recipe_dir / "recipe.yaml"
        with open(recipe_file, "w") as f:
            f.write(recipe_content)

        output_dir = Path(tmpdir) / "output"
        output_dir.mkdir()

        rendered_recipe = rattler_build.render(recipe_dir, output_dir)

        assert isinstance(rendered_recipe, list)
        assert len(rendered_recipe) > 0
        final_context = rendered_recipe[0].get("recipe", {}).get("context", {})

        assert "var" in final_context
        assert final_context["var"] == "1234"
        assert isinstance(final_context["var"], str) 