import os
from pathlib import Path

import pytest
from helpers import RattlerBuild
from syrupy.extensions.json import JSONSnapshotExtension


@pytest.fixture
def rattler_build():
    if os.environ.get("RATTLER_BUILD_PATH"):
        return RattlerBuild(os.environ["RATTLER_BUILD_PATH"])
    else:
        base_path = Path(__file__).parent.parent.parent
        executable_name = "rattler-build"
        if os.name == "nt":
            executable_name += ".exe"

        release_path = base_path / f"target/release/{executable_name}"
        debug_path = base_path / f"target/debug/{executable_name}"

        if release_path.exists():
            return RattlerBuild(release_path)
        elif debug_path.exists():
            return RattlerBuild(debug_path)

    raise FileNotFoundError("Could not find rattler-build executable")


@pytest.fixture
def snapshot_json(snapshot):
    return snapshot.use_extension(JSONSnapshotExtension)


@pytest.fixture
def recipes():
    return Path(__file__).parent.parent.parent / "test-data" / "recipes"
