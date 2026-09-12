# Pre-solve recipe generation

`build.metadata` is a bootstrap Run or Uses invocation that executes after source
fetching but before normal action compilation, named-step selection, and the
final build/host dependency solve. It can inspect the prepared source tree and
emit dependencies or the executable build plan itself. Packaged actions used by
metadata are loaded during bootstrap compilation, before source fetching.

!!! warning
    Metadata runs arbitrary recipe code during both builds and render-only
    operations. Do not render an untrusted recipe with experimental features
    enabled.

The recipe receives a bootstrap render to discover its outputs before this
phase. URL, Git, and path sources are then fetched, verified, extracted, and
patched, so metadata can inspect them through `SRC_DIR`. `RECIPE_DIR` remains
available for recipe-local support files. After metadata has generated its
requirements, rattler-build resolves generated reusable-step providers and then
performs the final variant expansion. A free dependency from metadata or a
resolved provider, such as `python` or `zlib`, therefore expands over all values
configured for that key (including `zip_keys` behavior) before any final
dependency solve. Metadata still cannot change package identity, sources, or the
output list. `build.metadata` is not yet supported in multi-output recipes:
even an `about` change alters the upstream build hash, requiring the output
graph's exact subpackage pins and dependent hashes to be recomputed.

Metadata uses the same strict Run and Uses schemas as normal steps. A Uses
invocation accepts `with`, but cannot override execution fields such as `env` or
`requirements`. Its action may recursively expand into multiple executable steps.
The invocation cannot be optional or depend on normal build steps.

Bootstrap requirements are solved and installed into temporary environments,
separate from the final package environments. Each executable step runs with
strict environment isolation and the configured sandbox policy. Its `cwd` is
relative to `SRC_DIR` and must stay within that directory.

The step receives `OUTPUT_FILE`, `RATTLER_BUILD_OUTPUT_FILE`, `RECIPE_DIR`,
`SRC_DIR`, `PKG_NAME`, `PKG_VERSION`, `BUILD_PLATFORM`, `HOST_PLATFORM`, and
`TARGET_PLATFORM`. Runtime tools and support code must be declared as action
requirements; the transport cache is not a runtime environment. All steps in
the metadata invocation share one `OUTPUT_FILE`. The completed plan must create
that file, which may be empty when no changes are needed. It uses the same
line-oriented format as [post-build outputs](outputs.md):

```text
requirements.build.append ["cmake", "ninja"]
requirements.host.append ["zlib"]
build.steps [{"name":"configure","run":"cmake -S . -B build"}]
about.repository https://github.com/example/project
```

Requirement fields are append-only. `build.steps` and `build.script` can be set
or extended, and `build.python.entry_points` can be appended for generated
Python console scripts. Backends that introduce variants not discoverable from
a free dependency name (for example compiler variants) can append explicit keys
to `build.variant.use_keys`. Other mutable fields use their authored recipe
shape; for example, `about.license_file.append "LICENSE"` extends the source
license list. Valid JSON values retain their native types; other values are strings.
The protocol patches authored recipe source, removes `build.metadata`, and
renders it through the same action compiler, provider registry, and variant
expansion as an ordinary recipe. Generated selectors and inputs therefore use
the normal source schema. Free dependency names introduce configured variants,
and script content retains normal late-bound rendering. The metadata content
fingerprint and final variant values participate in the ordinary package hash.

Every metadata-generated build step must have a unique, literal `name`,
including Uses invocations. Recipe-authored
`build.steps` with the same name replace the generated default; additional
recipe-authored steps are appended (unnamed authored steps are allowed but
cannot override by name). `build.steps.append` preserves authored order and adds
non-overridden generated steps after it. This lets a backend provide a useful
pipeline while a consumer replaces only the part it understands better:

```yaml
build:
  metadata:
    uses: cmake:metadata
    with:
      cmake_args: [-DBUILD_SHARED_LIBS=ON]
  steps:
    - name: configure # replaces the generated `configure` step
      run: cmake -S . -B build -DMY_PROJECT_OPTION=ON
```

Metadata providers declare and validate `inputs` exactly like normal reusable
steps, and consumers pass typed values through `build.metadata.with`. Unknown,
missing required, and incorrectly typed inputs fail during preprocessing.

After a successful metadata step, rattler-build prints the emitted patch
directives and a table of the final executable steps for every expanded variant.
Dependency edges have already been compiled into execution order at this point. During execution, each section is announced as
`Running build step: NAME`. To inspect all of this without solving or building
the final package, use:

```console
rattler-build build --recipe . --render-only --experimental
```

Metadata itself still executes during render-only operations because its output
is required to determine the final variants and recipe.

For example, a project can keep conda-specific dependency declarations in
`pyproject.toml` and generate its build pipeline:

```toml title="pyproject.toml"
[tool.rattler-build]
build = ["cmake", "ninja"]
host = ["zlib"]
```

```yaml title="recipe.yaml"
build:
  metadata:
    requirements:
      build: [python]
    interpreter: python
    run: |
      import json, os, pathlib, tomllib

      project = tomllib.loads(pathlib.Path("pyproject.toml").read_text())
      dynamic = project["tool"]["rattler-build"]
      steps = [
          {"name": "configure", "run": "cmake -S . -B build -G Ninja"},
          {"name": "build", "depends_on": ["configure"], "run": "cmake --build build"},
      ]
      with open(os.environ["OUTPUT_FILE"], "w") as output:
          output.write(f"requirements.build.append {json.dumps(dynamic['build'])}\n")
          output.write(f"requirements.host.append {json.dumps(dynamic['host'])}\n")
          output.write(f"build.steps {json.dumps(steps)}\n")
```

This deliberately uses a bootstrap `python` only to read TOML. Translating
arbitrary PyPI, CMake, Cargo, or other ecosystem dependencies into conda package
names remains the responsibility of the metadata script or a future dedicated
provider. A complete runnable version is available in
[`examples/metadata-step`](https://github.com/prefix-dev/rattler-build/tree/main/examples/metadata-step).
For a fuller backend-style example that reads PEP 621 metadata, maps PyPI
requirements to conda requirements, generates wheel build/install steps, and
builds a tested noarch package, see
[`examples/python-metadata-backend`](https://github.com/prefix-dev/rattler-build/tree/main/examples/python-metadata-backend).
