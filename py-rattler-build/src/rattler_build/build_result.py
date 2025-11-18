"""Build result types for rattler-build."""

from __future__ import annotations

from pathlib import Path
from typing import TYPE_CHECKING

if TYPE_CHECKING:
    from rattler_build._rattler_build import BuildResult as BuildResultPy


class BuildResult:
    """Result of a successful package build.

    Attributes:
        packages: List of paths to built package files (.conda or .tar.bz2)
        name: Package name
        version: Package version
        build_string: Build string (hash and variant identifier)
        platform: Target platform (e.g., "linux-64", "noarch")
        variant: Dictionary of variant values used for this build
        build_time: Build duration in seconds
    """

    def __init__(
        self,
        packages: list[Path],
        name: str,
        version: str,
        build_string: str,
        platform: str,
        variant: dict[str, str],
        build_time: float,
    ):
        self.packages = packages
        self.name = name
        self.version = version
        self.build_string = build_string
        self.platform = platform
        self.variant = variant
        self.build_time = build_time

    @classmethod
    def _from_inner(cls, inner: BuildResultPy) -> BuildResult:
        """Create a BuildResult from the Rust object (internal use only)."""
        return cls(
            packages=inner.packages,
            name=inner.name,
            version=inner.version,
            build_string=inner.build_string,
            platform=inner.platform,
            variant=inner.variant,
            build_time=inner.build_time,
        )

    def __repr__(self) -> str:
        """Return a concise string representation."""
        pkg_count = len(self.packages)
        pkg_str = "package" if pkg_count == 1 else "packages"
        return (
            f"BuildResult({self.name}={self.version}={self.build_string}, "
            f"{pkg_count} {pkg_str}, platform={self.platform}, "
            f"time={self.build_time:.2f}s)"
        )
