"""End-to-end tests for melange-style subpackages (experimental)."""

import json
import os
from pathlib import Path
from subprocess import CalledProcessError

import pytest
from helpers import RattlerBuild, get_extracted_package


def test_subpackages_require_experimental(
    rattler_build: RattlerBuild, recipes: Path, tmp_path: Path
):
    """Subpackages are gated behind the --experimental flag."""
    with pytest.raises(CalledProcessError):
        rattler_build.build(recipes / "subpackages/recipe.yaml", tmp_path)


def test_subpackages_render(
    rattler_build: RattlerBuild, recipes: Path, tmp_path: Path
):
    """A subpackages recipe renders as a single output with embedded subpackages."""
    rendered = rattler_build.render(
        recipes / "subpackages/recipe.yaml",
        tmp_path,
        extra_args=["--experimental"],
    )

    # The output is a single recipe (one build); subpackages are attached to it,
    # not turned into separate outputs or a staging cache.
    assert len(rendered) == 1
    recipe = rendered[0]["recipe"]
    assert recipe["package"]["name"] == "mylib"
    assert not recipe.get("staging_caches")

    sub_names = sorted(s["package"]["name"] for s in recipe["subpackages"])
    assert sub_names == ["mylib-dev", "mylib-doc"]


@pytest.mark.skipif(os.name == "nt", reason="recipe build script is unix-only")
def test_subpackages_split_files(
    rattler_build: RattlerBuild, recipes: Path, tmp_path: Path
):
    """The build runs once and files are split across parent + subpackages."""
    rattler_build.build(
        recipes / "subpackages/recipe.yaml",
        tmp_path,
        extra_args=["--experimental"],
    )

    # Parent keeps the remainder (the shared library), not headers or man pages.
    pkg = get_extracted_package(tmp_path, "mylib")
    assert (pkg / "lib/libmylib.so").exists()
    assert not (pkg / "include/mylib.h").exists()
    assert not (pkg / "share/man/man1/mylib.1").exists()

    # -dev gets the headers only.
    pkg_dev = get_extracted_package(tmp_path, "mylib-dev")
    assert (pkg_dev / "include/mylib.h").exists()
    assert not (pkg_dev / "lib/libmylib.so").exists()

    # -doc gets the man pages only.
    pkg_doc = get_extracted_package(tmp_path, "mylib-doc")
    assert (pkg_doc / "share/man/man1/mylib.1").exists()
    assert not (pkg_doc / "include/mylib.h").exists()


@pytest.mark.skipif(os.name == "nt", reason="recipe build script is unix-only")
def test_subpackages_pin_subpackage(
    rattler_build: RattlerBuild, recipes: Path, tmp_path: Path
):
    """A subpackage can exactly pin its parent via pin_subpackage."""
    rattler_build.build(
        recipes / "subpackages/recipe.yaml",
        tmp_path,
        extra_args=["--experimental"],
    )

    pkg_dev = get_extracted_package(tmp_path, "mylib-dev")
    index = json.loads((pkg_dev / "info/index.json").read_text())
    depends = index.get("depends", [])

    # Exact pin resolves to `mylib ==1.0.0 <build_string>`.
    assert any(
        dep.startswith("mylib ") and "==1.0.0" in dep for dep in depends
    ), f"expected an exact pin on mylib, got {depends}"


@pytest.mark.skipif(os.name == "nt", reason="recipe build script is unix-only")
def test_subpackages_about_inheritance(
    rattler_build: RattlerBuild, recipes: Path, tmp_path: Path
):
    """Subpackage about inherits from the parent, with overrides taking effect."""
    rattler_build.build(
        recipes / "subpackages/recipe.yaml",
        tmp_path,
        extra_args=["--experimental"],
    )

    # -dev overrides the summary.
    pkg_dev = get_extracted_package(tmp_path, "mylib-dev")
    about_dev = json.loads((pkg_dev / "info/about.json").read_text())
    assert about_dev["summary"] == "Development files for mylib"

    # -doc inherits the parent summary and license.
    pkg_doc = get_extracted_package(tmp_path, "mylib-doc")
    about_doc = json.loads((pkg_doc / "info/about.json").read_text())
    assert about_doc["summary"] == "My library"
    assert about_doc["license"] == "MIT"


@pytest.mark.skipif(os.name == "nt", reason="recipe build script is unix-only")
def test_subpackages_overlap_and_internal_exclude(
    rattler_build: RattlerBuild, recipes: Path, tmp_path: Path
):
    """Files are partitioned on concrete paths: first-match-wins, and a
    subpackage's internal `exclude` lets files fall through to the parent."""
    rattler_build.build(
        recipes / "subpackages/recipe-overlap.yaml",
        tmp_path,
        extra_args=["--experimental"],
    )

    def files(name: str) -> set[str]:
        pkg = get_extracted_package(tmp_path, name)
        paths = json.loads((pkg / "info/paths.json").read_text())
        return {p["_path"] for p in paths["paths"]}

    # edge-lib includes lib/** but excludes lib/*.a -> only the shared lib.
    assert files("edge-lib") == {"lib/libfoo.so"}
    # edge-static claims the static lib that fell through edge-lib's exclude.
    assert files("edge-static") == {"lib/libfoo.a"}
    # The parent keeps the remainder (the static lib claimed by nobody).
    assert files("edge") == {"lib/keepme.a"}


if __name__ == "__main__":
    pytest.main([__file__, "-v"])
