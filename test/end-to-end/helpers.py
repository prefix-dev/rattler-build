import json
from pathlib import Path
from subprocess import STDOUT, CalledProcessError, check_output
from typing import Any, Optional
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
        output = self(*args, "--render-only")

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
