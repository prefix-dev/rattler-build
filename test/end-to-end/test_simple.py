import hashlib
import json
import os
import platform
import uuid
from dataclasses import dataclass, field
from pathlib import Path
from subprocess import DEVNULL, STDOUT, CalledProcessError, check_output
from typing import Iterator

import boto3
import pytest
import requests
import yaml
from helpers import RattlerBuild, check_build_output, get_extracted_package, get_package


def test_functionality(rattler_build: RattlerBuild):
    suffix = ".exe" if os.name == "nt" else ""
    text = rattler_build("--help").splitlines()
    assert text[0] == f"Usage: rattler-build{suffix} [OPTIONS] [COMMAND]"


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


def test_run_exports(
    rattler_build: RattlerBuild, recipes: Path, tmp_path: Path, snapshot_json
):
    rattler_build.build(recipes / "run_exports", tmp_path)
    pkg = get_extracted_package(tmp_path, "run_exports_test")

    assert (pkg / "info/run_exports.json").exists()
    actual_run_export = json.loads((pkg / "info/run_exports.json").read_text())
    assert set(actual_run_export.keys()) == {"weak"}
    assert len(actual_run_export["weak"]) == 1
    x = actual_run_export["weak"][0]
    assert x.startswith("run_exports_test ==1.0.0 h") and x.endswith("_0")

    assert (pkg / "info/index.json").exists()
    index_json = json.loads((pkg / "info/index.json").read_text())
    assert index_json.get("depends") is None

    rendered = rattler_build.render(
        recipes / "run_exports/multi_run_exports_list.yaml", tmp_path
    )
    assert rendered[0]["recipe"]["requirements"]["run_exports"] == {
        "weak": ["abc", "def"]
    }

    rendered = rattler_build.render(
        recipes / "run_exports/multi_run_exports_dict.yaml", tmp_path
    )
    assert rendered[0]["recipe"]["requirements"]["run_exports"] == snapshot_json


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
    rattler_build.build(recipes / "pkg_hash", tmp_path, extra_args=["--test=skip"])
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


@dataclass
class S3Config:
    access_key_id: str
    secret_access_key: str
    region: str = "auto"
    endpoint_url: str = (
        "https://e1a7cde76f1780ec06bac859036dbaf7.r2.cloudflarestorage.com"
    )
    bucket_name: str = "rattler-build-upload-test"
    channel_name: str = field(default_factory=lambda: f"channel{uuid.uuid4()}")


@pytest.fixture()
def s3_config() -> S3Config:
    access_key_id = os.environ.get("S3_ACCESS_KEY_ID")
    if not access_key_id:
        pytest.skip("S3_ACCESS_KEY_ID environment variable is not set")
    secret_access_key = os.environ.get("S3_SECRET_ACCESS_KEY")
    if not secret_access_key:
        pytest.skip("S3_SECRET_ACCESS_KEY environment variable is not set")
    return S3Config(
        access_key_id=access_key_id,
        secret_access_key=secret_access_key,
    )


@pytest.fixture()
def s3_client(s3_config: S3Config):
    return boto3.client(
        service_name="s3",
        endpoint_url=s3_config.endpoint_url,
        aws_access_key_id=s3_config.access_key_id,
        aws_secret_access_key=s3_config.secret_access_key,
        region_name=s3_config.region,
    )


@pytest.fixture()
def s3_channel(s3_config: S3Config, s3_client) -> Iterator[str]:
    channel_url = f"s3://{s3_config.bucket_name}/{s3_config.channel_name}"

    yield channel_url

    # Clean up the channel after the test
    objects_to_delete = s3_client.list_objects_v2(
        Bucket=s3_config.bucket_name, Prefix=f"{s3_config.channel_name}/"
    )
    delete_keys = [{"Key": obj["Key"]} for obj in objects_to_delete.get("Contents", [])]
    if delete_keys:
        result = s3_client.delete_objects(
            Bucket=s3_config.bucket_name, Delete={"Objects": delete_keys}
        )
        assert result["ResponseMetadata"]["HTTPStatusCode"] == 200


@pytest.fixture()
def s3_credentials_file(
    tmp_path: Path,
    s3_config: S3Config,
    s3_channel: str,
) -> Path:
    path = tmp_path / "credentials.json"
    path.write_text(
        f"""\
{{
    "{s3_channel}": {{
        "S3Credentials": {{
            "access_key_id": "{s3_config.access_key_id}",
            "secret_access_key": "{s3_config.secret_access_key}"
        }}
    }}
}}"""
    )
    return path


def test_s3_minio_upload(
    rattler_build: RattlerBuild,
    recipes: Path,
    tmp_path: Path,
    s3_credentials_file: Path,
    s3_config: S3Config,
    s3_channel: str,
    s3_client,
    monkeypatch,
):
    monkeypatch.setenv("RATTLER_AUTH_FILE", str(s3_credentials_file))
    rattler_build.build(recipes / "globtest", tmp_path)
    cmd = [
        "upload",
        "-vvv",
        "s3",
        "--channel",
        s3_channel,
        "--region",
        s3_config.region,
        "--endpoint-url",
        s3_config.endpoint_url,
        "--force-path-style",
        str(get_package(tmp_path, "globtest")),
    ]
    rattler_build(*cmd)

    # Check if package was correctly uploaded
    package_key = f"{s3_config.channel_name}/{host_subdir()}/globtest-0.24.6-{variant_hash({'target_platform': host_subdir()})}_0.tar.bz2"
    result = s3_client.head_object(
        Bucket=s3_config.bucket_name,
        Key=package_key,
    )
    assert result["ResponseMetadata"]["HTTPStatusCode"] == 200

    # Raise an error if the same package is uploaded again
    with pytest.raises(CalledProcessError):
        rattler_build(*cmd)


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
    pyc_paths = [p["_path"] for p in paths["paths"] if p["_path"].endswith(".pyc")]
    assert len(pyc_paths) == 3
    assert "just_a_.cpython-311.pyc" in pyc_paths
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
    assert isinstance(index["track_features"], str)
    assert (
        index["track_features"]
        == "down_prioritize-p-0 down_prioritize-p-1 down_prioritize-p-2 down_prioritize-p-3"
    )


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
        elif path == "is_text/file_with_forwardslash_prefix":
            assert "\\" not in p["prefix_placeholder"]
            assert "/" in p["prefix_placeholder"]
            check_path(p, "text")


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
def test_git_submodule(
    rattler_build: RattlerBuild, recipes: Path, tmp_path: Path, snapshot_json
):
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
    assert snapshot_json == rendered_recipe["finalized_sources"]


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
def test_symlink_recipe(
    rattler_build: RattlerBuild, recipes: Path, tmp_path: Path, snapshot_json
):
    path_to_recipe = recipes / "symlink"
    args = rattler_build.build_args(
        path_to_recipe,
        tmp_path,
    )

    rattler_build(*args)

    pkg = get_extracted_package(tmp_path, "symlink")
    assert snapshot_json == json.loads((pkg / "info/paths.json").read_text())


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
    assert rendered[0]["recipe"]["build"]["string"] == "unix_5600cae_0"

    assert rendered[0]["build_configuration"]["variant"] == {
        "__unix": "__unix",
        "target_platform": "noarch",
    }

    pin = {
        "pin_subpackage": {
            "name": "rattler-build-demo",
            "exact": True,
        }
    }
    assert rendered[1]["recipe"]["build"]["string"] == "unix_63d9094_0"
    assert rendered[1]["recipe"]["build"]["noarch"] == "generic"
    assert rendered[1]["recipe"]["requirements"]["run"] == [pin]
    assert rendered[1]["build_configuration"]["variant"] == {
        "rattler_build_demo": "1 unix_5600cae_0",
        "target_platform": "noarch",
    }

    output = rattler_build(
        *args, "--target-platform=win-64", "--render-only", stderr=DEVNULL
    )
    rendered = json.loads(output)
    assert len(rendered) == 2

    assert rendered[0]["recipe"]["requirements"]["run"] == ["__win >=11.0.123 foobar"]
    assert rendered[0]["recipe"]["build"]["string"] == "win_19aa286_0"

    assert rendered[0]["build_configuration"]["variant"] == {
        "__win": "__win >=11.0.123 foobar",
        "target_platform": "noarch",
    }

    pin = {
        "pin_subpackage": {
            "name": "rattler-build-demo",
            "exact": True,
        }
    }
    assert rendered[1]["recipe"]["build"]["string"] == "win_95d38b2_0"
    assert rendered[1]["recipe"]["build"]["noarch"] == "generic"
    assert rendered[1]["recipe"]["requirements"]["run"] == [pin]
    assert rendered[1]["build_configuration"]["variant"] == {
        "rattler_build_demo": "1 win_19aa286_0",
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
        'target_compile_definitions(test PRIVATE "some_path;$ENV{CONDA_BUILD_SYSROOT}/and/more;some_other_path;$ENV{CONDA_BUILD_SYSROOT}/and/more")'  # noqa: E501
    )


@pytest.mark.skipif(
    os.name == "nt", reason="recipe does not support execution on windows"
)
def test_filter_files(
    rattler_build: RattlerBuild, recipes: Path, tmp_path: Path, snapshot_json
):
    path_to_recipe = recipes / "filter_files"
    args = rattler_build.build_args(
        path_to_recipe,
        tmp_path,
    )

    rattler_build(*args)
    pkg = get_extracted_package(tmp_path, "filter_files")

    assert snapshot_json == json.loads((pkg / "info/paths.json").read_text())


def test_double_license(rattler_build: RattlerBuild, recipes: Path, tmp_path: Path):
    path_to_recipe = recipes / "double_license"
    args = rattler_build.build_args(path_to_recipe, tmp_path)
    output = rattler_build(*args, stderr=STDOUT)
    assert "warning License file from source directory was overwritten" in output


@pytest.mark.skipif(
    os.name == "nt", reason="recipe does not support execution on windows"
)
def test_post_link(
    rattler_build: RattlerBuild, recipes: Path, tmp_path: Path, snapshot_json
):
    path_to_recipe = recipes / "post-link"
    args = rattler_build.build_args(
        path_to_recipe,
        tmp_path,
    )
    rattler_build(*args)

    pkg = get_extracted_package(tmp_path, "postlink")
    assert snapshot_json == json.loads((pkg / "info/paths.json").read_text())


@pytest.mark.skipif(
    os.name == "nt", reason="recipe does not support execution on windows"
)
def test_include_files(
    rattler_build: RattlerBuild, recipes: Path, tmp_path: Path, snapshot_json
):
    path_to_recipe = recipes / "include_files"
    args = rattler_build.build_args(
        path_to_recipe,
        tmp_path,
    )
    rattler_build(*args)

    pkg = get_extracted_package(tmp_path, "include_files")
    assert snapshot_json == json.loads((pkg / "info/paths.json").read_text())


def test_nushell_implicit_recipe(
    rattler_build: RattlerBuild, recipes: Path, tmp_path: Path
):
    rattler_build.build(
        recipes / "nushell-implicit/recipe.yaml",
        tmp_path,
    )
    pkg = get_extracted_package(tmp_path, "nushell")

    assert (pkg / "info/paths.json").exists()
    content = (pkg / "hello.txt").read_text()
    assert "Hello, world!" == content


def test_channel_specific(rattler_build: RattlerBuild, recipes: Path, tmp_path: Path):
    rattler_build.build(
        recipes / "channel_specific/recipe.yaml",
        tmp_path,
        extra_args="-c conda-forge -c quantstack".split(),
    )
    pkg = get_extracted_package(tmp_path, "channel_specific")

    assert (pkg / "info/recipe/rendered_recipe.yaml").exists()
    # load yaml
    text = (pkg / "info/recipe/rendered_recipe.yaml").read_text()
    rendered_recipe = yaml.safe_load(text)
    print(text)
    deps = rendered_recipe["finalized_dependencies"]["host"]["resolved"]

    for d in deps:
        if d["name"] == "sphinx":
            assert d["channel"] == "https://conda.anaconda.org/quantstack/"


def test_run_exports_from(
    rattler_build: RattlerBuild, recipes: Path, tmp_path: Path, snapshot_json
):
    rattler_build.build(
        recipes / "run_exports_from",
        tmp_path,
    )
    pkg = get_extracted_package(tmp_path, "run_exports_test")

    assert (pkg / "info/run_exports.json").exists()

    actual_run_export = json.loads((pkg / "info/run_exports.json").read_text())
    assert set(actual_run_export.keys()) == {"weak"}
    assert len(actual_run_export["weak"]) == 1
    x = actual_run_export["weak"][0]
    assert x.startswith("run_exports_test ==1.0.0 h") and x.endswith("_0")

    index_json = json.loads((pkg / "info/index.json").read_text())
    assert index_json.get("depends") is None


def test_script_execution(rattler_build: RattlerBuild, recipes: Path, tmp_path: Path):
    rattler_build.build(
        recipes / "script",
        tmp_path,
    )
    pkg = get_extracted_package(tmp_path, "script-test")

    # grab paths.json
    paths = json.loads((pkg / "info/paths.json").read_text())
    assert len(paths["paths"]) == 1
    assert paths["paths"][0]["_path"] == "script-executed.txt"

    rattler_build.build(
        recipes / "script/recipe_with_extensions.yaml",
        tmp_path,
    )
    pkg = get_extracted_package(tmp_path, "script-test-ext")

    # grab paths.json
    paths = json.loads((pkg / "info/paths.json").read_text())
    assert len(paths["paths"]) == 1
    assert paths["paths"][0]["_path"] == "script-executed.txt"


def test_noarch_flask(
    rattler_build: RattlerBuild, recipes: Path, tmp_path: Path, snapshot
):
    rattler_build.build(
        recipes / "flask",
        tmp_path,
    )
    pkg = get_extracted_package(tmp_path, "flask")

    # this is to ensure that the clone happens correctly
    license_file = pkg / "info/licenses/LICENSE.rst"
    assert license_file.exists()

    assert (pkg / "info/tests/tests.yaml").exists()

    # check that the snapshot matches
    test_yaml = (pkg / "info/tests/tests.yaml").read_text()
    assert test_yaml == snapshot

    # make sure that the entry point does not exist
    assert not (pkg / "python-scripts/flask").exists()

    assert (pkg / "info/link.json").exists()


def test_downstream_test(
    rattler_build: RattlerBuild, recipes: Path, tmp_path: Path, snapshot_json
):
    rattler_build.build(
        recipes / "downstream_test/succeed.yaml",
        tmp_path,
    )

    pkg = next(tmp_path.rglob("**/upstream-good-*"))
    test_result = rattler_build.test(pkg, "-c", str(tmp_path))

    assert "Running downstream test for package: downstream-good" in test_result
    assert "Downstream test could not run" not in test_result
    assert "Running test in downstream package" in test_result

    rattler_build.build(
        recipes / "downstream_test/fail_prelim.yaml",
        tmp_path,
    )

    with pytest.raises(CalledProcessError) as e:
        rattler_build.build(
            recipes / "downstream_test/fail.yaml",
            tmp_path,
        )

        assert "│ Failing test in downstream package" in e.value.output
        assert "│ Downstream test failed" in e.value.output


def test_cache_runexports(
    rattler_build: RattlerBuild, recipes: Path, tmp_path: Path, snapshot_json
):
    rattler_build.build(recipes / "cache_run_exports/helper.yaml", tmp_path)
    rattler_build.build(
        recipes / "cache_run_exports/recipe_test_1.yaml",
        tmp_path,
        extra_args=["--experimental"],
    )

    pkg = get_extracted_package(tmp_path, "cache-run-exports")

    assert (pkg / "info/index.json").exists()
    index = json.loads((pkg / "info/index.json").read_text())
    assert index["depends"] == ["normal-run-exports"]

    pkg = get_extracted_package(tmp_path, "no-cache-by-name-run-exports")
    assert (pkg / "info/index.json").exists()
    index = json.loads((pkg / "info/index.json").read_text())
    assert index["name"] == "no-cache-by-name-run-exports"
    assert index.get("depends", []) == []

    pkg = get_extracted_package(tmp_path, "no-cache-from-package-run-exports")
    assert (pkg / "info/index.json").exists()
    index = json.loads((pkg / "info/index.json").read_text())
    assert index["name"] == "no-cache-from-package-run-exports"
    print(index)
    assert index.get("depends", []) == []

    rattler_build.build(
        recipes / "cache_run_exports/recipe_test_2.yaml",
        tmp_path,
        extra_args=["--experimental"],
    )
    pkg = get_extracted_package(tmp_path, "cache-ignore-run-exports")
    index = json.loads((pkg / "info/index.json").read_text())
    assert index["name"] == "cache-ignore-run-exports"
    assert index.get("depends", []) == []

    rattler_build.build(
        recipes / "cache_run_exports/recipe_test_3.yaml",
        tmp_path,
        extra_args=["--experimental"],
    )
    pkg = get_extracted_package(tmp_path, "cache-ignore-run-exports-by-name")
    index = json.loads((pkg / "info/index.json").read_text())
    assert index["name"] == "cache-ignore-run-exports-by-name"
    assert index.get("depends", []) == []


def test_extra_meta_is_recorded_into_about_json(
    rattler_build: RattlerBuild, recipes: Path, tmp_path: Path, snapshot_json
):
    rattler_build.build(
        recipes / "toml",
        tmp_path,
        extra_meta={"flow_run_id": "some_id", "sha": "24ee3"},
    )
    pkg = get_extracted_package(tmp_path, "toml")

    about_json = json.loads((pkg / "info/about.json").read_text())

    assert snapshot_json == about_json


def test_used_vars(rattler_build: RattlerBuild, recipes: Path, tmp_path: Path):
    args = rattler_build.build_args(
        recipes / "used-vars/recipe_1.yaml",
        tmp_path,
    )

    output = rattler_build(
        *args, "--target-platform=linux-64", "--render-only", stderr=DEVNULL
    )

    rendered = json.loads(output)
    assert len(rendered) == 1
    assert rendered[0]["build_configuration"]["variant"] == {
        "target_platform": "noarch"
    }


def test_cache_install(
    rattler_build: RattlerBuild, recipes: Path, tmp_path: Path, snapshot_json
):
    rattler_build.build(
        recipes / "cache/recipe-cmake.yaml", tmp_path, extra_args=["--experimental"]
    )

    pkg1 = get_extracted_package(tmp_path, "check-1")
    pkg2 = get_extracted_package(tmp_path, "check-2")
    assert (pkg1 / "info/index.json").exists()
    assert (pkg2 / "info/index.json").exists()


def test_env_vars_override(rattler_build: RattlerBuild, recipes: Path, tmp_path: Path):
    rattler_build.build(
        recipes / "env_vars",
        tmp_path,
    )

    pkg = get_extracted_package(tmp_path, "env_var_test")

    # assert (pkg / "info/paths.json").exists()
    text = (pkg / "makeflags.txt").read_text()
    assert text.strip() == "OVERRIDDEN_MAKEFLAGS"

    variant_config = json.loads((pkg / "info/hash_input.json").read_text())
    assert variant_config["MAKEFLAGS"] == "OVERRIDDEN_MAKEFLAGS"

    text = (pkg / "pybind_abi.txt").read_text()
    assert text.strip() == "4"
    assert variant_config["pybind11_abi"] == 4

    # Check that we used the variant in the rendered recipe
    rendered_recipe = yaml.safe_load(
        (pkg / "info/recipe/rendered_recipe.yaml").read_text()
    )
    assert rendered_recipe["finalized_dependencies"]["build"]["specs"][0] == {
        "variant": "pybind11-abi",
        "spec": "pybind11-abi 4.*",
    }


def test_pin_subpackage(
    rattler_build: RattlerBuild, recipes: Path, tmp_path: Path, snapshot_json
):
    rattler_build.build(
        recipes / "pin_subpackage",
        tmp_path,
    )
    pkg = get_extracted_package(tmp_path, "my.package-a")
    assert (pkg / "info/index.json").exists()


def test_testing_strategy(
    rattler_build: RattlerBuild,
    recipes: Path,
    tmp_path: Path,
    capfd,
):
    # --test=skip
    check_build_output(
        rattler_build,
        capfd,
        recipe_path=recipes / "test_strategy" / "recipe.yaml",
        output_path=tmp_path,
        extra_args=["--test=skip"],
        string_to_check="Skipping tests because the argument --test=skip was set",
    )

    # --test=native
    check_build_output(
        rattler_build,
        capfd,
        recipe_path=recipes / "test_strategy" / "recipe.yaml",
        output_path=tmp_path,
        extra_args=["--test=native"],
        string_to_check="all tests passed!",
    )

    # --test=native and cross-compiling
    check_build_output(
        rattler_build,
        capfd,
        recipe_path=recipes / "test_strategy" / "recipe.yaml",
        output_path=tmp_path,
        extra_args=[
            "--test=native",
            "--target-platform=linux-64",
            "--build-platform=osx-64",
        ],
        string_to_check="Skipping tests because the argument "
        "--test=native was set and the build is a cross-compilation",
    )

    # --test=native-and-emulated
    check_build_output(
        rattler_build,
        capfd,
        recipe_path=recipes / "test_strategy" / "recipe.yaml",
        output_path=tmp_path,
        extra_args=["--test=native-and-emulated"],
        string_to_check="all tests passed!",
    )

    #  --test=native-and-emulated and cross-compiling
    check_build_output(
        rattler_build,
        capfd,
        recipe_path=recipes / "test_strategy" / "recipe.yaml",
        output_path=tmp_path,
        extra_args=[
            "--test=native-and-emulated",
            "--target-platform=linux-64",
            "--build-platform=osx-64",
        ],
        string_to_check="all tests passed!",
    )

    # --test=native and cross-compiling and noarch
    check_build_output(
        rattler_build,
        capfd,
        recipe_path=recipes / "test_strategy" / "recipe-noarch.yaml",
        output_path=tmp_path,
        extra_args=[
            "--test=native",
            "--target-platform=linux-64",
            "--build-platform=osx-64",
        ],
        string_to_check="all tests passed!",
    )


def test_pin_compatible(
    rattler_build: RattlerBuild, recipes: Path, tmp_path: Path, snapshot_json
):
    rendered = rattler_build.render(recipes / "pin_compatible", tmp_path)

    assert snapshot_json == rendered[0]["recipe"]["requirements"]


def test_render_variants(rattler_build: RattlerBuild, recipes: Path, tmp_path: Path):
    rendered = rattler_build.render(
        recipes / "race-condition/recipe-undefined-variant.yaml", tmp_path
    )
    assert [rx["recipe"]["package"]["name"] for rx in rendered] == [
        "my-package-a",
        "my-package-b",
    ]


def test_race_condition(rattler_build: RattlerBuild, recipes: Path, tmp_path: Path):
    # make sure that tests are ran in the right order and that the packages are built correctly
    rattler_build.build(recipes / "race-condition", tmp_path)


def test_variant_sorting(rattler_build: RattlerBuild, recipes: Path, tmp_path: Path):
    # make sure that tests are ran in the right order and that the packages are built correctly
    rendered = rattler_build.render(
        recipes / "race-condition" / "recipe-pin-subpackage.yaml", tmp_path
    )
    assert [rx["recipe"]["package"]["name"] for rx in rendered] == ["test1", "test2"]


def test_missing_pin_subpackage(
    rattler_build: RattlerBuild, recipes: Path, tmp_path: Path
):
    # make sure that tests are ran in the right order and that the packages are built correctly
    with pytest.raises(CalledProcessError) as e:
        rattler_build.render(
            recipes / "race-condition" / "recipe-pin-invalid.yaml",
            tmp_path,
            stderr=STDOUT,
        )
    stdout = e.value.output.decode("utf-8")
    assert "Missing output: test1 (used in pin_subpackage)" in stdout


def test_cycle_detection(rattler_build: RattlerBuild, recipes: Path, tmp_path: Path):
    # make sure that tests are ran in the right order and that the packages are built correctly
    with pytest.raises(CalledProcessError) as e:
        rattler_build.render(
            recipes / "race-condition" / "recipe-cycle.yaml",
            tmp_path,
            stderr=STDOUT,
        )
    stdout = e.value.output.decode("utf-8")
    assert "Found a cycle in the recipe outputs: bazbus" in stdout


def test_python_min_render(
    rattler_build: RattlerBuild, recipes: Path, tmp_path: Path, snapshot_json
):
    rendered = rattler_build.render(
        recipes / "race-condition" / "recipe-python-min.yaml", tmp_path
    )

    assert snapshot_json == rendered[0]["recipe"]["requirements"]


def test_recipe_variant_render(
    rattler_build: RattlerBuild, recipes: Path, tmp_path: Path, snapshot_json
):
    rendered = rattler_build.render(
        recipes / "recipe_variant" / "recipe.yaml", tmp_path, "--with-solve"
    )

    assert snapshot_json == [output["recipe"]["requirements"] for output in rendered]
    assert snapshot_json == [
        (
            output["finalized_dependencies"]["build"]["specs"],
            output["finalized_dependencies"]["run"],
        )
        for output in rendered
    ]


@pytest.mark.skipif(
    os.name == "nt", reason="recipe does not support execution on windows"
)
def test_cache_select_files(rattler_build: RattlerBuild, recipes: Path, tmp_path: Path):
    rattler_build.build(
        recipes / "cache/recipe-compiler.yaml", tmp_path, extra_args=["--experimental"]
    )
    pkg = get_extracted_package(tmp_path, "testlib-so-version")

    assert (pkg / "info/paths.json").exists()
    paths = json.loads((pkg / "info/paths.json").read_text())

    assert len(paths["paths"]) == 2
    assert paths["paths"][0]["_path"] == "lib/libdav1d.so.7"
    assert paths["paths"][0]["path_type"] == "softlink"
    assert paths["paths"][1]["_path"] == "lib/libdav1d.so.7.0.0"
    assert paths["paths"][1]["path_type"] == "hardlink"


@pytest.mark.skipif(
    os.name == "nt", reason="recipe does not support execution on windows"
)
def test_abi3(rattler_build: RattlerBuild, recipes: Path, tmp_path: Path):
    rattler_build.build(recipes / "abi3", tmp_path)
    pkg = get_extracted_package(tmp_path, "python-abi3-package-sample")

    assert (pkg / "info/paths.json").exists()
    paths = json.loads((pkg / "info/paths.json").read_text())
    # ensure that all paths start with `site-packages`
    for p in paths["paths"]:
        assert p["_path"].startswith("site-packages")

    actual_paths = [p["_path"] for p in paths["paths"]]
    if os.name == "nt":
        assert "site-packages\\spam.dll" in actual_paths
    else:
        assert "site-packages/spam.abi3.so" in actual_paths

    # load index.json
    index = json.loads((pkg / "info/index.json").read_text())
    assert index["name"] == "python-abi3-package-sample"
    assert index["noarch"] == "python"
    assert index["subdir"] == host_subdir()
    assert index["platform"] == host_subdir().split("-")[0]


# This is how cf-scripts is using rattler-build - rendering recipes from stdin
def test_rendering_from_stdin(
    rattler_build: RattlerBuild, recipes: Path, tmp_path: Path
):
    text = (recipes / "abi3" / "recipe.yaml").read_text()
    # variants = recipes / "abi3" / "variants.yaml" "-m", variants
    rendered = rattler_build("build", "--render-only", input=text, text=True)
    loaded = json.loads(rendered)

    assert loaded[0]["recipe"]["package"]["name"] == "python-abi3-package-sample"


def test_jinja_types(
    rattler_build: RattlerBuild, recipes: Path, tmp_path: Path, snapshot_json
):
    # render only and snapshot json
    rendered = rattler_build.render(
        recipes / "jinja-types", tmp_path, extra_args=["--experimental"]
    )
    print(rendered)
    # load as json
    assert snapshot_json == rendered[0]["recipe"]["context"]
    variant = rendered[0]["build_configuration"]["variant"]
    # remove target_platform from the variant
    variant.pop("target_platform")
    assert snapshot_json == variant
