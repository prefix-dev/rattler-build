from .rattler_build import get_rattler_build_version_py, build_recipes_py, test_py
from pathlib import Path
from typing import List, Union

__all__ = ["rattler_build_version", "build_recipe"]


def rattler_build_version() -> str:
    return get_rattler_build_version_py()


def build_recipes(
    recipes: List[Union[str, Path]],
    up_to: Union[str, None] = None,
    build_platform: Union[str, None] = None,
    target_platform: Union[str, None] = None,
    host_platform: Union[str, None] = None,
    channel: Union[List[str], None] = None,
    variant_config: Union[List[str], None] = None,
    ignore_recipe_variants: bool = False,
    render_only: bool = False,
    with_solve: bool = False,
    keep_build: bool = False,
    no_build_id: bool = False,
    package_format: Union[str, None] = None,
    compression_threads: Union[int, None] = None,
    no_include_recipe: bool = False,
    test: Union[str, None] = None,
    output_dir: Union[str, Path, None] = None,
    auth_file: Union[str, Path, None] = None,
    channel_priority: Union[str, None] = None,
    skip_existing: Union[str, None] = None,
    noarch_build_platform: Union[str, None] = None,
) -> None:
    recipes = [str(recipe) for recipe in recipes]
    output_dir = output_dir if output_dir is None else str(output_dir)
    auth_file = auth_file if auth_file is None else str(auth_file)
    build_recipes_py(
        recipes,
        up_to,
        build_platform,
        target_platform,
        host_platform,
        channel,
        variant_config,
        ignore_recipe_variants,
        render_only,
        with_solve,
        keep_build,
        no_build_id,
        package_format,
        compression_threads,
        no_include_recipe,
        test,
        output_dir,
        auth_file,
        channel_priority,
        skip_existing,
        noarch_build_platform,
    )


def test(
    package_file: Union[str, Path],
    channel: Union[List[str], None] = None,
    compression_threads: Union[int, None] = None,
    auth_file: Union[str, Path, None] = None,
    channel_priority: Union[str, None] = None,
):
    package_file = package_file if package_file is None else str(package_file)
    auth_file = auth_file if auth_file is None else str(auth_file)
    test_py(package_file, channel, compression_threads, auth_file, channel_priority)
