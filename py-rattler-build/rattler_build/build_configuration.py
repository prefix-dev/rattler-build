"""Python bindings for BuildConfig."""

from typing import Any, Dict, List, Optional
from .rattler_build import BuildConfig as _BuildConfig
from .debug import Debug
from .directories import Directories
from .packaging_settings import PackagingConfig
from .sandbox_config import SandboxConfig


class BuildConfig(_BuildConfig):
    """
    Complete configuration for building a package.

    BuildConfig contains all settings needed to build a conda package,
    including platforms, variants, channels, directories, and build settings.

    This is typically created internally during the build process and exposed
    for inspection.

    Note:
        This class is read-only. Properties cannot be modified from Python.

    Examples:
        Access build configuration (from build context):
        >>> config = get_build_config()  # From build context
        >>> print(f"Target: {config.target_platform}")
        >>> print(f"Hash: {config.hash}")
        >>> if config.cross_compilation():
        ...     print("Cross-compiling!")
        >>> print(f"Channels: {config.channels}")
    """

    @property
    def target_platform(self) -> str:
        """
        The target platform for the build.

        The platform for which the package is being built.

        Returns:
            Target platform string (e.g., "linux-64", "osx-arm64")
        """
        ...

    @property
    def host_platform(self) -> Dict[str, Any]:
        """
        The host platform with virtual packages.

        The platform where the package will run (usually same as target,
        but different for noarch packages).

        Returns:
            Dictionary with 'platform' (str) and 'virtual_packages' (list) keys
        """
        ...

    @property
    def build_platform(self) -> Dict[str, Any]:
        """
        The build platform with virtual packages.

        The platform on which the build is running.

        Returns:
            Dictionary with 'platform' (str) and 'virtual_packages' (list) keys
        """
        ...

    @property
    def variant(self) -> Dict[str, Any]:
        """
        The variant configuration for this build.

        The selected variant (e.g., python version, numpy version).

        Returns:
            Dictionary mapping variant keys to their values
        """
        ...

    @property
    def hash(self) -> str:
        """
        The computed hash of the variant configuration.

        Returns:
            Hash string (e.g., "h1234567_0")
        """
        ...

    @property
    def directories(self) -> Directories:
        """
        The build directories.

        Returns:
            Directories instance with all build paths
        """
        ...

    @property
    def channels(self) -> List[str]:
        """
        The channels used for resolving dependencies.

        Returns:
            List of channel URLs as strings
        """
        ...

    @property
    def channel_priority(self) -> str:
        """
        The channel priority strategy.

        Returns:
            Channel priority as a string (e.g., "Strict", "Flexible")
        """
        ...

    @property
    def solve_strategy(self) -> str:
        """
        The solver strategy for resolving dependencies.

        Returns:
            Solve strategy as a string
        """
        ...

    @property
    def timestamp(self) -> str:
        """
        The build timestamp.

        Returns:
            ISO 8601 timestamp string
        """
        ...

    @property
    def subpackages(self) -> Dict[str, Dict[str, Any]]:
        """
        All subpackages from this output or other outputs from the same recipe.

        Returns:
            Dictionary mapping package names to their identifiers
        """
        ...

    @property
    def packaging_settings(self) -> PackagingConfig:
        """
        The packaging settings for this build.

        Returns:
            PackagingConfig instance
        """
        ...

    @property
    def store_recipe(self) -> bool:
        """
        Whether the recipe should be stored in the package.

        Returns:
            True if recipe is stored, False otherwise
        """
        ...

    @property
    def force_colors(self) -> bool:
        """
        Whether colors are forced in build script output.

        Returns:
            True if colors are forced
        """
        ...

    @property
    def sandbox_config(self) -> Optional[SandboxConfig]:
        """
        The sandbox configuration for this build.

        Returns:
            SandboxConfig instance, or None if not configured
        """
        ...

    @property
    def debug(self) -> Debug:
        """
        The debug configuration.

        Returns:
            Debug instance
        """
        ...

    @property
    def exclude_newer(self) -> Optional[str]:
        """
        Timestamp for excluding newer packages.

        Packages newer than this date are excluded from the solver.

        Returns:
            ISO 8601 timestamp string, or None if not set
        """
        ...

    def cross_compilation(self) -> bool:
        """
        Check if this is a cross-compilation build.

        Returns:
            True if target platform differs from build platform
        """
        ...

    def target_platform_name(self) -> str:
        """
        Get the target platform name only (without virtual packages).

        Returns:
            Platform string
        """
        ...

    def host_platform_name(self) -> str:
        """
        Get the host platform name only (without virtual packages).

        Returns:
            Platform string
        """
        ...

    def build_platform_name(self) -> str:
        """
        Get the build platform name only (without virtual packages).

        Returns:
            Platform string
        """
        ...


__all__ = ["BuildConfig"]
