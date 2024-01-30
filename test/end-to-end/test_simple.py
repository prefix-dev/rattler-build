import hashlib
import json
import os
import platform
from pathlib import Path
from subprocess import CalledProcessError, check_output
from typing import Any, Optional

import pytest
import requests
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

    def build(
        self,
        recipe_folder: Path,
        output_folder: Path,
        variant_config: Optional[Path] = None,
        custom_channels: list[str] | None = None,
        extra_args: list[str] = None,
    ):
        if extra_args is None:
            extra_args = []
        args = ["build", "--recipe", str(recipe_folder), *extra_args]
        if variant_config is not None:
            args += ["--variant-config", str(variant_config)]
        args += ["--output-dir", str(output_folder)]

        if custom_channels:
            for c in custom_channels:
                args += ["--channel", c]

        print(args)
        return self(*args)


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


def test_functionality(rattler_build: RattlerBuild):
    suffix = ".exe" if os.name == "nt" else ""
    text = rattler_build("--help").splitlines()
    assert text[0] == f"Usage: rattler-build{suffix} [OPTIONS] [COMMAND]"


@pytest.fixture
def recipes():
    return Path(__file__).parent.parent.parent / "test-data" / "recipes"


def get_package(folder: Path, glob="*.tar.bz2"):
    if "tar.bz2" not in glob:
        glob += "*.tar.bz2"
    if "/" not in glob:
        glob = "**/" + glob
    package_path = next(folder.glob(glob))
    return package_path


def get_extracted_package(folder: Path, glob="*.tar.bz2"):
    package_path = get_package(folder, glob)
    extract_path = folder / "extract"
    extract(str(package_path), dest_dir=str(extract_path))
    return extract_path


def test_license_glob(rattler_build: RattlerBuild, recipes: Path, tmp_path: Path):
    rattler_build.build(recipes / "globtest", tmp_path)
    pkg = get_extracted_package(tmp_path, "globtest")
    assert (pkg / "info/licenses/LICENSE").exists()
    # Random files we moved into the package license folder
    assert (pkg / "info/licenses/cmake/FindTBB.cmake").exists()
    assert (pkg / "info/licenses/docs/ghp_environment.yml").exists()
    assert (pkg / "info/licenses/docs/rtd_environment.yml").exists()

    # Check that the total number of files under the license folder is correct
    # 4 files + 2 folders = 6
    assert len(list(pkg.glob("info/licenses/**/*"))) == 6


def check_info(folder: Path, expected: Path):
    for f in ["index.json", "about.json", "link.json", "paths.json"]:
        assert (folder / "info" / f).exists()
        cmp = json.loads((expected / f).read_text())

        actual = json.loads((folder / "info" / f).read_text())
        if f == "index.json":
            # We need to remove the timestamp from the index.json
            cmp["timestamp"] = actual["timestamp"]

        if f == "paths.json":
            assert len(actual["paths"]) == len(cmp["paths"])

            for i, p in enumerate(actual["paths"]):
                c = cmp["paths"][i]
                assert c["_path"] == p["_path"]
                assert c["path_type"] == p["path_type"]
                if "dist-info" not in p["_path"]:
                    assert c["sha256"] == p["sha256"]
                    assert c["size_in_bytes"] == p["size_in_bytes"]
                assert c.get("no_link") is None
        else:
            if actual != cmp:
                print(f"Expected {f} to be {cmp} but was {actual}")
                raise AssertionError(f"Expected {f} to be {cmp} but was {actual}")


def test_python_noarch(rattler_build: RattlerBuild, recipes: Path, tmp_path: Path):
    rattler_build.build(recipes / "toml", tmp_path)
    pkg = get_extracted_package(tmp_path, "toml")

    assert (pkg / "info/licenses/LICENSE").exists()
    assert (pkg / "site-packages/toml-0.10.2.dist-info/INSTALLER").exists()
    installer = pkg / "site-packages/toml-0.10.2.dist-info/INSTALLER"
    assert installer.read_text().strip() == "conda"

    check_info(pkg, expected=recipes / "toml" / "expected")


def test_run_exports(rattler_build: RattlerBuild, recipes: Path, tmp_path: Path):
    rattler_build.build(recipes / "run_exports", tmp_path)
    pkg = get_extracted_package(tmp_path, "run_exports_test")

    assert (pkg / "info/run_exports.json").exists()
    actual_run_export = json.loads((pkg / "info/run_exports.json").read_text())
    assert set(actual_run_export.keys()) == {"weak"}
    assert len(actual_run_export["weak"]) == 1
    x = actual_run_export["weak"][0]
    assert x.startswith("run_exports_test ==1.0.0 h") and x.endswith("_0")


def host_subdir():
    """return conda subdir based on current platform"""
    plat = platform.system()
    if plat == "Linux":
        if platform.machine().endswith("aarch64"):
            return "linux-aarch64"
        return "linux-64"
    elif plat == "Darwin":
        if platform.machine().endswith("arm64"):
            return "osx-arm64"
        return "osx-64"
    elif plat == "Windows":
        return "win-64"
    else:
        raise RuntimeError("Unsupported platform")


def variant_hash(variant):
    hash_length = 7
    m = hashlib.sha1()
    m.update(json.dumps(variant, sort_keys=True).encode())
    return f"h{m.hexdigest()[:hash_length]}"


def test_pkg_hash(rattler_build: RattlerBuild, recipes: Path, tmp_path: Path):
    rattler_build.build(recipes / "pkg_hash", tmp_path)
    pkg = get_package(tmp_path, "pkg_hash")
    expected_hash = variant_hash({"target_platform": host_subdir()})
    assert pkg.name.endswith(f"pkg_hash-1.0.0-{expected_hash}_my_pkg.tar.bz2")


@pytest.mark.skipif(
    not os.environ.get("PREFIX_DEV_READ_ONLY_TOKEN", ""),
    reason="requires PREFIX_DEV_READ_ONLY_TOKEN",
)
def test_auth_file(
    rattler_build: RattlerBuild, recipes: Path, tmp_path: Path, monkeypatch
):
    auth_file = tmp_path / "auth.json"
    monkeypatch.setenv("RATTLER_AUTH_FILE", str(auth_file))

    with pytest.raises(CalledProcessError):
        rattler_build.build(
            recipes / "private-repository",
            tmp_path,
            custom_channels=["conda-forge", "https://repo.prefix.dev/setup-pixi-test"],
        )

    auth_file.write_text(
        json.dumps(
            {
                "repo.prefix.dev": {
                    "BearerToken": os.environ["PREFIX_DEV_READ_ONLY_TOKEN"]
                }
            }
        )
    )

    rattler_build.build(
        recipes / "private-repository",
        tmp_path,
        custom_channels=["conda-forge", "https://repo.prefix.dev/setup-pixi-test"],
    )


@pytest.mark.skipif(
    not os.environ.get("ANACONDA_ORG_TEST_TOKEN", ""),
    reason="requires ANACONDA_ORG_TEST_TOKEN",
)
def test_anaconda_upload(
    rattler_build: RattlerBuild, recipes: Path, tmp_path: Path, monkeypatch
):
    URL = "https://api.anaconda.org/package/rattler-build-testpackages/globtest"

    # Make sure the package doesn't exist
    requests.delete(
        URL, headers={"Authorization": f"token {os.environ['ANACONDA_ORG_TEST_TOKEN']}"}
    )

    assert requests.get(URL).status_code == 404

    monkeypatch.setenv("ANACONDA_API_KEY", os.environ["ANACONDA_ORG_TEST_TOKEN"])

    rattler_build.build(recipes / "globtest", tmp_path)

    rattler_build(
        "upload",
        "-vvv",
        "anaconda",
        "--owner",
        "rattler-build-testpackages",
        str(get_package(tmp_path, "globtest")),
    )

    # Make sure the package exists
    assert requests.get(URL).status_code == 200

    # Make sure the package attempted overwrites fail without --force
    with pytest.raises(CalledProcessError):
        rattler_build(
            "upload",
            "-vvv",
            "anaconda",
            "--owner",
            "rattler-build-testpackages",
            str(get_package(tmp_path, "globtest")),
        )

    # Make sure the package attempted overwrites succeed with --force
    rattler_build(
        "upload",
        "-vvv",
        "anaconda",
        "--owner",
        "rattler-build-testpackages",
        "--force",
        str(get_package(tmp_path, "globtest")),
    )

    assert requests.get(URL).status_code == 200


@pytest.mark.skipif(
    os.name == "nt", reason="recipe does not support execution on windows"
)
def test_cross_testing(
    rattler_build: RattlerBuild, recipes: Path, tmp_path: Path
) -> None:
    native_platform = host_subdir()
    if native_platform.startswith("linux"):
        target_platform = "osx-64"
    elif native_platform.startswith("osx"):
        target_platform = "linux-64"

    rattler_build.build(
        recipes / "test-execution/recipe-test-succeed.yaml",
        tmp_path,
        extra_args=["--target-platform", target_platform],
    )


def test_additional_entrypoints(
    rattler_build: RattlerBuild, recipes: Path, tmp_path: Path
):
    rattler_build.build(
        recipes / "entry_points/additional_entrypoints.yaml",
        tmp_path,
    )

    pkg = get_extracted_package(tmp_path, "additional_entrypoints")

    if os.name == "nt":
        assert (pkg / "Scripts/additional_entrypoints-script.py").exists()
        assert (pkg / "Scripts/additional_entrypoints.exe").exists()
    else:
        assert (pkg / "bin/additional_entrypoints").exists()


def test_always_copy_files(rattler_build: RattlerBuild, recipes: Path, tmp_path: Path):
    rattler_build.build(
        recipes / "always-copy-files/recipe.yaml",
        tmp_path,
    )

    pkg = get_extracted_package(tmp_path, "always_copy_files")

    assert (pkg / "info/paths.json").exists()
    paths = json.loads((pkg / "info/paths.json").read_text())
    assert len(paths["paths"]) == 1
    assert paths["paths"][0]["_path"] == "hello.txt"
    assert paths["paths"][0]["no_link"] is True


def test_always_include_files(
    rattler_build: RattlerBuild, recipes: Path, tmp_path: Path
):
    rattler_build.build(
        recipes / "always-include-files/recipe.yaml",
        tmp_path,
    )

    pkg = get_extracted_package(tmp_path, "force-include-base")

    assert (pkg / "info/paths.json").exists()
    paths = json.loads((pkg / "info/paths.json").read_text())
    assert len(paths["paths"]) == 1
    assert paths["paths"][0]["_path"] == "hello.txt"
    assert paths["paths"][0].get("no_link") is None

    assert "Hello, world!" in (pkg / "hello.txt").read_text()

    pkg_sanity = get_extracted_package(tmp_path, "force-include-sanity-check")
    paths = json.loads((pkg_sanity / "info/paths.json").read_text())
    assert len(paths["paths"]) == 0

    pkg_force = get_extracted_package(tmp_path, "force-include-forced")
    paths = json.loads((pkg_force / "info/paths.json").read_text())
    assert len(paths["paths"]) == 1
    assert paths["paths"][0]["_path"] == "hello.txt"
    assert paths["paths"][0].get("no_link") is None
    assert (pkg_force / "hello.txt").exists()
    assert "Force include new file" in (pkg_force / "hello.txt").read_text()


def test_script_env_in_recipe(
    rattler_build: RattlerBuild, recipes: Path, tmp_path: Path
):
    rattler_build.build(
        recipes / "script_env/recipe.yaml",
        tmp_path,
    )
    pkg = get_extracted_package(tmp_path, "script_env")

    assert (pkg / "info/paths.json").exists()
    content = (pkg / "hello.txt").read_text()
    # Windows adds quotes to the string so we just check with `in`
    assert "FOO is Hello World!" in content


def test_crazy_characters(rattler_build: RattlerBuild, recipes: Path, tmp_path: Path):
    rattler_build.build(
        recipes / "crazy_characters/recipe.yaml",
        tmp_path,
    )
    pkg = get_extracted_package(tmp_path, "crazy_characters")
    assert (pkg / "info/paths.json").exists()

    file_1 = pkg / "files" / "File(Glob …).tmSnippet"
    assert file_1.read_text() == file_1.name

    file_2 = (
        pkg / "files" / "a $random_crazy file name with spaces and (parentheses).txt"
    )
    assert file_2.read_text() == file_2.name

    file_3 = pkg / "files" / ("a_really_long_" + ("placeholder" * 20) + ".txt")
    assert file_3.read_text() == file_3.name
