"""Tests for fetching recipe sources without building."""

from __future__ import annotations

import tarfile
from pathlib import Path

from rattler_build import Stage0Recipe, VariantConfig
from rattler_build.render import fetch_rendered_variant_sources


def _write_tarball(recipe_dir: Path) -> None:
    payload = recipe_dir / "payload"
    payload.mkdir()
    (payload / "somefile").write_text("abc\n")
    with tarfile.open(recipe_dir / "src.tar.gz", "w:gz") as tar:
        tar.add(payload, arcname="pkg-1.0")


def test_fetch_sources_does_not_build_or_patch(tmp_path: Path) -> None:
    recipe_dir = tmp_path / "recipe"
    recipe_dir.mkdir()
    _write_tarball(recipe_dir)
    (recipe_dir / "fix.patch").write_text("this is not applied\n")

    (recipe_dir / "recipe.yaml").write_text(
        """
package:
  name: fetch-sources-test
  version: 1.0.0

source:
  path: ./src.tar.gz
  patches:
    - fix.patch
  target_directory: sub

build:
  script:
    - exit 1
""".lstrip()
    )

    recipe = Stage0Recipe.from_file(recipe_dir / "recipe.yaml")
    rendered = recipe.render(VariantConfig())
    output_dir = tmp_path / "output"

    fetched = rendered[0].fetch_sources(output_dir=output_dir)

    assert fetched["name"] == "fetch-sources-test"
    assert fetched["version"] == "1.0.0"
    assert Path(fetched["source_cache"]) == output_dir / "src_cache"
    assert "staging" not in fetched

    [source] = fetched["sources"]
    assert source["kind"] == "path"
    assert source["target_directory"] == "sub"
    assert [Path(p).name for p in source["patches"]] == ["fix.patch"]

    # The archive is extracted into the source cache, unpatched.
    extracted = Path(source["path"])
    assert extracted.is_dir()
    assert extracted.is_relative_to(output_dir / "src_cache")
    assert (extracted / "somefile").read_text() == "abc\n"


def test_fetch_sources_includes_staging_sources(tmp_path: Path) -> None:
    recipe_dir = tmp_path / "recipe"
    recipe_dir.mkdir()
    _write_tarball(recipe_dir)
    (recipe_dir / "tool.txt").write_text("tool\n")

    (recipe_dir / "recipe.yaml").write_text(
        """
schema_version: 1

recipe:
  name: multi
  version: 1.0.0

outputs:
  - staging:
      name: stage
    source:
      path: ./src.tar.gz
    build:
      script:
        - exit 1

  - package:
      name: libmulti
    inherit: stage

  - package:
      name: multi-tools
    source:
      path: ./tool.txt
""".lstrip()
    )

    recipe = Stage0Recipe.from_file(recipe_dir / "recipe.yaml")
    rendered = recipe.render(VariantConfig())

    fetched = fetch_rendered_variant_sources(rendered, output_dir=tmp_path / "output")
    by_name = {f["name"]: f for f in fetched}

    assert set(by_name) == {"libmulti", "multi-tools"}

    lib = by_name["libmulti"]
    assert lib["sources"] == []
    assert lib["staging"]["name"] == "stage"
    [staging_source] = lib["staging"]["sources"]
    assert (Path(staging_source["path"]) / "somefile").exists()

    tools = by_name["multi-tools"]
    [tool_source] = tools["sources"]
    assert Path(tool_source["path"]) == (recipe_dir / "tool.txt").resolve()
