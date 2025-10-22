"""Python bindings for TestConfig."""

from pathlib import Path
from typing import List, Optional
from .rattler_build import TestConfig as _TestConfig
from .debug import Debug


class TestConfig(_TestConfig):
    """
    Configuration for package testing.

    TestConfig controls the settings for testing conda packages,
    including test environment location, platforms, channels, and solver settings.

    This is typically created internally during test runs and exposed for
    inspection.

    Note:
        This class is read-only. Properties cannot be modified from Python.

    Examples:
        Access test configuration (from test context):
        >>> config = get_test_config()  # From test run
        >>> print(f"Testing in: {config.test_prefix}")
        >>> print(f"Target: {config.target_platform}")
        >>> print(f"Channels: {config.channels}")
        >>> if config.debug:
        ...     print("Debug mode enabled")
    """

    @property
    def test_prefix(self) -> Path:
        """
        The test prefix directory path.

        The directory where the test environment is created.

        Returns:
            Path to the test prefix directory
        """
        ...

    @property
    def target_platform(self) -> Optional[str]:
        """
        The target platform for the package.

        Returns:
            Target platform string (e.g., "linux-64"), or None if not set
        """
        ...

    @property
    def host_platform(self) -> Optional[str]:
        """
        The host platform for runtime dependencies.

        Returns:
            Host platform string, or None if not set
        """
        ...

    @property
    def current_platform(self) -> str:
        """
        The current platform running the tests.

        Returns:
            Current platform string
        """
        ...

    @property
    def keep_test_prefix(self) -> bool:
        """
        Whether to keep the test prefix after the test completes.

        If True, the test environment directory is preserved for debugging.
        If False, it's deleted after the test.

        Returns:
            True if test prefix is kept, False if deleted
        """
        ...

    @property
    def test_index(self) -> Optional[int]:
        """
        The index of the specific test to execute.

        If set, only this test will be run. If None, all tests are executed.

        Returns:
            Test index (0-based), or None for all tests
        """
        ...

    @property
    def channels(self) -> List[str]:
        """
        The channels used for resolving test dependencies.

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
    def output_dir(self) -> Path:
        """
        The output directory for test artifacts.

        Returns:
            Path to the output directory
        """
        ...

    @property
    def debug(self) -> Debug:
        """
        The debug configuration.

        Returns:
            Debug instance indicating if debug mode is enabled
        """
        ...

    @property
    def exclude_newer(self) -> Optional[str]:
        """
        Timestamp for excluding newer packages.

        Packages released after this timestamp are excluded from the solver.

        Returns:
            ISO 8601 timestamp string, or None if not set
        """
        ...


__all__ = ["TestConfig"]
