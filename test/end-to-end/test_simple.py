import hashlib
import json
import os
import platform
from pathlib import Path
from subprocess import DEVNULL, STDOUT, CalledProcessError, check_output
from typing import Any, Optional

import pytest
import requests
import yaml
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

    def build_args(
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
        args += ["--package-format", str("tar.bz2")]

        if custom_channels:
            for c in custom_channels:
                args += ["--channel", c]

        return args

    def build(
        self,
        recipe_folder: Path,
        output_folder: Path,
        variant_config: Optional[Path] = None,
        custom_channels: list[str] | None = None,
        extra_args: list[str] = None,
    ):
        args = self.build_args(
            recipe_folder,
            output_folder,
            variant_config=variant_config,
            custom_channels=custom_channels,
            extra_args=extra_args,
        )
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

    if package_path.name.endswith(".tar.bz2"):
        package_without_extension = package_path.name[: -len(".tar.bz2")]
    elif package_path.name.endswith(".conda"):
        package_without_extension = package_path.name[: -len(".conda")]

    extract_path = folder / "extract" / package_without_extension
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
    rattler_build.build(recipes / "pkg_hash", tmp_path, extra_args=["--no-test"])
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

    pkg = get_extracted_package(tmp_path, "test-execution")

    assert (pkg / "info/paths.json").exists()
    # make sure that the recipe is renamed to `recipe.yaml` in the package
    assert (pkg / "info/recipe/recipe.yaml").exists()


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

    # limit on Windows is 260 chars
    file_3 = pkg / "files" / ("a_really_long_" + ("a" * 200) + ".txt")
    assert file_3.read_text() == file_3.name


def test_variant_config(rattler_build: RattlerBuild, recipes: Path, tmp_path: Path):
    rattler_build.build(
        recipes / "variant_config/recipe.yaml",
        tmp_path,
        variant_config=recipes / "variant_config/variant_config.yaml",
    )
    v1 = get_extracted_package(tmp_path, "bla-0.1.0-h2c65b68_0")
    v2 = get_extracted_package(tmp_path, "bla-0.1.0-h48a45df_0")

    assert (v1 / "info/paths.json").exists()
    assert (v2 / "info/paths.json").exists()

    assert (v1 / "info/hash_input.json").exists()
    assert (v2 / "info/hash_input.json").exists()
    print(v1)
    print(v2)
    print((v1 / "info/hash_input.json").read_text())
    print((v2 / "info/hash_input.json").read_text())

    hash_input = json.loads((v1 / "info/hash_input.json").read_text())
    assert hash_input["some_option"] == "DEF"
    hash_input = json.loads((v2 / "info/hash_input.json").read_text())
    assert hash_input["some_option"] == "ABC"


def test_compile_python(rattler_build: RattlerBuild, recipes: Path, tmp_path: Path):
    rattler_build.build(
        recipes / "python_compilation/recipe.yaml",
        tmp_path,
    )

    pkg = get_extracted_package(tmp_path, "python_compilation")

    assert (pkg / "info/paths.json").exists()
    paths = json.loads((pkg / "info/paths.json").read_text())
    assert (
        len([p for p in paths["paths"] if p["_path"].endswith(".cpython-311.pyc")]) == 2
    )
    assert len([p for p in paths["paths"] if p["_path"].endswith(".py")]) == 4

    # make sure that we include the `info/recipe/recipe.py` file
    py_files = list(pkg.glob("**/*.py"))
    assert len(py_files) == 5


def test_down_prioritize(rattler_build: RattlerBuild, recipes: Path, tmp_path: Path):
    rattler_build.build(
        recipes / "down_prioritize/recipe.yaml",
        tmp_path,
    )

    pkg = get_extracted_package(tmp_path, "down_prioritize")

    assert (pkg / "info/index.json").exists()
    index = json.loads((pkg / "info/index.json").read_text())
    assert len(index["track_features"]) == 4
    assert index["track_features"][0] == "down_prioritize-p-0"


def test_prefix_detection(rattler_build: RattlerBuild, recipes: Path, tmp_path: Path):
    rattler_build.build(
        recipes / "prefix_detection/recipe.yaml",
        tmp_path,
    )

    pkg = get_extracted_package(tmp_path, "prefix_detection")

    assert (pkg / "info/index.json").exists()
    assert (pkg / "info/paths.json").exists()

    index_json = json.loads((pkg / "info/index.json").read_text())
    subdir = index_json["subdir"]
    is_win = subdir.startswith("win")

    def check_path(p, t):
        if t == "binary" and is_win or t is None:
            assert "file_mode" not in p
            assert "prefix_placeholder" not in p
        else:
            assert p["file_mode"] == t
            assert len(p["prefix_placeholder"]) > 10

    paths = json.loads((pkg / "info/paths.json").read_text())
    for p in paths["paths"]:
        path = p["_path"]
        if path == "is_binary/file_with_prefix":
            check_path(p, "binary")
        elif path == "is_text/file_with_prefix":
            check_path(p, "text")
        elif path == "is_binary/file_without_prefix":
            check_path(p, None)
        elif path == "is_text/file_without_prefix":
            check_path(p, None)
        elif path == "force_text/file_with_prefix":
            if not is_win:
                check_path(p, "text")
            else:
                check_path(p, None)
        elif path == "force_text/file_without_prefix":
            check_path(p, None)
        elif path == "force_binary/file_with_prefix":
            check_path(p, "binary")
        elif path == "force_binary/file_without_prefix":
            check_path(p, None)
        elif path == "ignore/file_with_prefix":
            check_path(p, None)
        elif path == "ignore/text_with_prefix":
            check_path(p, None)


def test_empty_folder(rattler_build: RattlerBuild, recipes: Path, tmp_path: Path):
    path_to_recipe = recipes / "empty_folder"
    (path_to_recipe / "empty_folder_in_recipe").mkdir(parents=True, exist_ok=True)

    rattler_build.build(
        recipes / "empty_folder/recipe.yaml",
        tmp_path,
    )

    pkg = get_extracted_package(tmp_path, "empty_folder")

    assert (pkg / "info/index.json").exists()
    assert (pkg / "info/recipe/empty_folder_in_recipe").exists()
    assert (pkg / "info/recipe/empty_folder_in_recipe").is_dir()

    assert not (pkg / "empty_folder").exists()
    assert not (pkg / "empty_folder").is_dir()

    # read paths json
    paths = json.loads((pkg / "info/paths.json").read_text())
    assert len(paths["paths"]) == 0


@pytest.mark.skipif(
    os.name == "nt", reason="recipe does not support execution on windows"
)
def test_console_logging(rattler_build: RattlerBuild, recipes: Path, tmp_path: Path):
    path_to_recipe = recipes / "console_logging"
    os.environ["SECRET"] = "hahaha"
    args = rattler_build.build_args(
        path_to_recipe,
        tmp_path,
    )

    output = check_output([str(rattler_build.path), *args], stderr=STDOUT, text=True)
    assert "hahaha" not in output
    assert "I am hahaha" not in output
    assert "I am ********" in output


@pytest.mark.skipif(
    os.name == "nt", reason="recipe does not support execution on windows"
)
def test_git_submodule(rattler_build: RattlerBuild, recipes: Path, tmp_path: Path):
    path_to_recipe = recipes / "git_source_submodule"
    args = rattler_build.build_args(
        path_to_recipe,
        tmp_path,
    )

    _ = check_output([str(rattler_build.path), *args], stderr=STDOUT, text=True)
    pkg = get_extracted_package(tmp_path, "nanobind")

    assert (pkg / "info/paths.json").exists()
    assert (pkg / "info/recipe/rendered_recipe.yaml").exists()
    # load recipe as YAML

    text = (pkg / "info/recipe/rendered_recipe.yaml").read_text()

    # parse the rendered recipe
    rendered_recipe = yaml.safe_load(text)
    sources = rendered_recipe["finalized_sources"]

    assert len(sources) == 1
    source = sources[0]
    assert source["git"] == "https://github.com/wjakob/nanobind/"
    assert source["rev"] == "8e1f8408b37d994fb987440859eb977af39be8c3"


@pytest.mark.skipif(
    os.name == "nt", reason="recipe does not support execution on windows"
)
def test_git_patch(rattler_build: RattlerBuild, recipes: Path, tmp_path: Path):
    path_to_recipe = recipes / "git_source_patch"
    args = rattler_build.build_args(
        path_to_recipe,
        tmp_path,
    )

    _ = check_output([str(rattler_build.path), *args], stderr=STDOUT, text=True)
    pkg = get_extracted_package(tmp_path, "ament_package")

    assert (pkg / "info/paths.json").exists()
    assert (pkg / "info/recipe/rendered_recipe.yaml").exists()
    # load recipe as YAML

    text = (pkg / "info/recipe/rendered_recipe.yaml").read_text()

    # parse the rendered recipe
    rendered_recipe = yaml.safe_load(text)
    sources = rendered_recipe["finalized_sources"]

    assert len(sources) == 1
    source = sources[0]
    assert source["git"] == "https://github.com/ros2-gbp/ament_package-release.git"
    assert source["rev"] == "00da147b17c19bc225408dc693ed8fdc14c314ab"


@pytest.mark.skipif(
    os.name == "nt", reason="recipe does not support execution on windows"
)
def test_patch_strip_level(rattler_build: RattlerBuild, recipes: Path, tmp_path: Path):
    path_to_recipe = recipes / "patch_with_strip"
    args = rattler_build.build_args(
        path_to_recipe,
        tmp_path,
    )

    _ = check_output([str(rattler_build.path), *args], stderr=STDOUT, text=True)
    pkg = get_extracted_package(tmp_path, "patch_with_strip")

    assert (pkg / "info/paths.json").exists()
    assert (pkg / "info/recipe/rendered_recipe.yaml").exists()

    text = (pkg / "somefile").read_text()

    assert text == "123\n"


@pytest.mark.skipif(
    os.name == "nt", reason="recipe does not support execution on windows"
)
def test_symlink_recipe(rattler_build: RattlerBuild, recipes: Path, tmp_path: Path):
    path_to_recipe = recipes / "symlink"
    args = rattler_build.build_args(
        path_to_recipe,
        tmp_path,
    )

    rattler_build(*args)
    pkg = get_extracted_package(tmp_path, "symlink")

    assert (pkg / "info/paths.json").exists()
    # parse paths.json
    paths = json.loads((pkg / "info/paths.json").read_text())
    pp = paths["paths"]
    assert len(pp) == 3

    for p in paths["paths"]:
        if p["_path"] == "bin/symlink-to-lib":
            assert p["path_type"] == "softlink"
            assert (
                p["sha256"]
                == "e3b0c44298fc1c149afbf4c8996fb92427ae41e4649b934ca495991b7852b855"
            )
        if p["_path"] == "bin/symlink":
            assert p["path_type"] == "softlink"
            assert (
                p["sha256"]
                == "f2ca1bb6c7e907d06dafe4687e579fce76b37e4e93b7605022da52e6ccc26fd2"
            )
        if p["_path"] == "lib/symlink/symlink-target":
            assert p["path_type"] == "hardlink"
            assert (
                p["sha256"]
                == "f2ca1bb6c7e907d06dafe4687e579fce76b37e4e93b7605022da52e6ccc26fd2"
            )


@pytest.mark.skipif(
    os.name == "nt", reason="recipe does not support execution on windows"
)
def test_read_only_removal(rattler_build: RattlerBuild, recipes: Path, tmp_path: Path):
    path_to_recipe = recipes / "read_only_build_files"
    args = rattler_build.build_args(
        path_to_recipe,
        tmp_path,
    )

    rattler_build(*args)
    pkg = get_extracted_package(tmp_path, "read-only-build-files")

    assert (pkg / "info/index.json").exists()


def test_noarch_variants(rattler_build: RattlerBuild, recipes: Path, tmp_path: Path):
    path_to_recipe = recipes / "noarch_variant"
    args = rattler_build.build_args(
        path_to_recipe,
        tmp_path,
    )

    output = rattler_build(
        *args, "--target-platform=linux-64", "--render-only", stderr=DEVNULL
    )

    # parse as json
    rendered = json.loads(output)
    assert len(rendered) == 2

    assert rendered[0]["recipe"]["requirements"]["run"] == ["__unix"]
    assert rendered[0]["recipe"]["requirements"]["run"] == ["__unix"]
    assert rendered[0]["recipe"]["build"]["string"] == "unix_4616a5c_0"

    pin = {
        "pin_subpackage": {
            "name": "rattler-build-demo",
            "exact": True,
        }
    }
    assert rendered[1]["recipe"]["build"]["string"] == "unix_2233755_0"
    assert rendered[1]["recipe"]["build"]["noarch"] == "generic"
    assert rendered[1]["recipe"]["requirements"]["run"] == [pin]
    assert rendered[1]["build_configuration"]["variant"] == {
        "rattler-build-demo": "1 unix_4616a5c_0",
        "target_platform": "noarch",
    }

    output = rattler_build(
        *args, "--target-platform=win-64", "--render-only", stderr=DEVNULL
    )
    rendered = json.loads(output)
    assert len(rendered) == 2

    assert rendered[0]["recipe"]["requirements"]["run"] == ["__win"]
    assert rendered[0]["recipe"]["requirements"]["run"] == ["__win"]
    assert rendered[0]["recipe"]["build"]["string"] == "win_4616a5c_0"

    pin = {
        "pin_subpackage": {
            "name": "rattler-build-demo",
            "exact": True,
        }
    }
    assert rendered[1]["recipe"]["build"]["string"] == "win_b28fc4d_0"
    assert rendered[1]["recipe"]["build"]["noarch"] == "generic"
    assert rendered[1]["recipe"]["requirements"]["run"] == [pin]
    assert rendered[1]["build_configuration"]["variant"] == {
        "rattler-build-demo": "1 win_4616a5c_0",
        "target_platform": "noarch",
    }


def test_regex_post_process(rattler_build: RattlerBuild, recipes: Path, tmp_path: Path):
    path_to_recipe = recipes / "regex_post_process"
    args = rattler_build.build_args(
        path_to_recipe,
        tmp_path,
    )

    _ = rattler_build(*args)

    pkg = get_extracted_package(tmp_path, "regex-post-process")

    assert (pkg / "info/paths.json").exists()

    test_text = (pkg / "test.txt").read_text().splitlines()
    assert test_text[0] == "Building the regex-post-process-replaced package"
    assert test_text[1] == "Do not replace /some/path/to/sysroot/and/more this"

    text_pc = (pkg / "test.pc").read_text().splitlines()
    expect_begin = "I am a test file with $(CONDA_BUILD_SYSROOT_S)and/some/more"
    expect_end = "and: $(CONDA_BUILD_SYSROOT_S)and/some/more"
    assert text_pc[0] == expect_begin
    assert text_pc[2] == expect_end

    text_cmake = (pkg / "test.cmake").read_text()
    assert text_cmake.startswith(
        'target_compile_definitions(test PRIVATE "some_path;{CONDA_BUILD_SYSROOT}/and/more;some_other_path;{CONDA_BUILD_SYSROOT}/and/more")'  # noqa: E501
    )


@pytest.mark.skipif(
    os.name == "nt", reason="recipe does not support execution on windows"
)
def test_filter_files(rattler_build: RattlerBuild, recipes: Path, tmp_path: Path):
    path_to_recipe = recipes / "filter_files"
    args = rattler_build.build_args(
        path_to_recipe,
        tmp_path,
    )

    rattler_build(*args)
    pkg = get_extracted_package(tmp_path, "filter_files")

    assert (pkg / "info/paths.json").exists()

    # parse paths json
    paths = json.loads((pkg / "info/paths.json").read_text())
    pp = paths["paths"]
    assert len(pp) == 1
    assert pp[0]["_path"] == "exists.txt"


def test_double_license(rattler_build: RattlerBuild, recipes: Path, tmp_path: Path):
    path_to_recipe = recipes / "double_license"
    args = rattler_build.build_args(
        path_to_recipe,
        tmp_path,
    )
    # make sure that two license files in $SRC_DIR and $RECIPE_DIR raise an exception
    with pytest.raises(CalledProcessError):
        rattler_build(*args)


@pytest.mark.skipif(
    os.name == "nt", reason="recipe does not support execution on windows"
)
def test_post_link(rattler_build: RattlerBuild, recipes: Path, tmp_path: Path):
    path_to_recipe = recipes / "post-link"
    args = rattler_build.build_args(
        path_to_recipe,
        tmp_path,
    )
    rattler_build(*args)

    pkg = get_extracted_package(tmp_path, "postlink")

    paths = json.loads((pkg / "info/paths.json").read_text())
    pp = paths["paths"]
    assert len(pp) == 1
    assert pp[0]["_path"] == "bin/.postlink-post-link.sh"
