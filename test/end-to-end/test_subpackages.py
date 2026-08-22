"""End-to-end coverage for packages split from a parent output."""

import json
import sys
from pathlib import Path

from helpers import RattlerBuild, get_extracted_package


def package_files(package: Path) -> set[str]:
    paths = json.loads((package / "info/paths.json").read_text())["paths"]
    return {entry["_path"] for entry in paths}


def command_output(result) -> str:
    return (result.stdout or "") + (result.stderr or "")


def test_nested_subpackages_partition_one_build(
    rattler_build: RattlerBuild, recipes: Path, tmp_path: Path
):
    result = rattler_build(
        *rattler_build.build_args(recipes / "subpackages", tmp_path),
        capture_output=True,
        text=True,
    )
    assert result.returncode == 0, command_output(result)
    output = command_output(result)
    # Windows CI currently sends rattler-build tracing directly to the console
    # instead of the subprocess pipes.
    if sys.platform != "win32":
        assert (
            output.count("Building new staging cache: __rattler_build_split_split-demo")
            == 1
        )

    parent = get_extracted_package(tmp_path, "split-demo-1.0.0-")
    development = get_extracted_package(tmp_path, "split-demo-dev-1.0.0-")
    documentation = get_extracted_package(tmp_path, "split-demo-docs-1.0.0-")

    parent_files = package_files(parent)
    development_files = package_files(development)
    documentation_files = package_files(documentation)

    assert "share/split-demo/runtime.txt" in parent_files
    identity = (parent / "share/split-demo/runtime.txt").read_text().split()
    parent_index = json.loads((parent / "info/index.json").read_text())
    assert identity[:3] == ["split-demo", "1.0.0", "0"]
    assert identity[3] == parent_index["build"]
    assert identity[4].startswith("h")
    assert "include/split-demo/demo.h" not in parent_files
    assert "lib/cmake/split-demo/SplitDemoConfig.cmake" not in parent_files
    assert "share/split-demo/doc/readme.txt" not in parent_files

    assert "include/split-demo/demo.h" in development_files
    assert "lib/cmake/split-demo/SplitDemoConfig.cmake" in development_files
    assert "share/split-demo/runtime.txt" not in development_files

    assert documentation_files >= {"share/split-demo/doc/readme.txt"}
    assert "share/split-demo/runtime.txt" not in documentation_files

    dev_index = json.loads((development / "info/index.json").read_text())
    assert any(
        dependency.startswith("split-demo ==1.0.0")
        for dependency in dev_index["depends"]
    )
    assert dev_index["name"] == "split-demo-dev"
    assert dev_index["build_number"] == 7

    dev_about = json.loads((development / "info/about.json").read_text())
    assert dev_about["summary"] == "Development files split from split-demo"
    assert dev_about["license"] == "BSD-3-Clause"


def test_building_only_a_child_uses_parent_package_identity(
    rattler_build: RattlerBuild, recipes: Path, tmp_path: Path
):
    result = rattler_build(
        *rattler_build.build_args(
            recipes / "subpackages",
            tmp_path,
            extra_args=["--up-to", "split-demo-docs"],
        ),
        capture_output=True,
        text=True,
    )
    assert result.returncode == 0, command_output(result)
    documentation = get_extracted_package(tmp_path, "split-demo-docs-1.0.0-")
    identity = (documentation / "share/split-demo/doc/readme.txt").read_text().split()
    assert identity[:3] == ["split-demo", "1.0.0", "0"]
    assert identity[3].startswith("h")
    assert identity[4].startswith("h")


def test_overlapping_subpackage_globs_fail(
    rattler_build: RattlerBuild, recipes: Path, tmp_path: Path
):
    result = rattler_build(
        *rattler_build.build_args(recipes / "subpackages-overlap", tmp_path),
        capture_output=True,
        text=True,
    )
    assert result.returncode != 0
    if sys.platform != "win32":
        assert "selected by more than one subpackage" in command_output(result)


def test_generated_subpackage_globs_cannot_escape_prefix(
    rattler_build: RattlerBuild, recipes: Path, tmp_path: Path
):
    result = rattler_build(
        *rattler_build.build_args(recipes / "subpackages-traversal", tmp_path),
        capture_output=True,
        text=True,
    )
    assert result.returncode != 0
    if sys.platform != "win32":
        output = command_output(result)
        assert "Invalid generated subpackage pattern" in output
        assert "prefix-relative" in output
