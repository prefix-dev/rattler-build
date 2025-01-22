from pathlib import Path

import rattler_build
import tomli as tomllib
import shutil
import pytest


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


def test_test(tmp_path: Path, recipes_dir: Path) -> None:
    recipe_name = "recipe.yaml"
    recipe_path = tmp_path.joinpath(recipe_name)
    shutil.copy(recipes_dir.joinpath("dummy", recipe_name), recipe_path)
    output_dir = tmp_path.joinpath("output")
    rattler_build.build_recipes([recipe_path], output_dir=output_dir, test="skip")
    for conda_file in output_dir.glob("**/*.conda"):
        rattler_build.test_package(conda_file)
    assert output_dir.joinpath("noarch").is_dir()


def test_upload_to_quetz_no_token() -> None:
    url = "https://quetz.io"
    channel = "some_channel"
    with pytest.raises(RuntimeError, match="No quetz api key was given"):
        rattler_build.upload_package_to_quetz([], url, channel)


def test_upload_to_artifactory_no_token() -> None:
    url = "https://artifactory.io"
    channel = "some_channel"
    with pytest.raises(RuntimeError, match="No bearer token was given"):
        rattler_build.upload_package_to_artifactory([], url, channel)


def test_upload_to_prefix_no_token() -> None:
    url = "https://prefix.dev"
    channel = "some_channel"
    with pytest.raises(RuntimeError, match="No prefix.dev api key was given"):
        rattler_build.upload_package_to_prefix([], url, channel)


def test_upload_to_anaconda_no_token() -> None:
    url = "https://anaconda.org"
    with pytest.raises(RuntimeError, match="No anaconda.org api key was given"):
        rattler_build.upload_package_to_anaconda([], url)


def test_upload_packages_to_conda_forge_invalid_url() -> None:
    staging_token = "xxx"
    feedstock = "some_feedstock"
    feedstock_token = "xxx"
    anaconda_url = "invalid-url"

    with pytest.raises(RuntimeError, match="relative URL without a base"):
        rattler_build.upload_packages_to_conda_forge(
            [], staging_token, feedstock, feedstock_token, anaconda_url=anaconda_url
        )
