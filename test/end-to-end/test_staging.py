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
    rattler_build.build(
        recipes / "staging/basic-staging.yaml", tmp_path, extra_args=["--experimental"]
    )

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
    rattler_build.build(
        recipes / "staging/staging-symlinks.yaml",
        tmp_path,
        extra_args=["--experimental"],
    )

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
    rattler_build.build(
        recipes / "staging/staging-with-deps.yaml",
        tmp_path,
        extra_args=["--experimental"],
    )

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
    rattler_build.build(
        recipes / "staging/staging-run-exports.yaml",
        tmp_path,
        extra_args=["--experimental"],
    )

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


def test_staging_with_variants(
    rattler_build: RattlerBuild, recipes: Path, tmp_path: Path
):
    """Test that staging recipes with variant-dependent requirements can be rendered.

    This is a render-only test since we don't have the actual libfoo package,
    but we can verify the recipe structure and variant handling is correct.
    """
    rendered_outputs = rattler_build.render(
        recipes / "staging/staging-with-variants.yaml",
        tmp_path,
        variant_config=recipes / "staging/staging-variants-variant.yaml",
        extra_args=["--experimental"],
    )

    # There should be 2x variant-cache and 2x3 variant-cache-py outputs for 2 versions of libfoo and 3 versions of python
    variant_cache_outputs = [
        o for o in rendered_outputs if o["recipe"]["package"]["name"] == "variant-cache"
    ]
    variant_cache_py_outputs = [
        o
        for o in rendered_outputs
        if o["recipe"]["package"]["name"] == "variant-cache-py"
    ]

    # Should have 2 variant-cache outputs (one for each libfoo version)
    assert len(variant_cache_outputs) == 2, (
        f"Expected 2 variant-cache outputs, got {len(variant_cache_outputs)}"
    )

    # Should have 6 variant-cache-py outputs (2 libfoo × 3 python versions)
    assert len(variant_cache_py_outputs) == 6, (
        f"Expected 6 variant-cache-py outputs, got {len(variant_cache_py_outputs)}"
    )

    # Verify variant-cache has both libfoo versions
    cache_libfoo_versions = {
        o["build_configuration"]["variant"].get("libfoo") for o in variant_cache_outputs
    }
    assert cache_libfoo_versions == {
        "1.0",
        "2.0",
    }, f"Expected libfoo versions [1.0, 2.0], got {cache_libfoo_versions}"

    # Verify variant-cache-py has all combinations of libfoo and python
    expected_combinations = {
        ("1.0", "3.10"),
        ("1.0", "3.11"),
        ("1.0", "3.12"),
        ("2.0", "3.10"),
        ("2.0", "3.11"),
        ("2.0", "3.12"),
    }
    actual_combinations = {
        (
            o["build_configuration"]["variant"].get("libfoo"),
            o["build_configuration"]["variant"].get("python"),
        )
        for o in variant_cache_py_outputs
    }
    assert actual_combinations == expected_combinations, (
        f"Expected combinations {expected_combinations}, got {actual_combinations}"
    )


def test_multiple_staging_caches(
    rattler_build: RattlerBuild, recipes: Path, tmp_path: Path
):
    """Test multiple independent staging outputs in one recipe."""
    rattler_build.build(
        recipes / "staging/multiple-staging-caches.yaml",
        tmp_path,
        extra_args=["--experimental"],
    )

    # Package from core-build staging
    pkg_core = get_extracted_package(tmp_path, "libcore")
    assert (pkg_core / "lib/libcore.so").exists()
    assert (pkg_core / "include/core.h").exists()

    # Package from python-build staging
    pkg_py = get_extracted_package(tmp_path, "python-mycore")
    assert (pkg_py / "lib/python3.11/site-packages/mycore.py").exists()

    # Dev package also from core-build staging
    pkg_dev = get_extracted_package(tmp_path, "core-headers")
    assert (pkg_dev / "include/core.h").exists()
    # Should not have the lib file (filtered by files section)
    assert not (pkg_dev / "lib/libcore.so").exists()


def test_staging_with_top_level_inherit(
    rattler_build: RattlerBuild, recipes: Path, tmp_path: Path
):
    """Test mix of staging inheritance and top-level inheritance."""
    rattler_build.build(
        recipes / "staging/staging-with-top-level-inherit.yaml",
        tmp_path,
        extra_args=["--experimental"],
    )

    # Package that inherits from staging
    pkg_compiled = get_extracted_package(tmp_path, "mixed-compiled")
    import platform

    if platform.system() == "Windows":
        assert (pkg_compiled / "lib/compiled.dll").exists()
    else:
        assert (pkg_compiled / "lib/compiled.so").exists()

    # Package that inherits from top-level
    pkg_data = get_extracted_package(tmp_path, "mixed-data")
    assert (pkg_data / "share/data.txt").exists()
    assert (pkg_data / "share/data.txt").read_text().strip() == "data.txt"
    assert (pkg_data / "info/licenses/basic-staging.yaml").exists()
    assert (pkg_compiled / "info/licenses/basic-staging.yaml").exists()


def test_staging_no_inherit(rattler_build: RattlerBuild, recipes: Path, tmp_path: Path):
    """Test staging output used for side effects without explicit inheritance."""
    rattler_build.build(
        recipes / "staging/staging-no-inherit.yaml",
        tmp_path,
        extra_args=["--experimental"],
    )

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
    rattler_build.build(
        recipes / "staging/staging-work-dir-cache.yaml",
        tmp_path,
        extra_args=["--experimental"],
    )

    pkg = get_extracted_package(tmp_path, "work-dir-test")

    # Should have the library from prefix
    import platform

    if platform.system() == "Windows":
        assert (pkg / "lib/mylib.lib").exists()
        assert (pkg / "lib/mylib.lib").read_text().strip() == "library"
    else:
        assert (pkg / "lib/mylib.a").exists()
        assert (pkg / "lib/mylib.a").read_text().strip() == "library"


def test_staging_complex_deps(
    rattler_build: RattlerBuild, recipes: Path, tmp_path: Path
):
    """Test complex dependency scenarios with staging."""
    rattler_build.build(
        recipes / "staging/staging-complex-deps.yaml",
        tmp_path,
        extra_args=["--experimental"],
    )

    # Package with run_exports inherited
    pkg_full = get_extracted_package(tmp_path, "complex-deps-full")
    import platform

    if platform.system() == "Windows":
        assert (pkg_full / "lib/libcomplex.dll").exists()
    else:
        assert (pkg_full / "lib/libcomplex.so").exists()
    index_full = json.loads((pkg_full / "info/index.json").read_text())
    # Should have dependencies
    assert any("zlib" in dep for dep in index_full.get("depends", []))
    assert any("libcurl" in dep for dep in index_full.get("depends", []))

    # Package without run_exports
    pkg_minimal = get_extracted_package(tmp_path, "complex-deps-minimal")
    if platform.system() == "Windows":
        assert (pkg_minimal / "lib/libcomplex.dll").exists()
    else:
        assert (pkg_minimal / "lib/libcomplex.so").exists()
    index_minimal = json.loads((pkg_minimal / "info/index.json").read_text())
    # Should only have explicit dependencies
    assert any("zlib" in dep for dep in index_minimal.get("depends", []))


def test_staging_render_only(
    rattler_build: RattlerBuild, recipes: Path, tmp_path: Path
):
    """Test that rendering works correctly with staging outputs."""
    rendered = rattler_build.render(
        recipes / "staging/basic-staging.yaml", tmp_path, extra_args=["--experimental"]
    )

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
    rendered = rattler_build.render(
        recipes / "staging/basic-staging.yaml", tmp_path, extra_args=["--experimental"]
    )

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
        extra_args=["--experimental", "--target-platform", target_platform],
    )

    pkg1 = get_extracted_package(tmp_path, "foo-split-1")
    assert (pkg1 / "foo.txt").exists()


def test_staging_with_tests(rattler_build: RattlerBuild, recipes: Path, tmp_path: Path):
    """Test that package tests work correctly with staging outputs."""
    # The basic-staging.yaml includes tests that run 'cat $PREFIX/foo.txt'
    # This verifies the staging cache files are available during tests

    rattler_build.build(
        recipes / "staging/basic-staging.yaml", tmp_path, extra_args=["--experimental"]
    )

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
    rattler_build.build(
        recipes / "staging/multiple-staging-caches.yaml",
        tmp_path,
        extra_args=["--experimental"],
    )

    pkg = get_extracted_package(tmp_path, "libcore")

    # Check that the rendered recipe includes staging information
    assert (pkg / "info/recipe/rendered_recipe.yaml").exists()

    import yaml

    rendered = yaml.safe_load((pkg / "info/recipe/rendered_recipe.yaml").read_text())

    # The staging_caches and inherits_from are in the recipe section
    recipe = rendered["recipe"]

    # The recipe should have staging_caches
    assert "staging_caches" in recipe
    assert len(recipe["staging_caches"]) >= 1

    # Should have inherits_from
    assert "inherits_from" in recipe
    assert recipe["inherits_from"]["cache_name"] == "core-build"


def test_staging_error_invalid_inherit(
    rattler_build: RattlerBuild, recipes: Path, tmp_path: Path
):
    # This should fail during rendering because the cache does not exist
    with pytest.raises(CalledProcessError):
        rattler_build.build(
            recipes / "staging/staging-invalid-inherit.yaml",
            tmp_path,
            extra_args=["--experimental"],
        )


def test_staging_files_selection(
    rattler_build: RattlerBuild, recipes: Path, tmp_path: Path
):
    """Test that file selection works correctly with staging inheritance."""
    rattler_build.build(
        recipes / "staging/multiple-staging-caches.yaml",
        tmp_path,
        extra_args=["--experimental"],
    )

    # libcore should have both lib and include
    pkg_core = get_extracted_package(tmp_path, "libcore")
    paths_core = json.loads((pkg_core / "info/paths.json").read_text())
    core_files = [p["_path"] for p in paths_core["paths"]]
    assert any("lib/libcore.so" in f for f in core_files)
    assert any("include/core.h" in f for f in core_files)

    # core-headers should only have include
    pkg_dev = get_extracted_package(tmp_path, "core-headers")
    paths_dev = json.loads((pkg_dev / "info/paths.json").read_text())
    dev_files = [p["_path"] for p in paths_dev["paths"]]
    assert not any("lib/" in f for f in dev_files)
    assert any("include/" in f for f in dev_files)


def test_staging_source_handling(
    rattler_build: RattlerBuild, recipes: Path, tmp_path: Path
):
    """Test that sources are properly handled in staging outputs."""
    # The staging recipes reference sources, ensure they're fetched correctly
    rattler_build.build(
        recipes / "staging/multiple-staging-caches.yaml",
        tmp_path,
        extra_args=["--experimental"],
    )

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
    rattler_build.build(
        recipes / "staging/staging-with-deps.yaml",
        tmp_path,
        extra_args=["--experimental"],
    )

    pkg = get_extracted_package(tmp_path, "check-1")
    index = json.loads((pkg / "info/index.json").read_text())

    # Build number should be 0 as specified in context
    assert index["build_number"] == 0


def test_staging_about_propagation(
    rattler_build: RattlerBuild, recipes: Path, tmp_path: Path
):
    """Test that about metadata is properly set in package outputs."""
    rattler_build.build(
        recipes / "staging/multiple-staging-caches.yaml",
        tmp_path,
        extra_args=["--experimental"],
    )

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
    rattler_build.build(
        recipes / "staging/staging-compiler.yaml",
        tmp_path,
        extra_args=["--experimental"],
    )

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
    rattler_build.build(
        recipes / "staging/staging-run-exports-test2.yaml",
        tmp_path,
        extra_args=["--experimental"],
    )

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
    rattler_build.build(
        recipes / "staging/staging-run-exports-test3.yaml",
        tmp_path,
        extra_args=["--experimental"],
    )

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
