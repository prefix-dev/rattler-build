import warnings
from datetime import datetime
from pathlib import Path

from rattler_build._rattler_build import (
    build_recipes_py,
    test_package_py,
)


def build_recipes(
    recipes: list[str | Path],
    *,
    up_to: str | None = None,
    build_platform: str | None = None,
    target_platform: str | None = None,
    host_platform: str | None = None,
    channel: list[str] | None = None,
    variant_config: list[str] | None = None,
    variant_overrides: dict[str, list[str]] | None = None,
    ignore_recipe_variants: bool = False,
    render_only: bool = False,
    with_solve: bool = False,
    keep_build: bool = False,
    no_build_id: bool = False,
    package_format: str | None = None,
    compression_threads: int | None = None,
    io_concurrency_limit: int | None = None,
    no_include_recipe: bool = False,
    test: str | None = None,
    output_dir: str | Path | None = None,
    auth_file: str | Path | None = None,
    channel_priority: str | None = None,
    skip_existing: str | None = None,
    noarch_build_platform: str | None = None,
    allow_insecure_host: list[str] | None = None,
    continue_on_failure: bool = False,
    debug: bool = False,
    error_prefix_in_binary: bool = False,
    allow_symlinks_on_windows: bool = False,
    exclude_newer: datetime | None = None,
    build_num: int | None = None,
    use_bz2: bool = True,
    use_zstd: bool = True,
    use_jlap: bool = False,
    use_sharded: bool = True,
) -> None:
    """
    Build packages from a list of recipes.

    .. deprecated::
        This function is deprecated. Use `Recipe.from_file()` with `render()` and `run_build()` instead.

    Args:
        recipes: The recipe files or directories containing `recipe.yaml`.
        up_to: Build recipes up to the specified package.
        build_platform: The build platform to use for the build (e.g. for building with emulation, or rendering).
        target_platform: The target platform for the build.
        host_platform: The host platform for the build. If set, it will be used to determine also the target_platform (as long as it is not noarch).
        channel: Add a channel to search for dependencies in.
        variant_config: Variant configuration files for the build.
        variant_overrides: A dictionary of variant key-value pairs to override. Keys are strings, values are lists of strings.
        ignore_recipe_variants: Do not read the `variants.yaml` file next to a recipe.
        render_only: Render the recipe files without executing the build.
        with_solve: Render the recipe files with solving dependencies.
        keep_build: Keep intermediate build artifacts after the build.
        no_build_id: Don't use build id(timestamp) when creating build directory name.
        package_format: The package format to use for the build. Can be one of `tar-bz2` or `conda`. You can also add a compression level to the package format, e.g. `tar-bz2:<number>` (from 1 to 9) or `conda:<number>` (from -7 to 22).
        compression_threads: The number of threads to use for compression (only relevant when also using `--package-format conda`).
        io_concurrency_limit: The maximum number of concurrent I/O operations. This is useful for limiting the number of concurrent file operations.
        no_include_recipe: Don't store the recipe in the final package.
        test: The strategy to use for running tests.
        output_dir: The directory to store the output.
        auth_file: The authentication file.
        channel_priority: The channel priority.
        skip_existing: Whether to skip packages that already exist in any channel. If set to `none`, do not skip any packages, default when not specified. If set to `local`, only skip packages that already exist locally, default when using `--skip-existing`. If set to `all`, skip packages that already exist in any channel.
        noarch_build_platform: Define a "noarch platform" for which the noarch packages will be built for. The noarch builds will be skipped on the other platforms.
        allow_insecure_host: Allow insecure hosts for the build.
        continue_on_failure: Continue building other recipes even if one fails. (default: False)
        debug: Enable or disable debug mode. (default: False)
        error_prefix_in_binary: Do not allow the $PREFIX to appear in binary files. (default: False)
        allow_symlinks_on_windows: Allow symlinks on Windows and `noarch` packages. (default: False)
        exclude_newer: Exclude any packages that were released after the specified date when solving the build, host and test environments. (default: None)
        build_num: Override the build number for all outputs. (default: None, uses build number from recipe)
        use_bz2: Allow the use of bzip2 compression when downloading repodata. (default: True)
        use_zstd: Allow the use of zstd compression when downloading repodata. (default: True)
        use_jlap: Allow the use of jlap compression when downloading repodata. (default: False)
        use_sharded: Allow the use of sharded repodata when downloading repodata. (default: True)

    Returns:
        None
    """
    warnings.warn(
        "build_recipes is deprecated. Use Recipe.from_file() with render() and run_build() instead.",
        DeprecationWarning,
        stacklevel=2,
    )

    build_recipes_py(
        recipes,
        up_to,
        build_platform,
        target_platform,
        host_platform,
        channel,
        variant_config,
        variant_overrides,
        ignore_recipe_variants,
        render_only,
        with_solve,
        keep_build,
        no_build_id,
        package_format,
        compression_threads,
        io_concurrency_limit,
        no_include_recipe,
        test,
        output_dir,
        auth_file,
        channel_priority,
        skip_existing,
        noarch_build_platform,
        allow_insecure_host,
        continue_on_failure,
        debug,
        error_prefix_in_binary,
        allow_symlinks_on_windows,
        exclude_newer,
        build_num,
        use_bz2,
        use_zstd,
        use_jlap,
        use_sharded,
    )


def test_package(
    package_file: str | Path,
    *,
    channel: list[str] | None = None,
    compression_threads: int | None = None,
    auth_file: str | Path | None = None,
    channel_priority: str | None = None,
    allow_insecure_host: list[str] | None = None,
    debug: bool = False,
    test_index: int | None = None,
    use_bz2: bool = True,
    use_zstd: bool = True,
    use_jlap: bool = False,
    use_sharded: bool = True,
) -> None:
    """
    Run a test for a single package.

    .. deprecated::
        This function is deprecated. Use `Package.from_file()` with `run_test()` instead.

    Args:
        package_file: The package file to test.
        channel: Channels to use when testing.
        compression_threads: The number of threads to use for compression.
        auth_file: The authentication file.
        channel_priority: The channel priority.
        allow_insecure_host: Allow insecure hosts for the build.
        debug: Enable or disable debug mode. (default: False)
        test_index: The test to run, selected by index. (default: None - run all tests)
        use_bz2: Allow the use of bzip2 compression when downloading repodata. (default: True)
        use_zstd: Allow the use of zstd compression when downloading repodata. (default: True)
        use_jlap: Allow the use of jlap compression when downloading repodata. (default: False)
        use_sharded: Allow the use of sharded repodata when downloading repodata. (default: True)

    Returns:
        None
    """
    warnings.warn(
        "test_package is deprecated. Use Package.from_file() with run_test() instead.",
        DeprecationWarning,
        stacklevel=2,
    )
    test_package_py(
        package_file,
        channel,
        compression_threads,
        auth_file,
        channel_priority,
        allow_insecure_host,
        debug,
        test_index,
        use_bz2,
        use_zstd,
        use_jlap,
        use_sharded,
    )
