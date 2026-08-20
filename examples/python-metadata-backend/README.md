# Pack external Rich with a Python metadata provider

This example literally downloads the Rich 14.2.0 sdist from PyPI, reads its
`pyproject.toml`, derives conda metadata, builds a wheel, installs it into a
noarch package, and runs package tests. The consumer recipe has no Python host
or run requirements and no build script of its own.

The same `python-rattler-build-steps` conda package supplies both halves:

- `python:metadata` reads PEP 621 or Poetry `pyproject.toml` metadata and emits
  host/run requirements, `about` fields, license files, entry points, and a
  `python:build` step reference.
- `python:build` builds a wheel with `python-build`, then installs it with pip.

## Minimal Rich recipe

```yaml
schema_version: 1

context:
  version: "14.2.0"

package:
  name: rich
  version: ${{ version }}

source:
  url: https://pypi.io/packages/source/r/rich/rich-${{ version }}.tar.gz
  sha256: 73ff50c7c0c1c77c8243079283f4edb376f0f6442433aecb8ce7e6d0b92d1fe4

build:
  noarch: python
  metadata:
    uses: python:metadata@0.1.*

tests:
  - python:
      imports: [rich]
```

`package`, `source`, and `noarch` are required before metadata executes. All of
these fields are generated from the fetched `pyproject.toml`:

```text
requirements.host.append ["python","pip","python-build","poetry-core >=1.0.0"]
requirements.run.append ["python >=3.8.0","pygments >=2.13.0,<3","markdown-it-py >=2.2.0"]
about.summary "Render rich text, tables, progress bars, syntax highlighting, markdown and more to the terminal"
about.license "MIT"
about.license_file.include.append "LICENSE"
build.steps [{"uses":"python:build@==0.1.0"}]
```

## Run the complete example locally

First build the package containing both providers:

```console
rattler-build build \
  --recipe examples/python-metadata-backend/provider \
  --output-dir output/provider
```

Publish that artifact to a temporary local channel:

```console
rattler-build publish \
  output/provider/noarch/python-rattler-build-steps-0.1.0-*.conda \
  --to output/provider-channel
```

If your package format is `tar.bz2`, use that filename instead. Then build Rich:

```console
rattler-build build \
  --recipe examples/python-metadata-backend \
  --output-dir output/rich \
  --channel "file://$PWD/output/provider-channel" \
  --channel conda-forge \
  --experimental
```

The build downloads and verifies the upstream sdist, resolves the provider,
fetches the source before metadata execution, creates a bootstrap Python
environment for the extractor, solves the emitted final requirements, executes
the provider's wheel steps, and tests both `import rich` and Rich rendering.

The example provider is also published on the beta prefix.dev channel. After
building this feature branch's `rattler-build`, try Rich directly with:

```console
rattler-build build \
  --recipe examples/python-metadata-backend \
  --channel https://beta.prefix.dev/wolfv/rattler-build-steps \
  --channel conda-forge \
  --experimental
```

To upload a rebuilt provider with rattler-build 0.74.0 or newer:

```console
rattler-build publish \
  output/provider/noarch/python-rattler-build-steps-0.1.0-*.conda \
  --to prefix://beta.prefix.dev/wolfv/rattler-build-steps
```

Publishing requires an authenticated prefix.dev account with upload permission.

## Provider layout

The package recipe installs these files together:

```text
etc/rattler-build/steps/python/
├── metadata.yaml
├── build.yaml
└── python_metadata_backend.py
```

The metadata wrapper receives `SRC_DIR`, `RECIPE_DIR`, `PKG_NAME`,
`PKG_VERSION`, `RATTLER_BUILD_PROVIDER_PREFIX`, and
`RATTLER_BUILD_PROVIDER_VERSION`. The Python extractor reads
`$SRC_DIR/pyproject.toml`, verifies that its name/version agree with the recipe,
and writes the line-oriented protocol to `OUTPUT_FILE`. Its emitted
`python:build` reference resolves from the same already-cached provider package.

## PyPI-to-conda mapping

PyPI and conda names cannot generally be assumed to match. The example has a
small deterministic map for Rich and its Poetry build backend:

```python
DEFAULT_PYPI_TO_CONDA = {
    "markdown-it-py": "markdown-it-py",
    "poetry-core": "poetry-core",
    "pygments": "pygments",
}
```

A production backend should query and cache the prefix.dev PyPI-to-conda
mapping service and support user overrides for missing or ambiguous names.
Keeping the map local makes this example reproducible and avoids hiding that
important conversion step.

## Current holes and deliberate limitations

- **Identity and noarch are still static:** output discovery needs
  `package.name`, `package.version`, and `build.noarch` before metadata runs.
- **Sources are prepared twice:** source archives are downloaded once into the
  source cache, but currently extracted/copied for metadata and restored again
  for the normal build. The second pass uses the cache rather than downloading
  the archive again.
- **Dependency conversion is partial:** the extractor supports the static PEP
  621 subset and the Poetry constraints used by Rich. Arbitrary PEP 440/Poetry
  operators, direct URLs, extras, dependency groups, and dynamic metadata need
  a production-grade converter.
- **Marker evaluation is incomplete:** PEP 508 markers are evaluated in the
  bootstrap environment rather than a complete target-platform marker context.
- **Dynamic PEP 517 metadata is not queried:** the extractor does not call
  `prepare_metadata_for_build_wheel`; it only reads static TOML.
- **Generated tests are unsupported:** metadata may generate requirements,
  `about`, Python entry points, and build steps/scripts, but not recipe tests.
- **Provider prefixes are process-local:** the metadata wrapper receives the
  resolved provider prefix as an environment variable. Rendered provider
  provenance remains stable, but that absolute execution path is not portable
  to another machine outside a fresh render/build.

This is an executable protocol demonstration, not yet a replacement for
`pixi-build-python`.
