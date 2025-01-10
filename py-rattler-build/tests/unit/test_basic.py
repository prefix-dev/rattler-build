from pathlib import Path

import rattler_build
import tomli as tomllib


def test_version_match_parent_cargo() -> None:
    parent_cargo_toml = Path(__file__).parents[3].joinpath("Cargo.toml").read_text()
    parent_version = tomllib.loads(parent_cargo_toml)["package"]["version"]
    assert rattler_build.rattler_build_version() == parent_version


def test_version_match_local_cargo() -> None:
    local_cargo_toml = Path(__file__).parents[2].joinpath("Cargo.toml").read_text()
    local_version = tomllib.loads(local_cargo_toml)["package"]["version"]
    assert rattler_build.rattler_build_version() == local_version


def test_build_recipe() -> None:
    recipe_path = Path("recipe.yaml")
    rattler_build.build_recipe(recipe_path)
