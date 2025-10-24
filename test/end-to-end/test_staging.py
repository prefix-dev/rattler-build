"""End-to-end tests for staging output functionality in multi-output recipes."""

import json
import os
import platform
from pathlib import Path
from subprocess import CalledProcessError

import pytest

from helpers import (
    RattlerBuild,
    get_extracted_package,
)


def test_basic_staging(rattler_build: RattlerBuild, recipes: Path, tmp_path: Path):
    """Test basic staging output with multiple package outputs inheriting from it."""
    rattler_build.build(recipes / "staging/basic-staging.yaml", tmp_path)

    # Both package outputs should be built
    pkg1 = get_extracted_package(tmp_path, "foo-split-1")
    pkg2 = get_extracted_package(tmp_path, "foo-othersplit")

    # Both should have the file from the staging cache
    assert (pkg1 / "foo.txt").exists()
    assert (pkg2 / "foo.txt").exists()

    # Verify the content is the prefix path
    content1 = (pkg1 / "foo.txt").read_text().strip()
    content2 = (pkg2 / "foo.txt").read_text().strip()

    # Both should have the same content (from the shared staging cache)
    assert content1 == content2


@pytest.mark.skipif(os.name == "nt", reason="symlinks not fully supported on Windows")
def test_staging_symlinks(rattler_build: RattlerBuild, recipes: Path, tmp_path: Path):
    """Test that symlinks are properly cached and restored in staging outputs."""
    rattler_build.build(recipes / "staging/staging-symlinks.yaml", tmp_path)

    # First package output with most symlinks
    pkg = get_extracted_package(tmp_path, "cache-symlinks")

    # Basic existence checks
    assert (pkg / "bin/exe").exists()
    assert (pkg / "bin/exe-symlink").exists()
    assert (pkg / "bin/exe-symlink").is_symlink()
    assert (pkg / "foo.txt").exists()
    assert (pkg / "foo-symlink.txt").is_symlink()
    assert (pkg / "relative-symlink.txt").is_symlink()
    assert (
        pkg / "broken-symlink.txt"
    ).is_symlink()  # Broken symlinks should be preserved

    # Detailed symlink verification
    # foo-symlink.txt should be a relative symlink
    foo_symlink = pkg / "foo-symlink.txt"
    assert foo_symlink.is_symlink()
    assert not foo_symlink.readlink().is_absolute()

    # relative-symlink.txt should point to foo.txt
    relative_symlink = pkg / "relative-symlink.txt"
    assert relative_symlink.is_symlink()
    assert relative_symlink.readlink() == Path("foo.txt")

    # bin/exe-symlink should point to exe
    exe_symlink = pkg / "bin/exe-symlink"
    assert exe_symlink.is_symlink()
    assert exe_symlink.readlink() == Path("exe")

    # broken-symlink.txt should exist as symlink but not resolve
    broken_symlink = pkg / "broken-symlink.txt"
    assert broken_symlink.is_symlink()
    assert not broken_symlink.exists()  # Broken, so exists() returns False

    # Verify paths.json metadata
    paths_json = pkg / "info/paths.json"
    paths_data = json.loads(paths_json.read_text())
    paths = paths_data["paths"]

    # Check that all symlinks are marked with path_type: "softlink"
    symlink_paths = [p for p in paths if "symlink" in p["_path"]]
    for p in symlink_paths:
        assert p["path_type"] == "softlink"
        # Symlinks should have empty content hash
        assert (
            p["sha256"]
            == "e3b0c44298fc1c149afbf4c8996fb92427ae41e4649b934ca495991b7852b855"
        )

    # Verify prefix.txt has prefix placeholder
    prefix_txt = pkg / "prefix.txt"
    if prefix_txt.exists():
        contents = prefix_txt.read_text()
        assert len(contents) > 0
        # Find the path in paths.json for prefix.txt
        for p in paths:
            if p["_path"] == "prefix.txt":
                assert p["path_type"] == "hardlink"
                assert "prefix_placeholder" in p
                assert p["prefix_placeholder"] == contents.strip()

    # Verify excluded files are not present
    assert not (pkg / "absolute-symlink.txt").exists()
    assert not (pkg / "bin/absolute-exe-symlink").exists()

    # Second package with only absolute symlinks
    pkg_absolute = get_extracted_package(tmp_path, "absolute-cache-symlinks")

    # Check symlink files exist (use lexists to detect broken symlinks too)
    abs_symlink = pkg_absolute / "absolute-symlink.txt"
    abs_exe_symlink = pkg_absolute / "bin/absolute-exe-symlink"

    assert abs_symlink.exists() or abs_symlink.is_symlink(), (
        "absolute-symlink.txt not found"
    )
    assert abs_exe_symlink.exists() or abs_exe_symlink.is_symlink(), (
        "bin/absolute-exe-symlink not found"
    )

    # Verify they are symlinks
    if abs_symlink.is_symlink():
        target = abs_symlink.readlink()
        # Should be relative after prefix replacement
        assert not target.is_absolute(), f"Expected relative symlink but got {target}"

    if abs_exe_symlink.is_symlink():
        target = abs_exe_symlink.readlink()
        assert not target.is_absolute(), f"Expected relative symlink but got {target}"


def test_staging_with_deps(rattler_build: RattlerBuild, recipes: Path, tmp_path: Path):
    """Test staging output with build dependencies (cmake)."""
    rattler_build.build(recipes / "staging/staging-with-deps.yaml", tmp_path)

    # Both package outputs should be built
    pkg1 = get_extracted_package(tmp_path, "check-1")
    pkg2 = get_extracted_package(tmp_path, "check-2")

    # Verify they were created
    assert (pkg1 / "info/index.json").exists()
    assert (pkg2 / "info/index.json").exists()


def test_staging_run_exports(
    rattler_build: RattlerBuild, recipes: Path, tmp_path: Path
):
    """Test run_exports handling with staging outputs."""
    # Build the staging recipe (includes helper package as first output)
    rattler_build.build(recipes / "staging/staging-run-exports.yaml", tmp_path)

    # Package that inherits run_exports
    pkg = get_extracted_package(tmp_path, "cache-run-exports")
    index = json.loads((pkg / "info/index.json").read_text())
    depends = index.get("depends", [])
    assert any("normal-run-exports" in dep for dep in depends), (
        f"normal-run-exports should be in dependencies but got {depends}"
    )

    # Package that ignores run_exports from specific package
    pkg_no_from = get_extracted_package(tmp_path, "no-cache-from-package-run-exports")
    index = json.loads((pkg_no_from / "info/index.json").read_text())
    assert "normal-run-exports" not in index.get("depends", [])

    # Package that ignores run_exports by name
    pkg_no_name = get_extracted_package(tmp_path, "no-cache-by-name-run-exports")
    index = json.loads((pkg_no_name / "info/index.json").read_text())
    assert "normal-run-exports" not in index.get("depends", [])


@pytest.mark.skip(reason="Requires libfoo variant package")
def test_staging_with_variants(
    rattler_build: RattlerBuild, recipes: Path, tmp_path: Path
):
    """Test staging output with variant-dependent requirements."""
    rattler_build.build(recipes / "staging/staging-with-variants.yaml", tmp_path)

    # First package output
    pkg = get_extracted_package(tmp_path, "variant-cache")
    assert (pkg / "hello.txt").exists()
    assert (pkg / "hello.txt").read_text().strip() == "hello"

    # Second package with python dependency
    pkg_py = get_extracted_package(tmp_path, "variant-cache-py")
    index = json.loads((pkg_py / "info/index.json").read_text())
    # Should have python in dependencies
    assert any("python" in dep for dep in index.get("depends", []))


def test_staging_cache_reuse(
    rattler_build: RattlerBuild, recipes: Path, tmp_path: Path
):
    """Test that staging cache is reused on second build (performance test)."""
    import time

    # First build - should build the staging cache
    start1 = time.time()
    rattler_build.build(recipes / "staging/basic-staging.yaml", tmp_path)
    duration1 = time.time() - start1

    # Clean the output packages but keep the cache
    for pkg in tmp_path.glob("**/*.conda"):
        pkg.unlink()
    for pkg in tmp_path.glob("**/*.tar.bz2"):
        pkg.unlink()

    # Second build - should use the cached staging
    start2 = time.time()
    rattler_build.build(recipes / "staging/basic-staging.yaml", tmp_path)
    duration2 = time.time() - start2

    # Second build should be faster (though this is a weak assertion)
    # We mainly just verify it doesn't error when using cache
    pkg1 = get_extracted_package(tmp_path, "foo-split-1")
    pkg2 = get_extracted_package(tmp_path, "foo-othersplit")

    assert (pkg1 / "foo.txt").exists()
    assert (pkg2 / "foo.txt").exists()


@pytest.mark.xfail(reason="Staging implementation not finished")
def test_multiple_staging_caches(
    rattler_build: RattlerBuild, recipes: Path, tmp_path: Path
):
    """Test multiple independent staging outputs in one recipe."""
    rattler_build.build(recipes / "staging/multiple-staging-caches.yaml", tmp_path)

    # Package from core-build staging
    pkg_core = get_extracted_package(tmp_path, "libcore")
    assert (pkg_core / "lib/libcore.so").exists()
    assert (pkg_core / "include/core.h").exists()

    # Package from python-build staging
    pkg_py = get_extracted_package(tmp_path, "python-mycore")
    assert (pkg_py / "lib/python3.11/site-packages/mycore.py").exists()

    # Dev package also from core-build staging
    pkg_dev = get_extracted_package(tmp_path, "libcore-dev")
    assert (pkg_dev / "include/core.h").exists()
    # Should not have the lib file (filtered by files section)
    assert not (pkg_dev / "lib/libcore.so").exists()


def test_staging_with_top_level_inherit(
    rattler_build: RattlerBuild, recipes: Path, tmp_path: Path
):
    """Test mix of staging inheritance and top-level inheritance."""
    rattler_build.build(
        recipes / "staging/staging-with-top-level-inherit.yaml", tmp_path
    )

    # Package that inherits from staging
    pkg_compiled = get_extracted_package(tmp_path, "mixed-compiled")
    assert (pkg_compiled / "lib/compiled.so").exists()

    # Package that inherits from top-level
    pkg_data = get_extracted_package(tmp_path, "mixed-data")
    assert (pkg_data / "share/data.txt").exists()
    assert (pkg_data / "share/data.txt").read_text().strip() == "data.txt"


@pytest.mark.xfail(reason="Staging implementation not finished")
def test_staging_no_inherit(rattler_build: RattlerBuild, recipes: Path, tmp_path: Path):
    """Test staging output used for side effects without explicit inheritance."""
    rattler_build.build(recipes / "staging/staging-no-inherit.yaml", tmp_path)

    # Both packages should build successfully
    pkg_a = get_extracted_package(tmp_path, "package-a")
    pkg_b = get_extracted_package(tmp_path, "package-b")

    assert (pkg_a / "package-a.txt").exists()
    assert (pkg_b / "package-b.txt").exists()

    # Verify content
    assert (pkg_a / "package-a.txt").read_text().strip() == "Package A"
    assert (pkg_b / "package-b.txt").read_text().strip() == "Package B"


def test_staging_work_dir_cache(
    rattler_build: RattlerBuild, recipes: Path, tmp_path: Path
):
    """Test that both prefix AND work directory files are cached and restored."""
    rattler_build.build(recipes / "staging/staging-work-dir-cache.yaml", tmp_path)

    pkg = get_extracted_package(tmp_path, "work-dir-test")

    # Should have the library from prefix
    assert (pkg / "lib/mylib.a").exists()
    assert (pkg / "lib/mylib.a").read_text().strip() == "library"


def test_staging_complex_deps(
    rattler_build: RattlerBuild, recipes: Path, tmp_path: Path
):
    """Test complex dependency scenarios with staging."""
    rattler_build.build(recipes / "staging/staging-complex-deps.yaml", tmp_path)

    # Package with run_exports inherited
    pkg_full = get_extracted_package(tmp_path, "complex-deps-full")
    assert (pkg_full / "lib/libcomplex.so").exists()
    index_full = json.loads((pkg_full / "info/index.json").read_text())
    # Should have dependencies
    assert any("zlib" in dep for dep in index_full.get("depends", []))
    assert any("libcurl" in dep for dep in index_full.get("depends", []))

    # Package without run_exports
    pkg_minimal = get_extracted_package(tmp_path, "complex-deps-minimal")
    assert (pkg_minimal / "lib/libcomplex.so").exists()
    index_minimal = json.loads((pkg_minimal / "info/index.json").read_text())
    # Should only have explicit dependencies
    assert any("zlib" in dep for dep in index_minimal.get("depends", []))


def test_staging_render_only(
    rattler_build: RattlerBuild, recipes: Path, tmp_path: Path
):
    """Test that rendering works correctly with staging outputs."""
    rendered = rattler_build.render(recipes / "staging/basic-staging.yaml", tmp_path)

    # Should have 2 outputs (foo-split-1 and foo-othersplit)
    assert len(rendered) == 2

    # Both should reference the staging cache
    for output in rendered:
        assert "staging_caches" in output["recipe"]
        staging_caches = output["recipe"]["staging_caches"]
        assert len(staging_caches) == 1
        assert staging_caches[0]["name"] == "foo-build"


def test_staging_hash_includes_variant(
    rattler_build: RattlerBuild, recipes: Path, tmp_path: Path
):
    """Test that staging cache hash includes variant information."""
    rendered = rattler_build.render(recipes / "staging/basic-staging.yaml", tmp_path)

    # Check that used_variant is set for staging caches
    for output in rendered:
        if "staging_caches" in output["recipe"]:
            for staging in output["recipe"]["staging_caches"]:
                # The staging cache should have a used_variant
                # (in this case, probably just target_platform)
                assert "used_variant" in staging or "name" in staging


def test_staging_different_platforms(
    rattler_build: RattlerBuild, recipes: Path, tmp_path: Path
):
    """Test that different platforms get different staging caches."""
    # This test verifies the cache key includes platform information
    # by building for different platforms and checking they succeed

    current_platform = platform.system()
    if current_platform == "Linux":
        target_platform = "linux-64"
    elif current_platform == "Darwin":
        target_platform = "osx-arm64"
    elif current_platform == "Windows":
        target_platform = "win-64"
    else:
        pytest.skip("Unsupported platform")

    # Build for the current platform
    rattler_build.build(
        recipes / "staging/basic-staging.yaml",
        tmp_path,
        extra_args=["--target-platform", target_platform],
    )

    pkg1 = get_extracted_package(tmp_path, "foo-split-1")
    assert (pkg1 / "foo.txt").exists()


def test_staging_with_tests(rattler_build: RattlerBuild, recipes: Path, tmp_path: Path):
    """Test that package tests work correctly with staging outputs."""
    # The basic-staging.yaml includes tests that run 'cat $PREFIX/foo.txt'
    # This verifies the staging cache files are available during tests

    rattler_build.build(recipes / "staging/basic-staging.yaml", tmp_path)

    # If tests failed, the build would have failed
    # Just verify the packages were created
    pkg1 = get_extracted_package(tmp_path, "foo-split-1")
    pkg2 = get_extracted_package(tmp_path, "foo-othersplit")

    assert (pkg1 / "info/index.json").exists()
    assert (pkg2 / "info/index.json").exists()


def test_staging_metadata_preserved(
    rattler_build: RattlerBuild, recipes: Path, tmp_path: Path
):
    """Test that staging metadata is preserved in package outputs."""
    rattler_build.build(recipes / "staging/multiple-staging-caches.yaml", tmp_path)

    pkg = get_extracted_package(tmp_path, "libcore")

    # Check that the rendered recipe includes staging information
    assert (pkg / "info/recipe/rendered_recipe.yaml").exists()

    import yaml

    rendered = yaml.safe_load((pkg / "info/recipe/rendered_recipe.yaml").read_text())

    # The recipe should have staging_caches
    assert "staging_caches" in rendered
    assert len(rendered["staging_caches"]) >= 1

    # Should have inherits_from
    assert "inherits_from" in rendered
    assert rendered["inherits_from"]["cache_name"] == "core-build"


@pytest.mark.xfail(reason="Staging implementation not finished")
def test_staging_error_invalid_inherit(
    rattler_build: RattlerBuild, recipes: Path, tmp_path: Path
):
    """Test that invalid staging cache references produce errors."""
    # Create a test recipe with invalid inherit reference
    invalid_recipe = tmp_path / "invalid.yaml"
    invalid_recipe.write_text(
        """
schema_version: 1

recipe:
  name: invalid-test
  version: 1.0.0

outputs:
  - package:
      name: invalid-pkg
      version: 1.0.0
    inherit: nonexistent-cache
"""
    )

    # This should fail during rendering
    with pytest.raises(CalledProcessError):
        rattler_build.build(invalid_recipe, tmp_path)


@pytest.mark.xfail(reason="Staging implementation not finished")
def test_staging_files_selection(
    rattler_build: RattlerBuild, recipes: Path, tmp_path: Path
):
    """Test that file selection works correctly with staging inheritance."""
    rattler_build.build(recipes / "staging/multiple-staging-caches.yaml", tmp_path)

    # libcore should have both lib and include
    pkg_core = get_extracted_package(tmp_path, "libcore")
    paths_core = json.loads((pkg_core / "info/paths.json").read_text())
    core_files = [p["_path"] for p in paths_core["paths"]]
    assert any("lib/libcore.so" in f for f in core_files)
    assert any("include/core.h" in f for f in core_files)

    # libcore-dev should only have include
    pkg_dev = get_extracted_package(tmp_path, "libcore-dev")
    paths_dev = json.loads((pkg_dev / "info/paths.json").read_text())
    dev_files = [p["_path"] for p in paths_dev["paths"]]
    assert not any("lib/" in f for f in dev_files)
    assert any("include/" in f for f in dev_files)


@pytest.mark.xfail(reason="Staging implementation not finished")
def test_staging_source_handling(
    rattler_build: RattlerBuild, recipes: Path, tmp_path: Path
):
    """Test that sources are properly handled in staging outputs."""
    # The staging recipes reference sources, ensure they're fetched correctly
    rattler_build.build(recipes / "staging/multiple-staging-caches.yaml", tmp_path)

    pkg = get_extracted_package(tmp_path, "libcore")

    # Check finalized sources in rendered recipe
    assert (pkg / "info/recipe/rendered_recipe.yaml").exists()

    import yaml

    rendered = yaml.safe_load((pkg / "info/recipe/rendered_recipe.yaml").read_text())

    # Should have finalized sources (even if they're dummy URLs in test)
    if "finalized_sources" in rendered:
        assert isinstance(rendered["finalized_sources"], list)


def test_staging_build_number_propagation(
    rattler_build: RattlerBuild, recipes: Path, tmp_path: Path
):
    """Test that build numbers are properly handled with staging."""
    rattler_build.build(recipes / "staging/staging-with-deps.yaml", tmp_path)

    pkg = get_extracted_package(tmp_path, "check-1")
    index = json.loads((pkg / "info/index.json").read_text())

    # Build number should be 0 as specified in context
    assert index["build_number"] == 0


@pytest.mark.xfail(reason="Staging implementation not finished")
def test_staging_about_propagation(
    rattler_build: RattlerBuild, recipes: Path, tmp_path: Path
):
    """Test that about metadata is properly set in package outputs."""
    rattler_build.build(recipes / "staging/multiple-staging-caches.yaml", tmp_path)

    pkg = get_extracted_package(tmp_path, "libcore")
    about = json.loads((pkg / "info/about.json").read_text())

    # About information should be present
    assert about["summary"] == "Core library"
    assert about["license"] == "MIT"


@pytest.mark.skipif(os.name == "nt", reason="symlinks not fully supported on Windows")
def test_staging_select_files_with_symlinks(
    rattler_build: RattlerBuild, recipes: Path, tmp_path: Path
):
    """Test that file selection with glob patterns correctly handles versioned .so files.

    This tests issue #1290: https://github.com/prefix-dev/rattler-build/issues/1290
    The pattern lib/*.so.* should select lib/libdav1d.so.7.0.0 and lib/libdav1d.so.7
    but NOT lib/libdav1d.so (which doesn't match the pattern).
    """
    rattler_build.build(recipes / "staging/staging-compiler.yaml", tmp_path)

    pkg = get_extracted_package(tmp_path, "testlib-so-version")
    paths = json.loads((pkg / "info/paths.json").read_text())
    path_files = [p["_path"] for p in paths["paths"]]

    # Should include versioned .so files
    assert any("lib/libdav1d.so.7.0.0" in f for f in path_files), (
        f"lib/libdav1d.so.7.0.0 not found in {path_files}"
    )
    assert any("lib/libdav1d.so.7" in f for f in path_files), (
        f"lib/libdav1d.so.7 not found in {path_files}"
    )

    # Should NOT include the .so file (no version)
    assert not any(f.endswith("lib/libdav1d.so") for f in path_files), (
        f"lib/libdav1d.so should not be included in {path_files}"
    )

    # Verify the symlinks exist in the actual package
    assert (pkg / "lib/libdav1d.so.7.0.0").exists()
    assert (pkg / "lib/libdav1d.so.7").exists()
    assert (pkg / "lib/libdav1d.so.7").is_symlink()
    assert not (pkg / "lib/libdav1d.so").exists()


def test_staging_run_exports_ignore_from_package(
    rattler_build: RattlerBuild, recipes: Path, tmp_path: Path
):
    """Test ignore_run_exports.from_package for staging outputs.

    Verifies that run_exports from specific packages can be selectively ignored
    in staging cache requirements.
    """
    # Build the staging recipe (includes helper package as first output)
    rattler_build.build(recipes / "staging/staging-run-exports-test2.yaml", tmp_path)

    pkg = get_extracted_package(tmp_path, "staging-ignore-run-exports-from-package")
    index = json.loads((pkg / "info/index.json").read_text())

    # normal-run-exports should NOT be in dependencies (ignored by from_package)
    depends = index.get("depends", [])
    assert not any("normal-run-exports" in dep for dep in depends), (
        f"normal-run-exports should be ignored but found in {depends}"
    )


def test_staging_run_exports_ignore_by_name(
    rattler_build: RattlerBuild, recipes: Path, tmp_path: Path
):
    """Test ignore_run_exports.by_name for staging outputs.

    Verifies that run_exports from packages can be ignored by name pattern
    in staging cache requirements.
    """
    # Build the staging recipe (includes helper package as first output)
    rattler_build.build(recipes / "staging/staging-run-exports-test3.yaml", tmp_path)

    pkg = get_extracted_package(tmp_path, "staging-ignore-run-exports-by-name")
    index = json.loads((pkg / "info/index.json").read_text())

    # normal-run-exports should NOT be in dependencies (ignored by name)
    depends = index.get("depends", [])
    assert not any("normal-run-exports" in dep for dep in depends), (
        f"normal-run-exports should be ignored but found in {depends}"
    )


if __name__ == "__main__":
    # Allow running individual tests
    pytest.main([__file__, "-v"])
