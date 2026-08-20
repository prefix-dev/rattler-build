"""Tiny pyproject.toml -> rattler-build metadata backend used by this example.

This intentionally implements only the small, documented PEP 621 subset needed
by the demo. It is an example of the protocol, not a general PyPI-to-conda
converter.
"""

from __future__ import annotations

import json
import os
from pathlib import Path
import re
import tomllib

from packaging.requirements import Requirement


RECIPE_ROOT = Path(os.environ["RECIPE_DIR"])
PROJECT_ROOT = RECIPE_ROOT / "project"
PYPROJECT = PROJECT_ROOT / "pyproject.toml"

# PyPI and conda names frequently differ. A production backend needs the
# prefix.dev mapping service (plus explicit user overrides). These are enough
# for this self-contained example.
DEFAULT_PYPI_TO_CONDA = {
    "hatchling": "hatchling",
}


def conda_requirement(raw: str, mapping: dict[str, str]) -> str | None:
    """Convert a small PEP 508 subset to a conda MatchSpec string."""
    requirement = Requirement(raw)
    if requirement.marker and not requirement.marker.evaluate():
        return None
    normalized = re.sub(r"[-_.]+", "-", requirement.name).lower()
    name = mapping.get(normalized, normalized)
    # PEP 440 and conda constraints overlap for this example's >=,< bounds.
    constraint = str(requirement.specifier)
    return f"{name} {constraint}" if constraint else name


def emit(path: str, value: object, *, append: bool = False) -> str:
    operation = ".append" if append else ""
    return f"{path}{operation} {json.dumps(value, separators=(',', ':'))}\n"


def main() -> None:
    manifest = tomllib.loads(PYPROJECT.read_text())
    project = manifest["project"]
    build_system = manifest["build-system"]
    mapping = DEFAULT_PYPI_TO_CONDA | manifest.get("tool", {}).get(
        "rattler-build", {}
    ).get("pypi-to-conda", {})

    host = ["python", "pip", "python-build"]
    host.extend(
        dependency
        for raw in build_system.get("requires", [])
        if (dependency := conda_requirement(raw, mapping)) is not None
    )

    run = []
    if requires_python := project.get("requires-python"):
        run.append(f"python {requires_python}")
    run.extend(
        dependency
        for raw in project.get("dependencies", [])
        if (dependency := conda_requirement(raw, mapping)) is not None
    )

    wheel_step = {
        "name": "build-wheel",
        "interpreter": "python",
        "run": """from build import ProjectBuilder\nfrom pathlib import Path\nimport os\nout = Path(os.environ['BUILD_DIR']) / 'python-wheels'\nout.mkdir(parents=True, exist_ok=True)\nProjectBuilder(os.environ['SRC_DIR']).build('wheel', str(out))""",
    }
    install_step = {
        "name": "install-wheel",
        "depends_on": ["build-wheel"],
        "interpreter": "python",
        "run": """from pathlib import Path\nimport os, subprocess, sys\nwheels = list((Path(os.environ['BUILD_DIR']) / 'python-wheels').glob('*.whl'))\nif len(wheels) != 1:\n    raise RuntimeError(f'expected one wheel, found {wheels}')\nsubprocess.check_call([sys.executable, '-m', 'pip', 'install', str(wheels[0]), '--no-deps', '--no-build-isolation'])""",
    }

    urls = project.get("urls", {})
    output = Path(os.environ["OUTPUT_FILE"])
    with output.open("w", encoding="utf-8") as stream:
        stream.write(emit("requirements.host", host, append=True))
        stream.write(emit("requirements.run", run, append=True))
        entry_points = [
            f"{name} = {target}" for name, target in project.get("scripts", {}).items()
        ]
        if entry_points:
            stream.write(emit("build.python.entry_points", entry_points, append=True))
        stream.write(emit("build.steps", [wheel_step, install_step]))
        if summary := project.get("description"):
            stream.write(emit("about.summary", summary))
        if license_expression := project.get("license"):
            stream.write(emit("about.license", license_expression))
        for license_file in project.get("license-files", []):
            stream.write(emit("about.license_file.include", license_file, append=True))
        if homepage := urls.get("Homepage"):
            stream.write(emit("about.homepage", homepage))
        if repository := urls.get("Repository"):
            stream.write(emit("about.repository", repository))


if __name__ == "__main__":
    main()
