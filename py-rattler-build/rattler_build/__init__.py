from .rattler_build import get_rattler_build_version_py, build_recipes_py
from pathlib import Path
from typing import Union

__all__ = ["rattler_build_version", "build_recipe"]


def rattler_build_version() -> str:
    return get_rattler_build_version_py()


def build_recipe(recipe_path: Union[str, Path], output_dir: Union[str, Path, None]) -> None:
    output_dir = None if output_dir is None else str(output_dir)
    recipes = [str(recipe_path)]
    build_recipes_py(recipes, output_dir)
