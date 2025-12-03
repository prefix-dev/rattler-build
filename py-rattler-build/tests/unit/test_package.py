"""Tests for the Package inspection and testing API."""

import shutil
from pathlib import Path

import pytest

import rattler_build
from rattler_build import Package, PackageTest
from rattler_build import TestResult as _TestResult


@pytest.fixture
def built_package(tmp_path: Path, recipes_dir: Path) -> Path:
    """Build a test package and return the path to the .conda file."""
    recipe_name = "recipe.yaml"
    recipe_path = tmp_path / recipe_name
    shutil.copy(recipes_dir / "dummy" / recipe_name, recipe_path)
    output_dir = tmp_path / "output"

    # Build without running tests so we can test manually
    rattler_build.build_recipes([recipe_path], output_dir=output_dir, test="skip")

    # Find the built package
    conda_files = list(output_dir.glob("**/*.conda"))
    assert len(conda_files) > 0, "No .conda files were built"
    return conda_files[0]


class TestPackageLoading:
    """Tests for loading packages."""

    def test_from_file(self, built_package: Path) -> None:
        """Test loading a package from file."""
        pkg = Package.from_file(built_package)
        assert pkg is not None
        assert pkg.path == built_package

    def test_from_file_string_path(self, built_package: Path) -> None:
        """Test loading a package from file with string path."""
        pkg = Package.from_file(str(built_package))
        assert pkg is not None

    def test_from_file_invalid_path(self, tmp_path: Path) -> None:
        """Test that loading a non-existent file raises an error."""
        with pytest.raises(rattler_build.RattlerBuildError):
            Package.from_file(tmp_path / "nonexistent.conda")


class TestPackageMetadata:
    """Tests for package metadata inspection."""

    def test_name(self, built_package: Path) -> None:
        """Test getting package name."""
        pkg = Package.from_file(built_package)
        assert pkg.name == "dummy-x"

    def test_version(self, built_package: Path) -> None:
        """Test getting package version."""
        pkg = Package.from_file(built_package)
        assert pkg.version == "0.1.0"

    def test_build_string(self, built_package: Path) -> None:
        """Test getting build string."""
        pkg = Package.from_file(built_package)
        assert isinstance(pkg.build_string, str)
        assert len(pkg.build_string) > 0

    def test_build_number(self, built_package: Path) -> None:
        """Test getting build number."""
        pkg = Package.from_file(built_package)
        assert isinstance(pkg.build_number, int)
        assert pkg.build_number >= 0

    def test_subdir(self, built_package: Path) -> None:
        """Test getting subdir (platform)."""
        pkg = Package.from_file(built_package)
        # Could be noarch or a platform-specific subdir
        assert pkg.subdir is None or isinstance(pkg.subdir, str)

    def test_noarch(self, built_package: Path) -> None:
        """Test getting noarch type."""
        pkg = Package.from_file(built_package)
        # noarch can be None, "python", or "generic"
        assert pkg.noarch is None or pkg.noarch in ("python", "generic")

    def test_depends(self, built_package: Path) -> None:
        """Test getting dependencies."""
        pkg = Package.from_file(built_package)
        assert isinstance(pkg.depends, list)
        # All items should be strings
        for dep in pkg.depends:
            assert isinstance(dep, str)

    def test_constrains(self, built_package: Path) -> None:
        """Test getting constraints."""
        pkg = Package.from_file(built_package)
        assert isinstance(pkg.constrains, list)
        for constraint in pkg.constrains:
            assert isinstance(constraint, str)

    def test_license(self, built_package: Path) -> None:
        """Test getting license."""
        pkg = Package.from_file(built_package)
        # License can be None or a string
        assert pkg.license is None or isinstance(pkg.license, str)

    def test_to_dict(self, built_package: Path) -> None:
        """Test converting to dictionary."""
        pkg = Package.from_file(built_package)
        d = pkg.to_dict()
        assert isinstance(d, dict)
        assert "name" in d
        assert d["name"] == pkg.name
        assert "version" in d
        assert d["version"] == pkg.version

    def test_repr(self, built_package: Path) -> None:
        """Test string representation."""
        pkg = Package.from_file(built_package)
        repr_str = repr(pkg)
        assert "Package" in repr_str
        assert pkg.name in repr_str
        assert pkg.version in repr_str


class TestPackageFiles:
    """Tests for package file listing."""

    def test_files(self, built_package: Path) -> None:
        """Test getting list of files in the package."""
        pkg = Package.from_file(built_package)
        files = pkg.files
        assert isinstance(files, list)
        assert len(files) > 0
        # All items should be strings (file paths)
        for f in files:
            assert isinstance(f, str)

    def test_files_contains_bin(self, built_package: Path) -> None:
        """Test that files include bin directory contents."""
        pkg = Package.from_file(built_package)
        files = pkg.files
        # The dummy package creates a binary
        bin_files = [f for f in files if "bin/" in f or f.startswith("bin")]
        assert len(bin_files) > 0


class TestPackageTests:
    """Tests for package test inspection."""

    def test_tests_list(self, built_package: Path) -> None:
        """Test getting list of tests."""
        pkg = Package.from_file(built_package)
        tests = pkg.tests
        assert isinstance(tests, list)
        # The dummy package has tests
        assert len(tests) > 0

    def test_test_count(self, built_package: Path) -> None:
        """Test getting test count."""
        pkg = Package.from_file(built_package)
        count = pkg.test_count()
        assert isinstance(count, int)
        assert count == len(pkg.tests)

    def test_test_kind(self, built_package: Path) -> None:
        """Test getting test kind."""
        pkg = Package.from_file(built_package)
        for test in pkg.tests:
            assert isinstance(test, PackageTest)
            assert test.kind in (
                "python",
                "commands",
                "perl",
                "r",
                "ruby",
                "downstream",
                "package_contents",
            )

    def test_test_index(self, built_package: Path) -> None:
        """Test getting test index."""
        pkg = Package.from_file(built_package)
        for i, test in enumerate(pkg.tests):
            assert test.index == i

    def test_test_repr(self, built_package: Path) -> None:
        """Test test string representation."""
        pkg = Package.from_file(built_package)
        if pkg.tests:
            test = pkg.tests[0]
            repr_str = repr(test)
            assert "PackageTest" in repr_str
            assert "index=" in repr_str
            assert "kind=" in repr_str

    def test_test_to_dict(self, built_package: Path) -> None:
        """Test converting test to dictionary."""
        pkg = Package.from_file(built_package)
        if pkg.tests:
            test = pkg.tests[0]
            d = test.to_dict()
            assert isinstance(d, dict)


class TestTestTypeAccessors:
    """Tests for specific test type accessors."""

    def test_as_commands_test(self, built_package: Path) -> None:
        """Test getting commands test details."""
        pkg = Package.from_file(built_package)
        for test in pkg.tests:
            if test.kind == "commands":
                cmd_test = test.as_commands_test()
                assert cmd_test is not None
                # Check that script property exists
                assert hasattr(cmd_test, "script")
                assert hasattr(cmd_test, "requirements_run")
                assert hasattr(cmd_test, "requirements_build")
            else:
                # Should return None for non-commands tests
                assert test.as_commands_test() is None

    def test_as_python_test_returns_none_for_commands(self, built_package: Path) -> None:
        """Test that as_python_test returns None for commands test."""
        pkg = Package.from_file(built_package)
        for test in pkg.tests:
            if test.kind == "commands":
                assert test.as_python_test() is None


class TestRunTests:
    """Tests for running package tests."""

    def test_run_test(self, built_package: Path) -> None:
        """Test running a specific test by index."""
        pkg = Package.from_file(built_package)
        if pkg.test_count() > 0:
            result = pkg.run_test(0)
            assert isinstance(result, _TestResult)
            assert isinstance(result.success, bool)
            assert isinstance(result.output, list)
            assert result.test_index == 0

    def test_run_tests(self, built_package: Path) -> None:
        """Test running all tests."""
        pkg = Package.from_file(built_package)
        if pkg.test_count() > 0:
            results = pkg.run_tests()
            assert isinstance(results, list)
            # Should have at least one result
            assert len(results) > 0
            for result in results:
                assert isinstance(result, _TestResult)

    def test_run_test_invalid_index(self, built_package: Path) -> None:
        """Test that running a test with invalid index raises error."""
        pkg = Package.from_file(built_package)
        with pytest.raises(rattler_build.RattlerBuildError):
            pkg.run_test(999)

    def test_test_result_bool(self, built_package: Path) -> None:
        """Test that TestResult can be used as boolean."""
        pkg = Package.from_file(built_package)
        if pkg.test_count() > 0:
            result = pkg.run_test(0)
            # TestResult should be truthy if successful
            if result.success:
                assert result
            else:
                assert not result

    def test_test_result_repr(self, built_package: Path) -> None:
        """Test TestResult string representation."""
        pkg = Package.from_file(built_package)
        if pkg.test_count() > 0:
            result = pkg.run_test(0)
            repr_str = repr(result)
            assert "TestResult" in repr_str
            assert "index=" in repr_str
            assert "status=" in repr_str
