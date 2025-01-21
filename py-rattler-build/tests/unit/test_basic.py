from pathlib import Path

import rattler_build
import tomli as tomllib
import shutil


def test_version_match_parent_cargo() -> None:
    parent_cargo_toml = Path(__file__).parents[3].joinpath("Cargo.toml").read_text()
    parent_version = tomllib.loads(parent_cargo_toml)["package"]["version"]
    assert rattler_build.rattler_build_version() == parent_version


def test_version_match_local_cargo() -> None:
    local_cargo_toml = Path(__file__).parents[2].joinpath("Cargo.toml").read_text()
    local_version = tomllib.loads(local_cargo_toml)["package"]["version"]
    assert rattler_build.rattler_build_version() == local_version


def test_build_recipe(tmp_path: Path, recipes_dir: Path) -> None:
    recipe_name = "recipe.yaml"
    recipe_path = tmp_path.joinpath(recipe_name)
    shutil.copy(recipes_dir.joinpath("dummy", recipe_name), recipe_path)
    output_dir = tmp_path.joinpath("output")
    rattler_build.build_recipes([recipe_path], output_dir=output_dir)
    assert output_dir.joinpath("noarch").is_dir()
