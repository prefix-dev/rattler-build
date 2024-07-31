import json
import os
from pathlib import Path

import pytest
from helpers import RattlerBuild, get_extracted_package


@pytest.mark.skipif(
    os.name == "nt", reason="recipe does not support execution on windows"
)
def test_symlink_cache(
    rattler_build: RattlerBuild, recipes: Path, tmp_path: Path, snapshot_json
):
    rattler_build.build(
        recipes / "cache/recipe-symlinks.yaml", tmp_path, extra_args=["--experimental"]
    )

    pkg = get_extracted_package(tmp_path, "absolute-cache-symlinks")
    assert pkg.exists()
    link_file = pkg / "absolute-symlink.txt"
    assert link_file.is_symlink()
    # assure that this is a relative link
    assert link_file.readlink() == Path("foo.txt")

    link_target = link_file.resolve()
    assert link_target == (pkg / "foo.txt")

    link_file = pkg / "bin/absolute-exe-symlink"
    assert link_file.is_symlink()
    assert link_file.readlink() == Path("exe")

    pkg = get_extracted_package(tmp_path, "cache-symlinks")

    paths_json = pkg / "info/paths.json"
    j = json.loads(paths_json.read_text())
    assert snapshot_json == j

    paths = j["paths"]
    assert len(paths) == 5
    for p in paths:
        if "symlink" in p["_path"]:
            assert p["path_type"] == "softlink"
            assert (
                p["sha256"]
                == "e3b0c44298fc1c149afbf4c8996fb92427ae41e4649b934ca495991b7852b855"
            )

    foo_symlink = pkg / "foo-symlink.txt"
    assert foo_symlink.exists()
    assert foo_symlink.is_symlink()
    assert not foo_symlink.readlink().is_absolute()

    broken_symlink = pkg / "broken-symlink.txt"
    assert not broken_symlink.exists()

    relative_symlink = pkg / "relative-symlink.txt"
    assert relative_symlink.is_symlink()
    assert relative_symlink.readlink() == Path("foo.txt")

    relative_symlink = pkg / "bin/exe-symlink"
    assert relative_symlink.is_symlink()
    assert relative_symlink.readlink() == Path("exe")
