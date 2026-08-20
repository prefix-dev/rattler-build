# Tiny Python metadata backend

This is a complete, deliberately small analogue of `pixi-build-python`. A
custom Python program reads standard PEP 621 metadata from `pyproject.toml`,
emits conda requirements and package metadata during `build.metadata`, and
creates two normal rattler-build steps that build and install a wheel.

The Python project depends on `rich`, so the resulting noarch conda package can
run this generated console entry point:

```console
$ tingy-rich-demo
tingy metadata backend works!
```

## Build it

From the repository root:

```console
rattler-build build \
  --recipe examples/python-metadata-backend \
  --experimental
```

The package tests import `tingy_rich_demo` and execute `tingy-rich-demo`.

## Minimal recipe

All Python-specific host/run requirements, `about` metadata, and executable
build steps come from `pyproject.toml`. The recipe only supplies the fields that
must exist during initial output discovery, the local source, the noarch mode,
the metadata backend bootstrap, and tests:

```yaml
schema_version: 1

context:
  project: ${{ load_from_file("project/pyproject.toml").project }}

package:
  name: ${{ project.name }}
  version: ${{ project.version }}

source:
  path: project

build:
  noarch: python
  metadata:
    requirements:
      build: [python >=3.11, packaging]
    interpreter: python
    run: |
      import os
      from pathlib import Path

      backend = Path(os.environ["RECIPE_DIR"]) / "python_metadata_backend.py"
      exec(compile(backend.read_text(), backend, "exec"), {"__name__": "__main__"})

tests:
  - python:
      imports: [tingy_rich_demo]
  - script:
      - tingy-rich-demo
```

`python_metadata_backend.py` performs three jobs:

1. Parse `[project]` and `[build-system]` with `tomllib` and PEP 508 strings
   with `packaging.Requirement`.
2. Write host/run requirements and `about.*` fields to `OUTPUT_FILE`.
3. Set `build.steps` to a `python-build` wheel step followed by a `pip install
   --no-deps --no-build-isolation` step in the activated host prefix.

Its relevant output is equivalent to:

```text
requirements.host.append ["python","pip","python-build","hatchling >=1.26"]
requirements.run.append ["python >=3.10","rich <15,>=13.9"]
build.python.entry_points.append ["tingy-rich-demo = tingy_rich_demo:main"]
about.summary "A tiny Rich application built by a custom rattler-build metadata backend"
about.license "MIT"
about.license_file.include.append "LICENSE"
build.steps [{"name":"build-wheel",...},{"name":"install-wheel",...}]
```

## PyPI-to-conda mapping

A production backend cannot assume PyPI and conda package names are identical.
The example combines a tiny built-in map with project overrides:

```python
DEFAULT_PYPI_TO_CONDA = {"hatchling": "hatchling"}
```

```toml
[tool.rattler-build.pypi-to-conda]
rich = "rich"
```

That is sufficient here because both names happen to match. A real backend
should query the prefix.dev PyPI-to-conda mapping service, cache its answers,
and support user overrides for missing or ambiguous mappings. It must also
translate version syntax more carefully than this example's intentionally
small `>=`/`<` overlap.

## Current holes and deliberate limitations

- **Package identity is too early:** `package.name` and `package.version` cannot
  be emitted by metadata because output discovery needs them first. This local
  example bridges that gap with experimental `load_from_file`; a remote source
  still cannot provide identity this way.
- **Noarch is too early:** `build.noarch: python` also has to remain in the
  recipe because platform/output setup has already started.
- **Remote source is too late:** metadata runs before normal source fetching.
  This example therefore uses a local `project/` directory. A recipe for
  an upstream sdist cannot inspect that sdist's `pyproject.toml` in this phase
  without downloading it independently, which would duplicate source logic.
- **Dependency conversion is incomplete:** arbitrary PEP 440 operators, extras,
  direct URLs, environment markers for a cross-compilation target, optional
  dependencies, and build-backend-specific dynamic metadata need a real
  converter. This backend supports the straightforward dependencies used here.
- **Markers use the bootstrap machine:** `packaging` evaluates markers in the
  metadata environment, not against rattler-build's target platform.
- **Dynamic PEP 621 fields are unsupported:** this reads static TOML; it does
  not call PEP 517 `prepare_metadata_for_build_wheel`.
- **Tests cannot be generated:** the current pre-solve output allowlist covers
  requirements, `about`, and `build.script`/`build.steps`, but not recipe tests.
- **The backend is loaded by a tiny inline shim:** metadata steps do not yet
  support a local or packaged `uses` provider directly.

These constraints are why this is an executable protocol example rather than a
replacement for `pixi-build-python`.
