from .rattler_build import (
    get_rattler_build_version_py,
    build_recipes_py,
    test_package_py,
    upload_package_to_quetz_py,
    upload_package_to_artifactory_py,
    upload_package_to_prefix_py,
    upload_package_to_anaconda_py,
    upload_packages_to_conda_forge_py,
)


from pathlib import Path
from typing import List, Union

__all__ = [
    "rattler_build_version",
    "build_recipes",
    "test_package",
    "upload_package_to_quetz",
    "upload_package_to_artifactory",
    "upload_package_to_prefix",
    "upload_package_to_anaconda",
    "upload_packages_to_conda_forge",
]


def rattler_build_version() -> str:
    """Get the version of the rattler-build package"""
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
    io_concurrency_limit: Union[int, None] = None,
    no_include_recipe: bool = False,
    test: Union[str, None] = None,
    output_dir: Union[str, Path, None] = None,
    auth_file: Union[str, Path, None] = None,
    channel_priority: Union[str, None] = None,
    skip_existing: Union[str, None] = None,
    noarch_build_platform: Union[str, None] = None,
    allow_insecure_host: Union[List[str], None] = None,
    continue_on_failure: bool = False,
    debug: bool = False,
) -> None:
    """
    Build packages from a list of recipes.

    Args:
        recipes: The recipe files or directories containing `recipe.yaml`.
        up_to: Build recipes up to the specified package.
        build_platform: The build platform to use for the build (e.g. for building with emulation, or rendering).
        target_platform: The target platform for the build.
        host_platform: The host platform for the build. If set, it will be used to determine also the target_platform (as long as it is not noarch).
        channel: Add a channel to search for dependencies in.
        variant_config: Variant configuration files for the build.
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

    Returns:
        None
    """

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
    )


def test_package(
    package_file: Union[str, Path],
    channel: Union[List[str], None] = None,
    compression_threads: Union[int, None] = None,
    auth_file: Union[str, Path, None] = None,
    channel_priority: Union[str, None] = None,
    allow_insecure_host: Union[List[str], None] = None,
    debug: bool = False,
    test_index: Union[int, None] = None,
) -> None:
    """
    Run a test for a single package.

    Args:
        package_file: The package file to test.
        channel: Channels to use when testing.
        compression_threads: The number of threads to use for compression.
        auth_file: The authentication file.
        channel_priority: The channel priority.
        allow_insecure_host: Allow insecure hosts for the build.
        debug: Enable or disable debug mode. (default: False)
        test_index: The test to run, selected by index. (default: None - run all tests)

    Returns:
        None
    """
    test_package_py(
        package_file, channel, compression_threads, auth_file, channel_priority, allow_insecure_host, debug, test_index
    )


def upload_package_to_quetz(
    package_files: List[str],
    url: str,
    channels: str,
    api_key: Union[str, None] = None,
    auth_file: Union[str, Path, None] = None,
) -> None:
    """
    Upload to a Quetz server. Authentication is used from the keychain / auth-file.

    Args:
        package_files: The package files to upload.
        url: The URL of the Quetz server.
        channels: The channels to upload the package to.
        api_key: The API key for authentication.
        auth_file: The authentication file.

    Returns:
        None
    """
    upload_package_to_quetz_py(package_files, url, channels, api_key, auth_file)


def upload_package_to_artifactory(
    package_files: List[str],
    url: str,
    channels: str,
    token: Union[str, None] = None,
    auth_file: Union[str, Path, None] = None,
) -> None:
    """
    Upload to an Artifactory channel. Authentication is used from the keychain / auth-file.

    Args:
        package_files: The package files to upload.
        url: The URL to your Artifactory server.
        channels: The URL to your channel.
        token: Your Artifactory token.
        auth_file: The authentication file.

    Returns:
        None
    """
    upload_package_to_artifactory_py(package_files, url, channels, token, auth_file)


def upload_package_to_prefix(
    package_files: List[str],
    url: str,
    channels: str,
    api_key: Union[str, None] = None,
    auth_file: Union[str, Path, None] = None,
    skip_existing: bool = False,
) -> None:
    """
    Upload to a prefix.dev server. Authentication is used from the keychain / auth-file.

    Args:
        package_files: The package files to upload.
        url: The URL to the prefix.dev server (only necessary for self-hosted instances).
        channels: The channel to upload the package to.
        api_key: The prefix.dev API key, if none is provided, the token is read from the keychain / auth-file.
        auth_file: The authentication file.
        skip_existing: Skip upload if package is existed.

    Returns:
        None
    """
    upload_package_to_prefix_py(package_files, url, channels, api_key, auth_file, skip_existing)


def upload_package_to_anaconda(
    package_files: List[str],
    owner: str,
    channel: Union[List[str], None] = None,
    api_key: Union[str, None] = None,
    url: Union[str, None] = None,
    force: bool = False,
    auth_file: Union[str, Path, None] = None,
) -> None:
    """
    Upload to an Anaconda.org server.

    Args:
        package_files: The package files to upload.
        owner: The owner of the Anaconda.org account.
        channel: The channels to upload the package to.
        api_key: The Anaconda.org API key.
        url: The URL to the Anaconda.org server.
        force: Whether to force the upload.
        auth_file: The authentication file.

    Returns:
        None
    """
    upload_package_to_anaconda_py(package_files, owner, channel, api_key, url, force, auth_file)


def upload_packages_to_conda_forge(
    package_files: List[Union[str, Path]],
    staging_token: str,
    feedstock: str,
    feedstock_token: str,
    staging_channel: Union[str, None] = None,
    anaconda_url: Union[str, None] = None,
    validation_endpoint: Union[str, None] = None,
    provider: Union[str, None] = None,
    dry_run: bool = False,
) -> None:
    """
    Upload to conda forge.

    Args:
        package_files: The package files to upload.
        staging_token: The staging token for conda forge.
        feedstock: The feedstock repository.
        feedstock_token: The feedstock token.
        staging_channel: The staging channel for the upload.
        anaconda_url: The URL to the Anaconda.org server.
        validation_endpoint: The validation endpoint.
        provider: The provider for the upload.
        dry_run: Whether to perform a dry run.

    Returns:
        None
    """
    upload_packages_to_conda_forge_py(
        package_files,
        staging_token,
        feedstock,
        feedstock_token,
        staging_channel,
        anaconda_url,
        validation_endpoint,
        provider,
        dry_run,
    )
