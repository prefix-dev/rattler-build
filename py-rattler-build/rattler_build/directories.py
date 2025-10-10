"""Python bindings for Directories."""

from pathlib import Path
from .rattler_build import Directories as _Directories


class Directories(_Directories):
    """
    Directory structure used during package builds.

    Represents the various paths and directories used during the conda package
    build process, including recipe, cache, work, host and build directories.

    Note:
        This class is typically created internally during the build process.
        The properties are read-only from Python.

    Examples:
        Access directory information (from a build context):
        >>> dirs = get_build_directories()  # From a build
        >>> print(f"Recipe: {dirs.recipe_dir}")
        >>> print(f"Work: {dirs.work_dir}")
        >>> print(f"Host prefix: {dirs.host_prefix}")
        >>> print(f"Output: {dirs.output_dir}")
    """

    @property
    def recipe_dir(self) -> Path:
        """
        The directory where the recipe is located.

        Returns:
            Path to the recipe directory
        """
        ...

    @property
    def recipe_path(self) -> Path:
        """
        The path to the recipe file itself.

        Returns:
            Path to the recipe file
        """
        ...

    @property
    def cache_dir(self) -> Path:
        """
        The folder where the build cache is located.

        Returns:
            Path to the cache directory
        """
        ...

    @property
    def host_prefix(self) -> Path:
        """
        The directory where host dependencies are installed.

        This is exposed as $PREFIX (or %PREFIX% on Windows) in the build script.

        Returns:
            Path to the host prefix directory
        """
        ...

    @property
    def build_prefix(self) -> Path:
        """
        The directory where build dependencies are installed.

        This is exposed as $BUILD_PREFIX (or %BUILD_PREFIX% on Windows) in the
        build script.

        Returns:
            Path to the build prefix directory
        """
        ...

    @property
    def work_dir(self) -> Path:
        """
        The directory where the source code is copied to and built from.

        Returns:
            Path to the work directory
        """
        ...

    @property
    def build_dir(self) -> Path:
        """
        The parent directory of host, build and work directories.

        Returns:
            Path to the build directory
        """
        ...

    @property
    def output_dir(self) -> Path:
        """
        The output directory or local channel directory where packages are written.

        Returns:
            Path to the output directory
        """
        ...


__all__ = ["Directories"]
