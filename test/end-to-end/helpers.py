import json
from pathlib import Path
from subprocess import STDOUT, CalledProcessError, check_output, run
from typing import Any, Optional
from conda_package_handling.api import extract


class RattlerBuild:
    def __init__(self, path):
        self.path = path

    def __call__(self, *args: Any, **kwds: Any) -> Any:
        # Check if we need to return a result object with returncode
        needs_result_object = "capture_output" in kwds or kwds.get(
            "need_result_object", False
        )

        if needs_result_object or any("create-patch" in str(arg) for arg in args):
            # Use subprocess.run for commands that need result object
            kwds_copy = dict(kwds)
            kwds_copy.pop("need_result_object", None)
            if "capture_output" not in kwds_copy:
                kwds_copy["capture_output"] = True
            if "text" not in kwds_copy:
                kwds_copy["text"] = True

            result = run([str(self.path), *args], **kwds_copy)
            return result
        else:
            try:
                output = check_output([str(self.path), *args], **kwds)
                if "text" not in kwds:
                    return output.decode("utf-8")
                return output
            except CalledProcessError as e:
                if kwds.get("stderr") is None:
                    print(e.output)
                    print(e.stderr)
                raise e

    def build_args(
        self,
        recipe_folder: Path,
        output_folder: Path,
        variant_config: Optional[Path] = None,
        custom_channels: Optional[list[str]] = None,
        extra_args: list[str] = None,
        extra_meta: dict[str, Any] = None,
    ):
        if extra_args is None:
            extra_args = []
        args = ["build", "--recipe", str(recipe_folder), *extra_args]
        if variant_config is not None:
            args += ["--variant-config", str(variant_config)]
        args += ["--output-dir", str(output_folder)]
        args += ["--package-format", str("tar.bz2")]
        if extra_meta:
            args += [
                item
                for k, v in (extra_meta or {}).items()
                for item in ["--extra-meta", f"{k}={v}"]
            ]

        if custom_channels:
            for c in custom_channels:
                args += ["--channel", c]

        return args

    def build(
        self,
        recipe_folder: Path,
        output_folder: Path,
        variant_config: Optional[Path] = None,
        custom_channels: Optional[list[str]] = None,
        extra_args: list[str] = None,
        extra_meta: dict[str, Any] = None,
    ):
        args = self.build_args(
            recipe_folder,
            output_folder,
            variant_config=variant_config,
            custom_channels=custom_channels,
            extra_args=extra_args,
            extra_meta=extra_meta,
        )
        return self(*args)

    def test(self, package, *args: Any, **kwds: Any) -> Any:
        return self("test", "--package-file", package, *args, stderr=STDOUT, **kwds)

    def render(
        self,
        recipe_folder: Path,
        output_folder: Path,
        with_solve: bool = False,
        variant_config: Optional[Path] = None,
        custom_channels: Optional[list[str]] = None,
        extra_args: list[str] = None,
        extra_meta: dict[str, Any] = None,
        **kwargs: Any,
    ) -> Any:
        args = self.build_args(
            recipe_folder,
            output_folder,
            variant_config=variant_config,
            custom_channels=custom_channels,
            extra_args=extra_args,
            extra_meta=extra_meta,
        )
        if with_solve:
            args += ["--with-solve"]
        output = self(*args, "--render-only", **kwargs)
        print(output)
        return json.loads(output)


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


def setup_patch_test_environment(
    tmp_path: Path,
    test_name: str,
    cache_files: Optional[dict[str, str]] = None,
    work_files: Optional[dict[str, str]] = None,
    recipe_content: str = "package:\n  name: dummy\n",
    source_url: str = "https://example.com/example.tar.gz",
    source_sha256: str = "0123456789abcdef0123456789abcdef0123456789abcdef0123456789abcdef",
) -> dict[str, Path]:
    cache_dir = tmp_path / test_name / "cache"
    work_dir = tmp_path / test_name / "work"
    recipe_dir = tmp_path / test_name / "recipe"

    cache_dir.mkdir(parents=True, exist_ok=True)
    work_dir.mkdir(parents=True, exist_ok=True)
    recipe_dir.mkdir(parents=True, exist_ok=True)

    orig_dir_name = "example_01234567"
    orig_dir = cache_dir / orig_dir_name
    orig_dir.mkdir(parents=True, exist_ok=True)

    if cache_files:
        for filename, content in cache_files.items():
            (orig_dir / filename).write_text(content)

    if work_files:
        for filename, content in work_files.items():
            (work_dir / filename).write_text(content)

    recipe_path = recipe_dir / "recipe.yaml"
    recipe_path.write_text(recipe_content)

    source_info = {
        "recipe_path": str(recipe_path),
        "source_cache": str(cache_dir),
        "sources": [{"url": source_url, "sha256": source_sha256}],
    }
    (work_dir / ".source_info.json").write_text(json.dumps(source_info))

    return {
        "cache_dir": cache_dir,
        "work_dir": work_dir,
        "recipe_dir": recipe_dir,
        "recipe_path": recipe_path,
    }


def check_build_output(
    rattler_build: RattlerBuild,
    capfd,
    recipe_path,
    output_path,
    extra_args: list,
    string_to_check: str,
):
    """Run a build and check the output for a specific string."""

    rattler_build.build(recipe_path, output_path, extra_args=extra_args)
    _, err = capfd.readouterr()
    print(err)  # to debug in case it fails
    assert string_to_check in err
