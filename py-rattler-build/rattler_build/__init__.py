from .rattler_build import get_rattler_build_version_py
from .rattler_build import build_recipes_py
from pathlib import Path
from typing import Union

__all__ = ["rattler_build_version", "build_recipe"]


def rattler_build_version() -> str:
    return get_rattler_build_version_py()


def build_recipe(recipe_path: Union[str, Path]) -> None:
    build_recipes_py([str(recipe_path)])
