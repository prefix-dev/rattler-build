"""Package builder API for rattler-build.

This module provides a Pythonic API for creating conda packages programmatically,
without needing a recipe file. Use this when you have files staged and want to
package them directly.

Example:
    ```python
    from rattler_build import assemble_package, ArchiveType
    from pathlib import Path

    # Simple package from a directory of files
    output = assemble_package(
        name="mypkg",
        version="1.0.0",
        target_platform="linux-64",
        build_string="py312_0",
        output_dir=Path("/output"),
        files_dir=Path("/staged/files"),
    )
    print(f"Created: {output.path}")
    ```
"""

from __future__ import annotations

from enum import IntEnum
from pathlib import Path
from typing import TYPE_CHECKING

from rattler_build._rattler_build import _package_assembler

if TYPE_CHECKING:
    from collections.abc import Sequence


class ArchiveType(IntEnum):
    """Archive type for conda packages."""

    TarBz2 = 0
    """Legacy .tar.bz2 format."""

    Conda = 1
    """Modern .conda format (default, recommended)."""

    def extension(self) -> str:
        """Get the file extension for this archive type."""
        if self == ArchiveType.TarBz2:
            return ".tar.bz2"
        return ".conda"


class FileEntry:
    """Represents a file to be included in the package.

    Attributes:
        source: Source path on disk.
        destination: Destination path within the package (relative).
        is_symlink: Whether this is a symlink.
        symlink_target: Symlink target (if this is a symlink).
    """

    def __init__(self, inner: _package_assembler.FileEntry) -> None:
        self._inner = inner

    @classmethod
    def from_paths(cls, source: str | Path, destination: str | Path) -> FileEntry:
        """Create a FileEntry from source and destination paths.

        Args:
            source: Source path on disk.
            destination: Destination path within the package (relative).

        Returns:
            A new FileEntry instance.
        """
        return cls(_package_assembler.FileEntry.from_paths(str(source), str(destination)))

    @property
    def source(self) -> Path:
        """Source path on disk."""
        return Path(self._inner.source)

    @property
    def destination(self) -> Path:
        """Destination path within the package."""
        return Path(self._inner.destination)

    @property
    def is_symlink(self) -> bool:
        """Whether this is a symlink."""
        return self._inner.is_symlink

    @property
    def symlink_target(self) -> Path | None:
        """Symlink target (if this is a symlink)."""
        target = self._inner.symlink_target
        return Path(target) if target else None

    def __repr__(self) -> str:
        return f"FileEntry(source='{self.source}', destination='{self.destination}')"


def collect_files(
    source_dir: str | Path,
    include_globs: Sequence[str] | None = None,
    exclude_globs: Sequence[str] | None = None,
    follow_symlinks: bool = False,
    include_hidden: bool = False,
) -> list[FileEntry]:
    """Collect files from a directory using glob patterns.

    Args:
        source_dir: Directory to scan for files.
        include_globs: Glob patterns to include (e.g., ["**/*.py", "bin/*"]).
            If not specified, all files are included by default.
        exclude_globs: Glob patterns to exclude. Exclusions take precedence
            over inclusions.
        follow_symlinks: Whether to follow symlinks when traversing (default: False).
        include_hidden: Whether to include hidden files starting with . (default: False).

    Returns:
        List of FileEntry objects for all matched files.

    Example:
        ```python
        from rattler_build import collect_files

        # Collect all Python files except tests
        files = collect_files(
            "/path/to/project",
            include_globs=["**/*.py"],
            exclude_globs=["**/test_*.py", "**/__pycache__/**"],
        )

        # Collect everything including hidden files
        files = collect_files("/path/to/project", include_hidden=True)
        ```
    """
    collector = _package_assembler.FileCollector(str(source_dir))

    if include_globs:
        for pattern in include_globs:
            collector.include_glob(pattern)

    if exclude_globs:
        for pattern in exclude_globs:
            collector.exclude_glob(pattern)

    if follow_symlinks:
        collector.set_follow_symlinks(follow_symlinks)

    if include_hidden:
        collector.set_include_hidden(include_hidden)

    return [FileEntry(f) for f in collector.collect()]


class PackageOutput:
    """Result of successful package creation.

    Attributes:
        path: Path to the created package file.
        identifier: Package identifier (name-version-build).
    """

    def __init__(self, inner: _package_assembler.PackageOutput) -> None:
        self._inner = inner

    @property
    def path(self) -> Path:
        """Path to the created package file."""
        return Path(self._inner.path)

    @property
    def identifier(self) -> str:
        """Package identifier (name-version-build)."""
        return self._inner.identifier

    def __repr__(self) -> str:
        return f"PackageOutput(path='{self.path}', identifier='{self.identifier}')"


def assemble_package(
    name: str,
    version: str,
    target_platform: str,
    build_string: str,
    output_dir: str | Path,
    *,
    # File sources (at least one required)
    files_dir: str | Path | None = None,
    files: Sequence[FileEntry] | None = None,
    # Package metadata (optional)
    homepage: str | None = None,
    license: str | None = None,
    license_family: str | None = None,
    summary: str | None = None,
    description: str | None = None,
    # Dependencies (optional)
    depends: Sequence[str] | None = None,
    constrains: Sequence[str] | None = None,
    build_number: int = 0,
    noarch: str | None = None,
    # Additional files (optional)
    license_files: Sequence[str | Path] | None = None,
    test_files: Sequence[str | Path] | None = None,
    recipe_dir: str | Path | None = None,
    # Build options (optional)
    compression_level: int = 9,
    archive_type: ArchiveType = ArchiveType.Conda,
    timestamp: int | None = None,
    compression_threads: int | None = None,
    detect_prefix: bool = True,
) -> PackageOutput:
    """Create a conda package from files and metadata.

    This is a low-level function for creating conda packages without a recipe.
    Use this when you have files staged and want to package them directly.

    Args:
        name: Package name.
        version: Package version.
        target_platform: Target platform (e.g., "linux-64", "osx-arm64", "noarch").
        build_string: Build string (e.g., "py312_0", "h1234567_0").
        output_dir: Directory where the package will be created.
        files_dir: Directory containing files to include. All files in this
            directory will be added to the package.
        files: List of FileEntry objects for explicit file mappings.
            Use this for fine-grained control over which files to include.
        homepage: Homepage URL for the package.
        license: License identifier (e.g., "MIT", "Apache-2.0").
        license_family: License family (e.g., "MIT", "GPL").
        summary: Short one-line summary of the package.
        description: Full description of the package.
        depends: List of runtime dependencies (e.g., ["python >=3.8", "numpy"]).
        constrains: List of version constraints (e.g., ["cudatoolkit >=11.0"]).
        build_number: Build number (default: 0).
        noarch: Noarch type ("python" or "generic") or None for arch-specific.
        license_files: List of paths to license files to include.
        test_files: List of paths to test files to include.
        recipe_dir: Path to recipe directory to include in info/recipe/.
        compression_level: Compression level 0-9 (default: 9, higher = smaller but slower).
        archive_type: Archive format (default: ArchiveType.Conda).
        timestamp: Build timestamp in milliseconds since epoch for reproducible builds.
        compression_threads: Number of threads for compression (default: auto-detect).
        detect_prefix: Whether to detect and record prefix placeholders (default: True).

    Returns:
        PackageOutput with path to the created package and identifier.

    Raises:
        RattlerBuildError: If package name, version, or platform is invalid,
            or if the build fails.

    Example:
        ```python
        from rattler_build import assemble_package, ArchiveType

        # Simple package
        output = assemble_package(
            name="mypackage",
            version="1.0.0",
            target_platform="linux-64",
            build_string="py312_0",
            output_dir="/output",
            files_dir="/staged/files",
        )

        # Package with metadata
        output = assemble_package(
            name="mypackage",
            version="1.0.0",
            target_platform="linux-64",
            build_string="py312_0",
            output_dir="/output",
            files_dir="/staged/files",
            homepage="https://github.com/org/mypackage",
            license="MIT",
            summary="My awesome package",
            depends=["python >=3.12", "numpy"],
        )
        ```
    """
    # Convert paths to strings
    output_dir_str = str(output_dir)
    files_dir_str = str(files_dir) if files_dir else None
    recipe_dir_str = str(recipe_dir) if recipe_dir else None

    # Convert FileEntry wrappers to inner objects
    files_inner = [f._inner for f in files] if files else None

    # Convert path sequences
    license_files_str = [str(p) for p in license_files] if license_files else None
    test_files_str = [str(p) for p in test_files] if test_files else None

    # Convert sequences to lists
    depends_list = list(depends) if depends else None
    constrains_list = list(constrains) if constrains else None

    # Map ArchiveType enum to Rust enum
    if archive_type == ArchiveType.TarBz2:
        archive_type_rust = _package_assembler.ArchiveType.TarBz2
    else:
        archive_type_rust = _package_assembler.ArchiveType.Conda

    result = _package_assembler.assemble_package_py(
        name=name,
        version=version,
        target_platform=target_platform,
        build_string=build_string,
        output_dir=output_dir_str,
        files_dir=files_dir_str,
        files=files_inner,
        homepage=homepage,
        license=license,
        license_family=license_family,
        summary=summary,
        description=description,
        depends=depends_list,
        constrains=constrains_list,
        build_number=build_number,
        noarch=noarch,
        license_files=license_files_str,
        test_files=test_files_str,
        recipe_dir=recipe_dir_str,
        compression_level=compression_level,
        archive_type=archive_type_rust,
        timestamp=timestamp,
        compression_threads=compression_threads,
        detect_prefix=detect_prefix,
    )

    return PackageOutput(result)
