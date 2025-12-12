"""
Tool configuration for rattler-build.

This module provides a Pythonic API for configuring the build tool.
"""

from rattler_build._rattler_build import tool_config as _tool_config


class PlatformConfig:
    """Platform configuration for building packages.

    This class provides platform settings that are shared across different
    configuration objects (JinjaConfig, RenderConfig).

    Args:
        target_platform: Target platform (e.g., "linux-64", "osx-arm64").
            If not specified, defaults to the current platform.
        build_platform: Build platform (where the build runs).
            If not specified, defaults to the target platform.
        host_platform: Host platform (for cross-compilation).
            If not specified, defaults to the target platform.
        experimental: Enable experimental features
        recipe_path: Path to the recipe file (for relative path resolution)

    Example:
        ```python
        # Create with default (current) platform
        config = PlatformConfig()

        # Create for a specific platform (build and host will default to target)
        config = PlatformConfig("linux-64")

        # Create with different platforms for cross-compilation
        config = PlatformConfig(
            target_platform="osx-arm64",
            build_platform="linux-64",
            host_platform="osx-arm64"
        )
        ```
    """

    def __init__(
        self,
        *,
        target_platform: str | None = None,
        build_platform: str | None = None,
        host_platform: str | None = None,
        experimental: bool = False,
        recipe_path: str | None = None,
    ):
        """Create a new platform configuration."""
        self.target_platform = target_platform
        self.build_platform = build_platform if build_platform is not None else None
        self.host_platform = host_platform if host_platform is not None else None
        self.experimental = experimental
        self.recipe_path = recipe_path

    def __repr__(self) -> str:
        return (
            f"PlatformConfig(target_platform={self.target_platform!r}, "
            f"build_platform={self.build_platform!r}, "
            f"host_platform={self.host_platform!r}, "
            f"experimental={self.experimental})"
        )


class ToolConfiguration:
    """Configuration for the rattler-build tool.

    Args:
        keep_build: Whether to keep the build directory after the build is done
        compression_threads: Number of threads to use for compression (default: None - auto)
        io_concurrency_limit: Maximum number of concurrent I/O operations (default: None)
        test_strategy: Test strategy to use ("skip", "native", or "tests") (default: "skip")
        skip_existing: Whether to skip packages that already exist ("none", "local", or "all") (default: "none")
        continue_on_failure: Whether to continue building other recipes even if one fails (default: False)
        noarch_build_platform: Platform to use for noarch builds (default: None)
        channel_priority: Channel priority for solving ("strict" or "disabled") (default: "strict")
        allow_insecure_host: List of hosts for which SSL certificate verification should be skipped
        error_prefix_in_binary: Whether to error if the host prefix is detected in binary files (default: False)
        allow_symlinks_on_windows: Whether to allow symlinks in packages on Windows (default: False)
        use_zstd: Whether to use zstd compression when downloading repodata (default: True)
        use_bz2: Whether to use bzip2 compression when downloading repodata (default: True)
        use_sharded: Whether to use sharded repodata when downloading (default: True)
        use_jlap: Whether to use JLAP when downloading repodata (default: False)

    Example:
        ```python
        config = ToolConfiguration(
            keep_build=True,
            test_strategy="native",
            compression_threads=4
        )
        print(config.keep_build)
        # True
        print(config.test_strategy)
        # Native
        ```
    """

    def __init__(
        self,
        *,
        keep_build: bool = False,
        compression_threads: int | None = None,
        io_concurrency_limit: int | None = None,
        test_strategy: str | None = None,
        skip_existing: str | None = None,
        continue_on_failure: bool = False,
        noarch_build_platform: str | None = None,
        channel_priority: str | None = None,
        allow_insecure_host: list[str] | None = None,
        error_prefix_in_binary: bool = False,
        allow_symlinks_on_windows: bool = False,
        use_zstd: bool = True,
        use_bz2: bool = True,
        use_sharded: bool = True,
        use_jlap: bool = False,
    ):
        """Create a new tool configuration."""
        self._inner = _tool_config.ToolConfiguration(
            keep_build=keep_build,
            compression_threads=compression_threads,
            io_concurrency_limit=io_concurrency_limit,
            test_strategy=test_strategy,
            skip_existing=skip_existing,
            continue_on_failure=continue_on_failure,
            noarch_build_platform=noarch_build_platform,
            channel_priority=channel_priority,
            allow_insecure_host=allow_insecure_host,
            error_prefix_in_binary=error_prefix_in_binary,
            allow_symlinks_on_windows=allow_symlinks_on_windows,
            use_zstd=use_zstd,
            use_bz2=use_bz2,
            use_sharded=use_sharded,
            use_jlap=use_jlap,
        )

    @property
    def keep_build(self) -> bool:
        """Whether to keep the build directory after the build is done."""
        return self._inner.keep_build

    @property
    def test_strategy(self) -> str:
        """The test strategy to use."""
        return self._inner.test_strategy

    @property
    def skip_existing(self) -> str:
        """Whether to skip existing packages."""
        return self._inner.skip_existing

    @property
    def continue_on_failure(self) -> bool:
        """Whether to continue building on failure."""
        return self._inner.continue_on_failure

    @property
    def channel_priority(self) -> str:
        """The channel priority to use in solving."""
        return self._inner.channel_priority

    @property
    def use_zstd(self) -> bool:
        """Whether to use zstd compression."""
        return self._inner.use_zstd

    @property
    def use_bz2(self) -> bool:
        """Whether to use bzip2 compression."""
        return self._inner.use_bz2

    @property
    def use_sharded(self) -> bool:
        """Whether to use sharded repodata."""
        return self._inner.use_sharded

    @property
    def use_jlap(self) -> bool:
        """Whether to use JLAP."""
        return self._inner.use_jlap

    @property
    def compression_threads(self) -> int | None:
        """Number of compression threads."""
        return self._inner.compression_threads

    @property
    def io_concurrency_limit(self) -> int | None:
        """IO concurrency limit."""
        return self._inner.io_concurrency_limit

    @property
    def allow_insecure_host(self) -> list[str] | None:
        """List of hosts for which SSL certificate verification should be skipped."""
        return self._inner.allow_insecure_host

    @property
    def error_prefix_in_binary(self) -> bool:
        """Whether to error if the host prefix is detected in binary files."""
        return self._inner.error_prefix_in_binary

    @property
    def allow_symlinks_on_windows(self) -> bool:
        """Whether to allow symlinks in packages on Windows."""
        return self._inner.allow_symlinks_on_windows

    def __repr__(self) -> str:
        return repr(self._inner)
