"""Small pyproject.toml -> rattler-build metadata provider.

Supports the static PEP 621 subset and Poetry metadata used by Rich 14.2.0.
It demonstrates the provider protocol; it is not a general dependency converter.
"""

from __future__ import annotations

import json
import os
from pathlib import Path
import re
import tomllib

from packaging.requirements import Requirement
from packaging.version import Version


SOURCE = Path(os.environ["SRC_DIR"])
PYPROJECT = SOURCE / "pyproject.toml"

DEFAULT_PYPI_TO_CONDA = {
    "markdown-it-py": "markdown-it-py",
    "poetry-core": "poetry-core",
    "pygments": "pygments",
}


def normalized_name(name: str) -> str:
    return re.sub(r"[-_.]+", "-", name).lower()


def compatible_release(version: str) -> str:
    """Translate the basic Poetry caret ranges used by this example."""
    parsed = Version(version)
    release = parsed.release
    if release[0] != 0:
        upper = str(release[0] + 1)
    elif len(release) > 1 and release[1] != 0:
        upper = f"0.{release[1] + 1}"
    else:
        upper = f"0.0.{release[2] + 1}"
    return f">={version},<{upper}"


def poetry_constraint(value: object) -> tuple[str, bool]:
    optional = False
    if isinstance(value, dict):
        optional = bool(value.get("optional", False))
        value = value.get("version", "")
    constraint = str(value)
    if constraint.startswith("^"):
        constraint = compatible_release(constraint[1:])
    elif constraint == "*":
        constraint = ""
    return constraint, optional


def conda_requirement(raw: str, mapping: dict[str, str]) -> str | None:
    requirement = Requirement(raw)
    if requirement.marker and not requirement.marker.evaluate():
        return None
    name = mapping.get(
        normalized_name(requirement.name), normalized_name(requirement.name)
    )
    constraint = str(requirement.specifier)
    return f"{name} {constraint}" if constraint else name


def emit(path: str, value: object, *, append: bool = False) -> str:
    operation = ".append" if append else ""
    return f"{path}{operation} {json.dumps(value, separators=(',', ':'))}\n"


def read_metadata(manifest: dict) -> tuple[dict, list[str], list[str], list[str]]:
    """Return project metadata, build requirements, run requirements, entry points."""
    configured_mapping = (
        manifest.get("tool", {}).get("rattler-build", {}).get("pypi-to-conda", {})
    )
    mapping = DEFAULT_PYPI_TO_CONDA | {
        normalized_name(name): conda_name
        for name, conda_name in configured_mapping.items()
    }
    build_requires = [
        dependency
        for raw in manifest.get("build-system", {}).get("requires", [])
        if (dependency := conda_requirement(raw, mapping)) is not None
    ]

    if project := manifest.get("project"):
        run = []
        if requires_python := project.get("requires-python"):
            run.append(f"python {requires_python}")
        run.extend(
            dependency
            for raw in project.get("dependencies", [])
            if (dependency := conda_requirement(raw, mapping)) is not None
        )
        entry_points = [
            f"{name} = {target}" for name, target in project.get("scripts", {}).items()
        ]
        urls = project.get("urls", {})
        license_value = project.get("license")
        license_files = list(project.get("license-files", []))
        if isinstance(license_value, dict):
            if license_file := license_value.get("file"):
                license_files.append(license_file)
            license_value = license_value.get("text")
        metadata = {
            "name": project["name"],
            "version": project["version"],
            "summary": project.get("description"),
            "license": license_value,
            "homepage": urls.get("Homepage"),
            "repository": urls.get("Repository"),
            "documentation": urls.get("Documentation"),
            "license_files": license_files,
        }
        return metadata, build_requires, run, entry_points

    poetry = manifest["tool"]["poetry"]
    run = []
    for pypi_name, value in poetry.get("dependencies", {}).items():
        constraint, optional = poetry_constraint(value)
        if optional:
            continue
        if normalized_name(pypi_name) == "python":
            run.append(f"python {constraint}".rstrip())
            continue
        conda_name = mapping.get(normalized_name(pypi_name), normalized_name(pypi_name))
        run.append(f"{conda_name} {constraint}".rstrip())
    metadata = {
        "name": poetry["name"],
        "version": poetry["version"],
        "summary": poetry.get("description"),
        "license": poetry.get("license"),
        "homepage": poetry.get("homepage"),
        "repository": poetry.get("repository") or poetry.get("homepage"),
        "documentation": poetry.get("documentation"),
        "license_files": ["LICENSE"] if (SOURCE / "LICENSE").is_file() else [],
    }
    return metadata, build_requires, run, []


def main() -> None:
    manifest = tomllib.loads(PYPROJECT.read_text())
    metadata, build_requires, run, entry_points = read_metadata(manifest)
    if normalized_name(metadata["name"]) != normalized_name(os.environ["PKG_NAME"]):
        raise RuntimeError("recipe package.name does not match pyproject.toml")
    if str(metadata["version"]) != os.environ["PKG_VERSION"]:
        raise RuntimeError("recipe package.version does not match pyproject.toml")

    host = ["python", "pip", "python-build", *build_requires]
    with Path(os.environ["OUTPUT_FILE"]).open("w", encoding="utf-8") as stream:
        stream.write(emit("requirements.host", host, append=True))
        stream.write(emit("requirements.run", run, append=True))
        if entry_points:
            stream.write(emit("build.python.entry_points", entry_points, append=True))
        provider_version = os.environ["RATTLER_BUILD_PROVIDER_VERSION"]
        stream.write(
            emit(
                "build.steps",
                [
                    {
                        "name": "python-build",
                        "uses": f"python:build@=={provider_version}",
                    }
                ],
            )
        )
        for field in ["summary", "license", "homepage", "repository", "documentation"]:
            if value := metadata.get(field):
                stream.write(emit(f"about.{field}", value))
        for license_file in metadata["license_files"]:
            stream.write(emit("about.license_file.include", license_file, append=True))


if __name__ == "__main__":
    main()
