"""Python bindings for PackagingConfig."""

from enum import Enum
from .rattler_build import (
    PackagingConfig as _PackagingConfig,
    ArchiveType as _ArchiveType,
)


class ArchiveType(Enum):
    """
    Archive format for conda packages.

    Attributes:
        TarBz2: Traditional .tar.bz2 format
        Conda: Modern .conda format (recommended)
    """

    TarBz2 = _ArchiveType.TarBz2
    Conda = _ArchiveType.Conda


class PackagingConfig(_PackagingConfig):
    """
    Configuration for package format and compression.

    Controls the archive format (.tar.bz2 or .conda) and compression level
    when creating conda packages.

    Examples:
        Create with defaults:
        >>> settings = PackagingConfig.conda()
        >>> settings = PackagingConfig.tar_bz2()

        Create with custom compression:
        >>> settings = PackagingConfig(ArchiveType.Conda, compression_level=15)
        >>> settings = PackagingConfig.conda(compression_level=10)

        Modify settings:
        >>> settings = PackagingConfig.conda()
        >>> settings.compression_level = 18
        >>> settings.archive_type = ArchiveType.TarBz2

        Check format:
        >>> if settings.is_conda():
        ...     print(f"Using {settings.extension()} format")
    """

    def __init__(
        self,
        archive_type: ArchiveType,
        compression_level: int | None = None,
    ) -> None:
        """
        Create a new PackagingConfig.

        Args:
            archive_type: The archive format (TarBz2 or Conda)
            compression_level: Compression level
                - For tar.bz2: 1-9 (default 9)
                - For conda: -7 to 22 (default 22)
                - Higher values = better compression but slower

        Raises:
            ValueError: If compression_level is out of valid range
        """
        ...

    @staticmethod
    def tar_bz2(compression_level: int = 9) -> "PackagingConfig":
        """
        Create PackagingConfig for tar.bz2 format.

        Args:
            compression_level: Compression level (1-9, default 9)

        Returns:
            PackagingConfig configured for tar.bz2

        Raises:
            ValueError: If compression_level is not 1-9
        """
        ...

    @staticmethod
    def conda(compression_level: int = 22) -> "PackagingConfig":
        """
        Create PackagingConfig for conda format (recommended).

        The .conda format is faster to extract and provides better compression
        than tar.bz2. It is the recommended format for new packages.

        Args:
            compression_level: Compression level (-7 to 22, default 22)
                - Negative values: faster compression, larger files
                - Positive values: slower compression, smaller files
                - 22: maximum compression (recommended)

        Returns:
            PackagingConfig configured for .conda format

        Raises:
            ValueError: If compression_level is not -7 to 22
        """
        ...

    @property
    def archive_type(self) -> ArchiveType:
        """The archive format (TarBz2 or Conda)."""
        ...

    @archive_type.setter
    def archive_type(self, value: ArchiveType) -> None:
        """Set the archive format."""
        ...

    @property
    def compression_level(self) -> int:
        """
        The compression level.

        Valid ranges:
        - tar.bz2: 1-9
        - conda: -7 to 22
        """
        ...

    @compression_level.setter
    def compression_level(self, value: int) -> None:
        """
        Set the compression level.

        Args:
            value: Compression level (validated based on archive_type)

        Raises:
            ValueError: If value is out of range for the current archive type
        """
        ...

    def extension(self) -> str:
        """
        Get the file extension for the current archive type.

        Returns:
            ".tar.bz2" or ".conda"
        """
        ...

    def is_tar_bz2(self) -> bool:
        """
        Check if this is using the tar.bz2 format.

        Returns:
            True if using tar.bz2 format
        """
        ...

    def is_conda(self) -> bool:
        """
        Check if this is using the conda format.

        Returns:
            True if using conda format
        """
        ...


__all__ = ["PackagingConfig", "ArchiveType"]
