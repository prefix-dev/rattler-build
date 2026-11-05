import json
from pathlib import Path

from helpers import RattlerBuild


def test_fetch_sources(rattler_build: RattlerBuild, recipes: Path, tmp_path: Path):
    """`fetch-sources` extracts sources into the cache, reports their
    locations per output and does not apply patches or run the build."""
    recipe = recipes / "patch_with_strip"
    output_dir = tmp_path / "output"
    json_path = tmp_path / "sources.json"

    rattler_build(
        "fetch-sources",
        "--recipe",
        str(recipe),
        "--output-dir",
        str(output_dir),
        "--json-output",
        str(json_path),
    )

    [output] = json.loads(json_path.read_text())
    assert output["name"] == "patch_with_strip"
    src_cache = (output_dir / "src_cache").resolve()
    assert Path(output["source_cache"]).resolve() == src_cache

    [source] = output["sources"]
    assert source["kind"] == "path"
    assert [Path(p).name for p in source["patches"]] == [
        "0001-Patch-with-levels-to-strip.patch",
        "0002-Patch-without-additional-levels.patch",
    ]

    # the tarball is extracted into the source cache
    extracted = Path(source["path"])
    assert extracted.is_dir()
    assert src_cache in extracted.resolve().parents

    # nothing was built and no work directory was created
    assert not list(output_dir.glob("**/*.conda"))
    assert not list(output_dir.glob("**/work"))


def test_fetch_sources_stdout_and_output_name(
    rattler_build: RattlerBuild, recipes: Path, tmp_path: Path
):
    output = rattler_build(
        "fetch-sources",
        "--recipe",
        str(recipes / "patch_with_strip"),
        "--output-dir",
        str(tmp_path),
        "--output-name",
        "patch_with_strip",
    )
    [entry] = json.loads(output)
    assert entry["name"] == "patch_with_strip"

    result = rattler_build(
        "fetch-sources",
        "--recipe",
        str(recipes / "patch_with_strip"),
        "--output-dir",
        str(tmp_path),
        "--output-name",
        "does-not-exist",
        need_result_object=True,
    )
    assert result.returncode != 0
    assert "does-not-exist" in result.stderr
