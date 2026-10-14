import json
import os
from pathlib import Path

import pytest
from conda_package_handling.api import extract
from helpers import RattlerBuild


def write_manifest(tmp_path: Path, **overrides) -> Path:
    src = tmp_path / "src"
    src.mkdir()
    (src / "greeting.txt").write_text("hello from package create\n")
    (src / "greet.sh").write_text("#!/bin/sh\necho hi\n")
    manifest = {
        "name": "greet",
        "version": "1.0.0",
        "build_string": "h1234_0",
        "target_platform": "noarch",
        "noarch": "generic",
        "depends": ["python >=3.10"],
        "license": "MIT",
        "summary": "Created from a JSON manifest",
        "timestamp": 1672531200000,
        "files": [
            {"src": str(src / "greeting.txt"), "dest": "share/greet/greeting.txt"},
            {"src": str(src / "greet.sh"), "dest": "bin/greet", "executable": True},
        ],
        "symlinks": [{"dest": "bin/hi", "target": "greet"}],
    }
    manifest.update(overrides)
    path = tmp_path / "manifest.json"
    path.write_text(json.dumps(manifest))
    return path


def test_package_create(rattler_build: RattlerBuild, tmp_path: Path):
    manifest = write_manifest(tmp_path)
    out = tmp_path / "out"
    rattler_build(
        "package", "create", "--manifest", str(manifest), "--output-dir", str(out)
    )

    package = out / "greet-1.0.0-h1234_0.conda"
    assert package.exists()

    pkg = tmp_path / "extracted"
    extract(str(package), dest_dir=str(pkg))
    assert (
        pkg / "share/greet/greeting.txt"
    ).read_text() == "hello from package create\n"
    if os.name != "nt":
        assert os.access(pkg / "bin/greet", os.X_OK)
        assert (pkg / "bin/hi").is_symlink()

    index = json.loads((pkg / "info/index.json").read_text())
    assert index["name"] == "greet"
    assert index["build"] == "h1234_0"
    assert index["noarch"] == "generic"
    assert index["depends"] == ["python >=3.10"]

    about = json.loads((pkg / "info/about.json").read_text())
    assert about["summary"] == "Created from a JSON manifest"


def test_package_create_is_reproducible(rattler_build: RattlerBuild, tmp_path: Path):
    manifest = write_manifest(tmp_path)
    for out in ("a", "b"):
        rattler_build(
            "package",
            "create",
            "--manifest",
            str(manifest),
            "--output-dir",
            str(tmp_path / out),
        )
    a = (tmp_path / "a" / "greet-1.0.0-h1234_0.conda").read_bytes()
    b = (tmp_path / "b" / "greet-1.0.0-h1234_0.conda").read_bytes()
    assert a == b


@pytest.mark.parametrize(
    "overrides",
    [
        {"files": [{"src": "/dev/null", "dest": "../escape"}]},
        {"files": [{"src": "/dev/null", "dest": "info/index.json"}]},
        {"unknown_field": True},
    ],
    ids=["outside-package", "info-dir", "unknown-field"],
)
def test_package_create_rejects_invalid_manifests(
    rattler_build: RattlerBuild, tmp_path: Path, overrides
):
    manifest = write_manifest(tmp_path, **overrides)
    result = rattler_build(
        "package",
        "create",
        "--manifest",
        str(manifest),
        "--output-dir",
        str(tmp_path / "out"),
        capture_output=True,
    )
    assert result.returncode != 0
