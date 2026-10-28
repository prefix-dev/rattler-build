import os
import platform
import shutil
import uuid
from pathlib import Path
from subprocess import CalledProcessError

import pytest
from helpers import RattlerBuild

pytestmark = pytest.mark.skipif(
    platform.system() not in ("Linux", "Darwin"),
    reason="the sandbox is only available on Linux and macOS",
)


@pytest.fixture
def outside_dir():
    # The default sandbox policy leaves the temporary directories writable, so
    # the directory a script must not reach lives under the home directory.
    path = Path.home() / f".rattler-build-sandbox-test-{uuid.uuid4().hex}"
    path.mkdir()
    try:
        yield path
    finally:
        shutil.rmtree(path, ignore_errors=True)


def build(
    rattler_build: RattlerBuild,
    recipe: Path,
    output: Path,
    outside: Path,
    sandbox: bool,
):
    args = rattler_build.build_args(
        recipe, output, extra_args=["--sandbox"] if sandbox else []
    )
    env = {**os.environ, "RATTLER_BUILD_TEST_OUTSIDE": str(outside)}
    return rattler_build(*args, env=env)


def test_sandbox_blocks_the_build_script_outside_the_build_directory(
    rattler_build: RattlerBuild, recipes: Path, tmp_path: Path, outside_dir: Path
):
    recipe = recipes / "sandbox" / "build_writes_outside"
    marker = outside_dir / "build-marker.txt"

    build(rattler_build, recipe, tmp_path / "unsandboxed", outside_dir, sandbox=False)
    assert marker.read_text().strip() == "outside"
    marker.unlink()

    with pytest.raises(CalledProcessError):
        build(rattler_build, recipe, tmp_path / "sandboxed", outside_dir, sandbox=True)
    assert not marker.exists()


def test_sandbox_blocks_the_package_test_outside_the_test_directory(
    rattler_build: RattlerBuild, recipes: Path, tmp_path: Path, outside_dir: Path
):
    recipe = recipes / "sandbox" / "test_writes_outside"
    marker = outside_dir / "test-marker.txt"

    build(rattler_build, recipe, tmp_path / "unsandboxed", outside_dir, sandbox=False)
    assert marker.read_text().strip() == "outside"
    marker.unlink()

    with pytest.raises(CalledProcessError):
        build(rattler_build, recipe, tmp_path / "sandboxed", outside_dir, sandbox=True)
    assert not marker.exists()
    # The build script itself was allowed to run: only the test phase failed.
    assert next((tmp_path / "sandboxed").glob("**/inside.txt")).exists()
