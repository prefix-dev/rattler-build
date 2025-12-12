"""Package inspection and testing API for rattler-build.

This module provides functionality to load and inspect conda packages (.conda or .tar.bz2),
examine their metadata and embedded tests, and run tests against them.

Example:
    ```python
    from rattler_build import Package
    from rattler_build.package import PythonTest, CommandsTest, PackageContentsTest

    pkg = Package.from_file("mypackage-1.0.0-py312_0.conda")
    print(pkg.name, pkg.version)
    # mypackage 1.0.0

    print(pkg.depends)
    # ['python >=3.12', 'numpy >=1.20']

    # Pattern match on test types (Python 3.10+)
    for test in pkg.tests:
        match test:
            case PythonTest() as py_test:
                print(f"Python test {py_test.index}: imports={py_test.imports}")
            case CommandsTest() as cmd_test:
                print(f"Commands test {cmd_test.index}")
            case PackageContentsTest() as pc_test:
                print(f"Package contents test {pc_test.index}: strict={pc_test.strict}")

    results = pkg.run_tests(channel=["conda-forge"])
    for r in results:
        print(f"Test {r.test_index}: {'PASS' if r.success else 'FAIL'}")
    ```
"""

from pathlib import Path
from typing import TYPE_CHECKING, Any, Union

from rattler_build._rattler_build import _package

if TYPE_CHECKING:
    from collections.abc import Sequence

# Type alias for test union
PackageTestType = Union[
    "PythonTest",
    "CommandsTest",
    "PerlTest",
    "RTest",
    "RubyTest",
    "DownstreamTest",
    "PackageContentsTest",
]


class Package:
    """A loaded conda package for inspection and testing.

    This class provides access to package metadata, file contents, and embedded tests.
    The package is lazily extracted when needed (e.g., when accessing files or tests).

    Attributes:
        name: Package name (e.g., "numpy")
        version: Package version (e.g., "1.26.0")
        build_string: Build string (e.g., "py312_0")
        build_number: Build number
        subdir: Target platform subdirectory (e.g., "linux-64", "noarch")
        noarch: NoArch type (None, "python", or "generic")
        depends: List of runtime dependencies
        constrains: List of dependency constraints
        license: Package license
        license_family: License family
        timestamp: Build timestamp in milliseconds since epoch
        arch: Architecture (e.g., "x86_64")
        platform: Platform (e.g., "linux")
        path: Path to the package file
        archive_type: Archive format ("conda" or "tar.bz2")
        filename: Filename of the package
        files: List of all files in the package
        tests: List of tests embedded in the package
    """

    def __init__(self, inner: _package.Package) -> None:
        """Initialize from internal Package object.

        Users should use Package.from_file() instead of this constructor.
        """
        self._inner = inner

    @classmethod
    def from_file(cls, path: str | Path) -> "Package":
        """Load a package from a .conda or .tar.bz2 file.

        Args:
            path: Path to the package file

        Returns:
            A Package object for inspection and testing

        Raises:
            RattlerBuildError: If the package cannot be loaded or parsed

        Example:
            ```python
            pkg = Package.from_file("numpy-1.26.0-py312_0.conda")
            print(pkg.name)
            # numpy
            ```
        """
        return cls(_package.Package.from_file(str(path)))

    @property
    def name(self) -> str:
        """Package name."""
        return self._inner.name

    @property
    def version(self) -> str:
        """Package version."""
        return self._inner.version

    @property
    def build_string(self) -> str:
        """Build string (e.g., "py312_0")."""
        return self._inner.build_string

    @property
    def build_number(self) -> int:
        """Build number."""
        return self._inner.build_number

    @property
    def subdir(self) -> str | None:
        """Target platform subdirectory (e.g., "linux-64", "noarch")."""
        return self._inner.subdir

    @property
    def noarch(self) -> str | None:
        """NoArch type (None, "python", or "generic")."""
        return self._inner.noarch

    @property
    def depends(self) -> list[str]:
        """List of runtime dependencies."""
        return self._inner.depends

    @property
    def constrains(self) -> list[str]:
        """List of dependency constraints."""
        return self._inner.constrains

    @property
    def license(self) -> str | None:
        """Package license."""
        return self._inner.license

    @property
    def license_family(self) -> str | None:
        """License family."""
        return self._inner.license_family

    @property
    def timestamp(self) -> int | None:
        """Build timestamp in milliseconds since epoch."""
        return self._inner.timestamp

    @property
    def arch(self) -> str | None:
        """Architecture (e.g., "x86_64")."""
        return self._inner.arch

    @property
    def platform(self) -> str | None:
        """Platform (e.g., "linux")."""
        return self._inner.platform

    @property
    def path(self) -> Path:
        """Path to the package file."""
        return Path(self._inner.path)

    @property
    def archive_type(self) -> str:
        """Archive format ("conda" or "tar.bz2")."""
        return self._inner.archive_type

    @property
    def filename(self) -> str:
        """Filename of the package (e.g., "numpy-1.26.0-py312_0.conda")."""
        return self._inner.filename

    @property
    def files(self) -> list[str]:
        """List of all files in the package."""
        return self._inner.files

    @property
    def tests(self) -> list[PackageTestType]:
        """List of tests embedded in the package.

        Returns a list of test objects that can be pattern matched:

        ```python
        for test in pkg.tests:
            match test:
                case PythonTest() as py_test:
                    print(f"imports: {py_test.imports}")
                case CommandsTest() as cmd_test:
                    print(f"script: {cmd_test.script}")
                case PackageContentsTest() as pc_test:
                    print(f"strict: {pc_test.strict}")
        ```
        """
        return [_wrap_test(t) for t in self._inner.tests]

    def test_count(self) -> int:
        """Get the number of tests in the package."""
        return self._inner.test_count()

    def run_test(
        self,
        index: int,
        *,
        channel: "Sequence[str] | None" = None,
        channel_priority: str | None = None,
        debug: bool = False,
        auth_file: str | Path | None = None,
        allow_insecure_host: "Sequence[str] | None" = None,
        compression_threads: int | None = None,
        use_bz2: bool = True,
        use_zstd: bool = True,
        use_jlap: bool = False,
        use_sharded: bool = True,
    ) -> "TestResult":
        """Run a specific test by index.

        Args:
            index: Index of the test to run (0-based)
            channel: List of channels to use for dependencies
            channel_priority: Channel priority ("disabled", "strict", or "flexible")
            debug: Enable debug mode (keeps test environment)
            auth_file: Path to authentication file
            allow_insecure_host: List of hosts to allow insecure connections
            compression_threads: Number of compression threads
            use_bz2: Enable bz2 repodata
            use_zstd: Enable zstd repodata
            use_jlap: Enable JLAP incremental repodata
            use_sharded: Enable sharded repodata

        Returns:
            TestResult with success status and output

        Raises:
            RattlerBuildError: If the test index is out of range or test execution fails

        Example:
            ```python
            result = pkg.run_test(0, channel=["conda-forge"])
            if result.success:
                print("Test passed!")
            ```
        """
        return TestResult(
            self._inner.run_test(
                index,
                channel=list(channel) if channel else None,
                channel_priority=channel_priority,
                debug=debug,
                auth_file=str(auth_file) if auth_file else None,
                allow_insecure_host=list(allow_insecure_host) if allow_insecure_host else None,
                compression_threads=compression_threads,
                use_bz2=use_bz2,
                use_zstd=use_zstd,
                use_jlap=use_jlap,
                use_sharded=use_sharded,
            )
        )

    def run_tests(
        self,
        *,
        channel: "Sequence[str] | None" = None,
        channel_priority: str | None = None,
        debug: bool = False,
        auth_file: str | Path | None = None,
        allow_insecure_host: "Sequence[str] | None" = None,
        compression_threads: int | None = None,
        use_bz2: bool = True,
        use_zstd: bool = True,
        use_jlap: bool = False,
        use_sharded: bool = True,
    ) -> list["TestResult"]:
        """Run all tests in the package.

        Args:
            channel: List of channels to use for dependencies
            channel_priority: Channel priority ("disabled", "strict", or "flexible")
            debug: Enable debug mode (keeps test environment)
            auth_file: Path to authentication file
            allow_insecure_host: List of hosts to allow insecure connections
            compression_threads: Number of compression threads
            use_bz2: Enable bz2 repodata
            use_zstd: Enable zstd repodata
            use_jlap: Enable JLAP incremental repodata
            use_sharded: Enable sharded repodata

        Returns:
            List of TestResult objects, one per test

        Example:
            ```python
            results = pkg.run_tests(channel=["conda-forge"])
            for r in results:
                status = "PASS" if r.success else "FAIL"
                print(f"Test {r.test_index}: {status}")
            ```
        """
        return [
            TestResult(r)
            for r in self._inner.run_tests(
                channel=list(channel) if channel else None,
                channel_priority=channel_priority,
                debug=debug,
                auth_file=str(auth_file) if auth_file else None,
                allow_insecure_host=list(allow_insecure_host) if allow_insecure_host else None,
                compression_threads=compression_threads,
                use_bz2=use_bz2,
                use_zstd=use_zstd,
                use_jlap=use_jlap,
                use_sharded=use_sharded,
            )
        ]

    def rebuild(
        self,
        *,
        test: str | None = None,
        compression_threads: int | None = None,
        output_dir: str | Path | None = None,
        auth_file: str | Path | None = None,
        allow_insecure_host: "Sequence[str] | None" = None,
        use_bz2: bool = True,
        use_zstd: bool = True,
        use_jlap: bool = False,
        use_sharded: bool = True,
    ) -> "RebuildResult":
        """Rebuild this package from its embedded recipe.

        Extracts the recipe embedded in the package and rebuilds it,
        then compares SHA256 hashes to verify reproducibility.

        Args:
            test: Test strategy ("skip", "native", "native-and-emulated").
                  Defaults to "native".
            compression_threads: Number of compression threads
            output_dir: Output directory for rebuilt package. Defaults to
                        current working directory.
            auth_file: Path to authentication file
            allow_insecure_host: List of hosts to allow insecure connections
            use_bz2: Enable bz2 repodata (default: True)
            use_zstd: Enable zstd repodata (default: True)
            use_jlap: Enable JLAP incremental repodata (default: False)
            use_sharded: Enable sharded repodata (default: True)

        Returns:
            RebuildResult with original/rebuilt paths, SHA256 hashes,
            and the rebuilt Package for inspection

        Example:
            ```python
            from rattler_build import Package

            pkg = Package.from_file("mypackage-1.0.0-py312_0.conda")
            result = pkg.rebuild(test="skip")

            if result.is_identical:
                print("Build is reproducible!")
            else:
                print(f"Original: {result.original_sha256}")
                print(f"Rebuilt:  {result.rebuilt_sha256}")

            # Inspect the rebuilt package
            rebuilt = result.rebuilt_package
            print(f"Rebuilt: {rebuilt.name}-{rebuilt.version}")
            ```
        """
        return RebuildResult(
            self._inner.rebuild(
                test=test,
                compression_threads=compression_threads,
                output_dir=str(output_dir) if output_dir else None,
                auth_file=str(auth_file) if auth_file else None,
                allow_insecure_host=list(allow_insecure_host) if allow_insecure_host else None,
                use_bz2=use_bz2,
                use_zstd=use_zstd,
                use_jlap=use_jlap,
                use_sharded=use_sharded,
            )
        )

    def to_dict(self) -> dict[str, Any]:
        """Convert package metadata to a dictionary."""
        return self._inner.to_dict()

    def __repr__(self) -> str:
        return f"Package({self.name}-{self.version}-{self.build_string})"


def _wrap_test(inner: Any) -> PackageTestType:
    """Wrap a raw test object in the appropriate Python wrapper class."""
    if isinstance(inner, _package.PythonTest):
        return PythonTest(inner)
    elif isinstance(inner, _package.CommandsTest):
        return CommandsTest(inner)
    elif isinstance(inner, _package.PerlTest):
        return PerlTest(inner)
    elif isinstance(inner, _package.RTest):
        return RTest(inner)
    elif isinstance(inner, _package.RubyTest):
        return RubyTest(inner)
    elif isinstance(inner, _package.DownstreamTest):
        return DownstreamTest(inner)
    elif isinstance(inner, _package.PackageContentsTest):
        return PackageContentsTest(inner)
    else:
        raise TypeError(f"Unknown test type: {type(inner)}")


class PythonTest:
    """Python test - imports modules and optionally runs pip check.

    Attributes:
        index: Index of this test in the package's test list
        imports: List of modules to import
        pip_check: Whether to run pip check (default: True)
        python_version: Python version specification
    """

    def __init__(self, inner: _package.PythonTest) -> None:
        self._inner = inner

    @property
    def index(self) -> int:
        """Index of this test in the package's test list."""
        return self._inner.index

    @property
    def imports(self) -> list[str]:
        """List of modules to import."""
        return self._inner.imports

    @property
    def pip_check(self) -> bool:
        """Whether to run pip check (default: True)."""
        return self._inner.pip_check

    @property
    def python_version(self) -> "PythonVersion | None":
        """Python version specification."""
        inner = self._inner.python_version
        return PythonVersion(inner) if inner else None

    def to_dict(self) -> dict[str, Any]:
        """Convert to a dictionary."""
        return self._inner.to_dict()

    def __repr__(self) -> str:
        return f"PythonTest(imports={self.imports!r}, pip_check={self.pip_check})"


class PythonVersion:
    """Python version specification for tests.

    Can be a single version, multiple versions, or unspecified (None).
    """

    def __init__(self, inner: _package.PythonVersion) -> None:
        self._inner = inner

    def as_single(self) -> str | None:
        """Get the version as a single string (if single version)."""
        return self._inner.as_single()

    def as_multiple(self) -> list[str] | None:
        """Get the versions as a list (if multiple versions)."""
        return self._inner.as_multiple()

    def is_none(self) -> bool:
        """Check if no specific version is set."""
        return self._inner.is_none()

    def __repr__(self) -> str:
        if self.is_none():
            return "PythonVersion(None)"
        single = self.as_single()
        if single:
            return f"PythonVersion('{single}')"
        multiple = self.as_multiple()
        return f"PythonVersion({multiple!r})"


class CommandsTest:
    """Commands test - runs arbitrary shell commands.

    Attributes:
        index: Index of this test in the package's test list
        script: The script content (as dict)
        requirements_run: Extra runtime requirements for the test
        requirements_build: Extra build requirements for the test (e.g., emulators)
    """

    def __init__(self, inner: _package.CommandsTest) -> None:
        self._inner = inner

    @property
    def index(self) -> int:
        """Index of this test in the package's test list."""
        return self._inner.index

    @property
    def script(self) -> dict[str, Any]:
        """The script content."""
        return self._inner.script

    @property
    def requirements_run(self) -> list[str]:
        """Extra runtime requirements for the test."""
        return self._inner.requirements_run

    @property
    def requirements_build(self) -> list[str]:
        """Extra build requirements for the test (e.g., emulators)."""
        return self._inner.requirements_build

    def to_dict(self) -> dict[str, Any]:
        """Convert to a dictionary."""
        return self._inner.to_dict()

    def __repr__(self) -> str:
        return "CommandsTest(...)"


class PerlTest:
    """Perl test - tests Perl modules.

    Attributes:
        index: Index of this test in the package's test list
        uses: List of Perl modules to load with 'use'
    """

    def __init__(self, inner: _package.PerlTest) -> None:
        self._inner = inner

    @property
    def index(self) -> int:
        """Index of this test in the package's test list."""
        return self._inner.index

    @property
    def uses(self) -> list[str]:
        """List of Perl modules to load with 'use'."""
        return self._inner.uses

    def to_dict(self) -> dict[str, Any]:
        """Convert to a dictionary."""
        return self._inner.to_dict()

    def __repr__(self) -> str:
        return f"PerlTest(uses={self.uses!r})"


class RTest:
    """R test - tests R libraries.

    Attributes:
        index: Index of this test in the package's test list
        libraries: List of R libraries to load with library()
    """

    def __init__(self, inner: _package.RTest) -> None:
        self._inner = inner

    @property
    def index(self) -> int:
        """Index of this test in the package's test list."""
        return self._inner.index

    @property
    def libraries(self) -> list[str]:
        """List of R libraries to load with library()."""
        return self._inner.libraries

    def to_dict(self) -> dict[str, Any]:
        """Convert to a dictionary."""
        return self._inner.to_dict()

    def __repr__(self) -> str:
        return f"RTest(libraries={self.libraries!r})"


class RubyTest:
    """Ruby test - tests Ruby modules.

    Attributes:
        index: Index of this test in the package's test list
        requires: List of Ruby modules to require
    """

    def __init__(self, inner: _package.RubyTest) -> None:
        self._inner = inner

    @property
    def index(self) -> int:
        """Index of this test in the package's test list."""
        return self._inner.index

    @property
    def requires(self) -> list[str]:
        """List of Ruby modules to require."""
        return self._inner.requires

    def to_dict(self) -> dict[str, Any]:
        """Convert to a dictionary."""
        return self._inner.to_dict()

    def __repr__(self) -> str:
        return f"RubyTest(requires={self.requires!r})"


class DownstreamTest:
    """Downstream test - tests a downstream package that depends on this package.

    Attributes:
        index: Index of this test in the package's test list
        downstream: Name of the downstream package to test
    """

    def __init__(self, inner: _package.DownstreamTest) -> None:
        self._inner = inner

    @property
    def index(self) -> int:
        """Index of this test in the package's test list."""
        return self._inner.index

    @property
    def downstream(self) -> str:
        """Name of the downstream package to test."""
        return self._inner.downstream

    def to_dict(self) -> dict[str, Any]:
        """Convert to a dictionary."""
        return self._inner.to_dict()

    def __repr__(self) -> str:
        return f"DownstreamTest(downstream='{self.downstream}')"


class PackageContentsTest:
    """Package contents test - checks that files exist or don't exist in the package.

    Attributes:
        index: Index of this test in the package's test list
        files: File checks for all files
        site_packages: File checks for Python site-packages
        bin: File checks for binaries in bin/
        lib: File checks for libraries
        include: File checks for include headers
        strict: Whether to fail on non-matched glob patterns (strict mode)
    """

    def __init__(self, inner: _package.PackageContentsTest) -> None:
        self._inner = inner

    @property
    def index(self) -> int:
        """Index of this test in the package's test list."""
        return self._inner.index

    @property
    def files(self) -> "FileChecks":
        """File checks for all files."""
        return FileChecks(self._inner.files)

    @property
    def site_packages(self) -> "FileChecks":
        """File checks for Python site-packages."""
        return FileChecks(self._inner.site_packages)

    @property
    def bin(self) -> "FileChecks":
        """File checks for binaries in bin/."""
        return FileChecks(self._inner.bin)

    @property
    def lib(self) -> "FileChecks":
        """File checks for libraries."""
        return FileChecks(self._inner.lib)

    @property
    def include(self) -> "FileChecks":
        """File checks for include headers."""
        return FileChecks(self._inner.include)

    @property
    def strict(self) -> bool:
        """Whether to fail on non-matched glob patterns (strict mode)."""
        return self._inner.strict

    def to_dict(self) -> dict[str, Any]:
        """Convert to a dictionary."""
        return self._inner.to_dict()

    def __repr__(self) -> str:
        return f"PackageContentsTest(strict={self.strict})"


class FileChecks:
    """File existence checks (glob patterns).

    Attributes:
        exists: Glob patterns that must match at least one file
        not_exists: Glob patterns that must NOT match any file
    """

    def __init__(self, inner: _package.FileChecks) -> None:
        self._inner = inner

    @property
    def exists(self) -> list[str]:
        """Glob patterns that must match at least one file."""
        return self._inner.exists

    @property
    def not_exists(self) -> list[str]:
        """Glob patterns that must NOT match any file."""
        return self._inner.not_exists

    def __repr__(self) -> str:
        return f"FileChecks(exists={len(self.exists)}, not_exists={len(self.not_exists)})"


class TestResult:
    """Result of running a test.

    Attributes:
        success: Whether the test passed
        output: Test output/logs
        test_index: Index of the test that was run
    """

    def __init__(self, inner: _package.TestResult) -> None:
        self._inner = inner

    @property
    def success(self) -> bool:
        """Whether the test passed."""
        return self._inner.success

    @property
    def output(self) -> list[str]:
        """Test output/logs."""
        return self._inner.output

    @property
    def test_index(self) -> int:
        """Index of the test that was run."""
        return self._inner.test_index

    def __bool__(self) -> bool:
        """Returns True if the test passed."""
        return self.success

    def __repr__(self) -> str:
        status = "PASS" if self.success else "FAIL"
        return f"TestResult(index={self.test_index}, status={status})"


class PathEntry:
    """Path entry from paths.json.

    Attributes:
        relative_path: Relative path of the file in the package
        no_link: Whether to skip linking this file
        path_type: Path type ("hardlink", "softlink", or "directory")
        size_in_bytes: Size of the file in bytes (if available)
        sha256: SHA256 hash of the file (if available)
    """

    def __init__(self, inner: _package.PathEntry) -> None:
        self._inner = inner

    @property
    def relative_path(self) -> str:
        """Relative path of the file in the package."""
        return self._inner.relative_path

    @property
    def no_link(self) -> bool:
        """Whether to skip linking this file."""
        return self._inner.no_link

    @property
    def path_type(self) -> str:
        """Path type: "hardlink", "softlink", or "directory"."""
        return self._inner.path_type

    @property
    def size_in_bytes(self) -> int | None:
        """Size of the file in bytes (if available)."""
        return self._inner.size_in_bytes

    @property
    def sha256(self) -> str | None:
        """SHA256 hash of the file (if available)."""
        return self._inner.sha256

    def __repr__(self) -> str:
        return f"PathEntry('{self.relative_path}')"


class RebuildResult:
    """Result of rebuilding a package.

    Contains the original and rebuilt package paths, their SHA256 hashes,
    and provides access to the rebuilt Package object for inspection.

    Attributes:
        original_path: Path to the original package
        rebuilt_path: Path to the rebuilt package
        original_sha256: SHA256 hash of the original package (hex-encoded)
        rebuilt_sha256: SHA256 hash of the rebuilt package (hex-encoded)
        is_identical: Whether the hashes match (reproducible build)
    """

    def __init__(self, inner: _package.RebuildResult) -> None:
        """Initialize from internal RebuildResult object.

        Users should use Package.rebuild() instead of this constructor.
        """
        self._inner = inner

    @property
    def original_path(self) -> Path:
        """Path to the original package."""
        return Path(self._inner.original_path)

    @property
    def rebuilt_path(self) -> Path:
        """Path to the rebuilt package."""
        return Path(self._inner.rebuilt_path)

    @property
    def original_sha256(self) -> str:
        """SHA256 hash of the original package (hex-encoded)."""
        return self._inner.original_sha256

    @property
    def rebuilt_sha256(self) -> str:
        """SHA256 hash of the rebuilt package (hex-encoded)."""
        return self._inner.rebuilt_sha256

    @property
    def is_identical(self) -> bool:
        """Whether the original and rebuilt packages are bit-for-bit identical."""
        return self._inner.is_identical

    @property
    def rebuilt_package(self) -> Package:
        """Get the rebuilt Package object for inspection.

        Returns:
            A Package object for the rebuilt package
        """
        return Package(self._inner.rebuilt_package())

    def __repr__(self) -> str:
        status = "identical" if self.is_identical else "different"
        return f"RebuildResult(original='{self.original_path}', rebuilt='{self.rebuilt_path}', status={status})"
