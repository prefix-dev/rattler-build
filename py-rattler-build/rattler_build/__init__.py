from .rattler_build import get_rattler_build_version_py, build_recipes_py, test_package_py, upload_package_to_quetz_py
from pathlib import Path
from typing import List, Union

__all__ = ["rattler_build_version", "build_recipe", "test_package", "upload_package_to_quetz"]


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


def test_package(
    package_file: Union[str, Path],
    channel: Union[List[str], None] = None,
    compression_threads: Union[int, None] = None,
    auth_file: Union[str, Path, None] = None,
    channel_priority: Union[str, None] = None,
) -> None:
    package_file = str(package_file)
    auth_file = auth_file if auth_file is None else str(auth_file)
    test_package_py(package_file, channel, compression_threads, auth_file, channel_priority)


def upload_package_to_quetz(
    package_files: List[str],
    url: str,
    channels: str,
    api_key: Union[str, None] = None,
    auth_file: Union[str, Path, None] = None,
) -> None:
    upload_package_to_quetz_py(package_files, url, channels, api_key, auth_file)
