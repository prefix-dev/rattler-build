import os
import sys
from pathlib import Path

import pytest
from helpers import RattlerBuild
from syrupy.extensions.json import JSONSnapshotExtension


@pytest.fixture
def clean_path_on_win32():
    # On Windows, clear path to avoid hitting the cmd.exe
    # line-length limit during VS compiler activation (vcvars64.bat).
    if sys.platform == "win32":
        original_path = os.environ.get("PATH", "")
        try:
            os.environ["PATH"] = ""
            yield
        finally:
            os.environ["PATH"] = original_path


def pytest_configure(config):
    # On Windows, use a short absolute path to avoid hitting the cmd.exe
    # line-length limit during VS compiler activation (vcvars64.bat).
    if sys.platform == "win32":
        worker_id = os.environ.get("PYTEST_XDIST_WORKER", "bld").replace("gw", "")
        config.option.basetemp = Path(f"C:/{worker_id}")


@pytest.fixture
def rattler_build():
    if os.environ.get("RATTLER_BUILD_PATH"):
        return RattlerBuild(os.environ["RATTLER_BUILD_PATH"])
    else:
        base_path = Path(__file__).parent.parent.parent
        executable_name = "rattler-build"
        if os.name == "nt":
            executable_name += ".exe"

        # Check multiple possible locations for the binary
        possible_paths = [
            base_path / f"target/release/{executable_name}",
            base_path / f"target/debug/{executable_name}",
            base_path / f"target-pixi/release/{executable_name}",
            base_path / f"target-pixi/debug/{executable_name}",
        ]

        # Use the most recently modified binary
        candidates = []
        for path in possible_paths:
            if path.exists():
                candidates.append((path, path.stat().st_mtime))

        if candidates:
            # Sort by modification time (newest first) and return the most recent
            candidates.sort(key=lambda x: x[1], reverse=True)
            return RattlerBuild(candidates[0][0])

    raise FileNotFoundError("Could not find rattler-build executable")


@pytest.fixture
def snapshot_json(snapshot):
    return snapshot.use_extension(JSONSnapshotExtension)


@pytest.fixture
def recipes():
    return Path(__file__).parent.parent.parent / "test-data" / "recipes"
