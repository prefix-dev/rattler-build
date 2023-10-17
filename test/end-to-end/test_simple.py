from typing import Any, Optional
import pytest
import os
from pathlib import Path
from subprocess import CalledProcessError, check_output
from conda_package_handling.api import extract

class RattlerBuild:
    def __init__(self, path):
        self.path = path
    
    def __call__(self, *args: Any, **kwds: Any) -> Any:
        
        try:
            return check_output([str(self.path), *args], **kwds).decode("utf-8")
        except CalledProcessError as e:
            print(e.output)
            print(e.stderr)
            raise e

    def build(self, recipe_folder: Path, output_folder: Path, variant_config: Optional[Path] = None):
        args = ["build", "--recipe", str(recipe_folder)]
        if variant_config is not None:
            args += ["--variant-config", str(variant_config)]
        args += ["--output-dir", str(output_folder)]
        print(args)
        return self(*args)

@pytest.fixture
def rattler_build():
    if os.environ.get("RATTLER_BUILD_PATH"):
        return RattlerBuild(os.environ["RATTLER_BUILD_PATH"])
    else:
        base_path = Path(__file__).parent.parent.parent
        # use the default target release path, then debug
        if (base_path / "target/release/rattler-build").exists():
            return RattlerBuild((base_path / "target/release/rattler-build"))
        elif (base_path / "target/debug/rattler-build").exists():
            return RattlerBuild((base_path / "target/debug/rattler-build"))

    raise FileNotFoundError("Could not find rattler-build executable")

def test_functionality(rattler_build: RattlerBuild):
    assert rattler_build("--help").startswith("Usage: rattler-build [OPTIONS]")

@pytest.fixture
def recipes():
    return Path(__file__).parent.parent.parent / "test-data" / "recipes"

def get_package(folder: Path, glob = "*.tar.bz2"):
    if 'tar.bz2' not in glob:
        glob += "*.tar.bz2"
    if '/' not in glob:
        glob = "**/" + glob
    package_path = next(folder.glob(glob))

    extract_path = folder / "extract"
    extract(str(package_path), dest_dir=str(extract_path))
    return extract_path


def test_license_glob(rattler_build: RattlerBuild, recipes: Path, tmp_path: Path):
    out = rattler_build.build(recipes / "globtest", tmp_path)
    pkg = get_package(tmp_path, "globtest")
    assert (pkg / "info/licenses/LICENSE").exists()
    # Random files we moved into the package license folder
    assert (pkg / "info/licenses/cmake/FindTBB.cmake").exists()
    assert (pkg / "info/licenses/docs/ghp_environment.yml").exists()
    assert (pkg / "info/licenses/docs/rtd_environment.yml").exists()

    # Check that the total number of files under the license folder is correct
    # 4 files + 2 folders = 6
    assert len(list(pkg.glob("info/licenses/**/*"))) == 6
