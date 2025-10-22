"""Python bindings for Debug."""

from .rattler_build import Debug as _Debug


class Debug(_Debug):
    """
    Control debug output during builds.

    Debug is a simple wrapper around a boolean that enables or disables
    debug output during package builds.

    Examples:
        Create with debug enabled:
        >>> debug = Debug(True)
        >>> assert debug.is_enabled()

        Create with debug disabled:
        >>> debug = Debug(False)
        >>> assert not debug.is_enabled()

        Use factory methods:
        >>> debug = Debug.enabled()
        >>> debug = Debug.disabled()

        Toggle debug mode:
        >>> debug = Debug(False)
        >>> debug.enable()
        >>> assert debug.is_enabled()
        >>> debug.toggle()
        >>> assert not debug.is_enabled()

        Use as boolean:
        >>> debug = Debug(True)
        >>> if debug:
        ...     print("Debug is enabled")
    """

    def __init__(self, enabled: bool = False) -> None:
        """
        Create a new Debug instance.

        Args:
            enabled: Whether debug output is enabled (default: False)
        """
        ...

    @staticmethod
    def enabled() -> "Debug":
        """
        Create a Debug instance with debug enabled.

        Returns:
            Debug instance with debug enabled
        """
        ...

    @staticmethod
    def disabled() -> "Debug":
        """
        Create a Debug instance with debug disabled.

        Returns:
            Debug instance with debug disabled
        """
        ...

    def is_enabled(self) -> bool:
        """
        Check if debug output is enabled.

        Returns:
            True if debug output is enabled, False otherwise
        """
        ...

    def set_enabled(self, enabled: bool) -> None:
        """
        Set whether debug output is enabled.

        Args:
            enabled: Whether to enable debug output
        """
        ...

    def enable(self) -> None:
        """Enable debug output."""
        ...

    def disable(self) -> None:
        """Disable debug output."""
        ...

    def toggle(self) -> None:
        """Toggle debug output (enabled ↔ disabled)."""
        ...

    def __bool__(self) -> bool:
        """
        Boolean conversion.

        Returns:
            True if debug is enabled, False otherwise
        """
        ...


__all__ = ["Debug"]
