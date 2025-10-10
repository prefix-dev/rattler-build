"""Python bindings for SandboxConfig."""

from pathlib import Path
from typing import List, Optional
from .rattler_build import SandboxConfig as _SandboxConfig


class SandboxConfig(_SandboxConfig):
    """
    Configuration for build sandboxing and isolation.

    Controls network access and filesystem permissions during package builds.

    Examples:
        Create a basic sandbox configuration:
        >>> config = SandboxConfig(
        ...     allow_network=False,
        ...     read=["/usr", "/etc"],
        ...     read_execute=["/bin", "/usr/bin"],
        ...     read_write=["/tmp"]
        ... )

        Use platform defaults:
        >>> macos_config = SandboxConfig.for_macos()
        >>> linux_config = SandboxConfig.for_linux()

        Modify permissions:
        >>> config = SandboxConfig.for_linux()
        >>> config.allow_network = True
        >>> config.add_read_write(Path("/my/custom/path"))
    """

    def __init__(
        self,
        allow_network: bool = False,
        read: Optional[List[Path]] = None,
        read_execute: Optional[List[Path]] = None,
        read_write: Optional[List[Path]] = None,
    ) -> None:
        """
        Create a new SandboxConfiguration.

        Args:
            allow_network: Whether to allow network access during the build
            read: List of paths that can be read
            read_execute: List of paths that can be read and executed
            read_write: List of paths that can be read and written
        """
        ...

    @property
    def allow_network(self) -> bool:
        """Whether network access is allowed."""
        ...

    @allow_network.setter
    def allow_network(self, value: bool) -> None:
        """Set whether network access is allowed."""
        ...

    @property
    def read(self) -> List[Path]:
        """List of read-only paths."""
        ...

    @read.setter
    def read(self, value: List[Path]) -> None:
        """Set the list of read-only paths."""
        ...

    @property
    def read_execute(self) -> List[Path]:
        """List of read-execute paths."""
        ...

    @read_execute.setter
    def read_execute(self, value: List[Path]) -> None:
        """Set the list of read-execute paths."""
        ...

    @property
    def read_write(self) -> List[Path]:
        """List of read-write paths."""
        ...

    @read_write.setter
    def read_write(self, value: List[Path]) -> None:
        """Set the list of read-write paths."""
        ...

    @staticmethod
    def for_macos() -> "SandboxConfig":
        """
        Create a default sandbox configuration for macOS.

        This configuration includes:
        - Network access: disabled
        - Read access: entire filesystem
        - Read-execute: /bin, /usr/bin
        - Read-write: /tmp, /var/tmp, $TMPDIR

        Returns:
            A SandboxConfig configured for macOS
        """
        ...

    @staticmethod
    def for_linux() -> "SandboxConfig":
        """
        Create a default sandbox configuration for Linux.

        This configuration includes:
        - Network access: disabled
        - Read access: entire filesystem
        - Read-execute: /bin, /usr/bin, /lib*, /usr/lib*
        - Read-write: /tmp, /dev/shm, $TMPDIR

        Returns:
            A SandboxConfig configured for Linux
        """
        ...

    def add_read(self, path: Path) -> None:
        """
        Add a path to the read-only list.

        Args:
            path: Path to add to the read-only list
        """
        ...

    def add_read_execute(self, path: Path) -> None:
        """
        Add a path to the read-execute list.

        Args:
            path: Path to add to the read-execute list
        """
        ...

    def add_read_write(self, path: Path) -> None:
        """
        Add a path to the read-write list.

        Args:
            path: Path to add to the read-write list
        """
        ...


__all__ = ["SandboxConfig"]
