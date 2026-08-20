# Experimental build steps

`build.steps` is an experimental alternative to `build.script`. Enable it with
`--experimental`. `script` and `steps` are mutually exclusive; even
`steps: []` explicitly selects steps mode and prevents default `build.sh` /
`build.bat` discovery.

Each step is a scoped section of the generated build wrapper, so step-local
`env` values and `cwd` changes do not leak into later steps. A step supports:

- **`name`** - Optional unique name. Named steps can be selected from the CLI.
  A `uses` reference becomes the default name when this is omitted.
- **`optional`** - Exclude the step from normal package builds (default: `false`).
- **`depends_on`** - Names of prerequisite steps, forming a DAG.
- **`requirements.build` / `requirements.host`** - Extra dependencies added
  to the selected step's build or host solve group.
- **`requirements.inherit`** - Whether the solve group includes the parent
  recipe environments. Use `false` to disable both, or a `{build, host}`
  mapping to control them separately.
- **`run` / `uses`** - Exactly one is required. `run` is an inline command,
  multiline string, or command list. `uses` references a reusable step.
- **`if`** - Optional Jinja selector expression, such as `unix` or
  `target_platform == "linux-64"`. Do not wrap expressions in `${{ }}`.
- **`interpreter`** - Optional interpreter override for this step.
- **`cwd`** - Optional working directory for this step. Relative paths are
  resolved against the host prefix (`$PREFIX` / `%PREFIX%`), and the wrapper
  changes to it only for that step.
- **`env`** - Optional environment variables scoped to this step.

```yaml title="recipe.yaml"
build:
  steps:
    - if: unix
      run: |
        mkdir -p "$PREFIX/bin"
        cp "$RECIPE_DIR/my_script_with_recipe.sh" "$PREFIX/bin/super-cool-script.sh"

    - if: win
      run: copy %RECIPE_DIR%\my_script_with_recipe.bat %LIBRARY_BIN%\super-cool-script.bat

    - name: build
      run: python -m pip install . --no-deps
      env:
        SETUPTOOLS_SCM_PRETEND_VERSION: ${{ version }}

    - name: test
      optional: true
      depends_on: [build]
      requirements:
        host: [pytest]
      run: pytest
```

Run a named step and its transitive prerequisites with:

```console
rattler-build run test --recipe . --source-dir . --experimental
```

`run` uses a deterministic build directory and updates its prefixes in place.
With `--source-dir .`, commands execute directly in the project checkout,
`SRC_DIR` points there, and tools such as CMake reuse the project's cache.
This intentionally bypasses recipe source fetching and patch application; the
checkout is treated as already prepared.
Set `requirements.inherit: false` to create a standalone tool environment,
such as for a Python `ruff` lint step, while retaining the step requirements.
Use `inherit: {build: false, host: true}` (or the expanded YAML mapping) to
control the parent environments independently. Isolated solves use their own
deterministic prefixes, preventing packages from an earlier parent-based run
from leaking into the tool environment. See
the [`examples/adjacent`](https://github.com/prefix-dev/rattler-build/tree/main/examples/adjacent)
recipe for an independent lint step and an optional C++ test step.

## Pre-solve metadata step

`build.metadata` is a single bootstrap step that runs after source fetching but
before normal reusable-step resolution, final build-step DAG selection, and the
final build/host dependency solve. It can inspect the prepared source tree and
emit dependencies or the executable build plan itself. If metadata itself uses
a provider, that one provider is resolved before source fetching.

!!! warning
Metadata runs arbitrary recipe code during both builds and render-only
operations. Do not render an untrusted recipe with experimental features
enabled.

The recipe is initially parsed and rendered to discover outputs and variants
before this phase. Consequently, a metadata step cannot change package identity,
sources, outputs, or variant selection. URL, Git, and path sources are fetched,
verified, extracted, and patched first, so metadata can inspect them through
`SRC_DIR`. `RECIPE_DIR` remains available for recipe-local support files.

The metadata step uses the normal step fields `run`, `uses`, `with`,
`interpreter`, `env`, `cwd`, and `requirements.build` / `requirements.host`.
Its `cwd` is relative to `SRC_DIR`. Its requirements are solved and installed
into a temporary bootstrap environment separate from the final package
environments. A metadata `uses` file must resolve to exactly one executable
step; it cannot be optional or depend on normal build steps.

The step receives `OUTPUT_FILE`, `RATTLER_BUILD_OUTPUT_FILE`, `RECIPE_DIR`,
`SRC_DIR`, `PKG_NAME`, `PKG_VERSION`, `BUILD_PLATFORM`, `HOST_PLATFORM`, and
`TARGET_PLATFORM`. A packaged metadata provider additionally receives
`RATTLER_BUILD_PROVIDER_PREFIX` and `RATTLER_BUILD_PROVIDER_VERSION` so code
and version-pinned normal build-step definitions can live in the same package. A successful metadata command must create
`OUTPUT_FILE` (it may be empty when no changes are needed); otherwise the phase
fails. The step writes the same line-oriented format as [post-build outputs](#post-build-metadata-outputs):

```text
requirements.build.append ["cmake", "ninja"]
requirements.host.append ["zlib"]
build.steps [{"name":"configure","run":"cmake -S . -B build"}]
about.repository https://github.com/example/project
```

Requirement fields are append-only. `build.steps` and `build.script` can be set
or extended, and `build.python.entry_points` can be appended for generated
Python console scripts. The normal post-build mutable fields can also be changed.
Arrays and objects use JSON syntax. Emitted dependency values must be concrete
match specs; selectors and variant expansion have already happened. If a
metadata-generated build/host dependency has a configured variant (for example
`python`), the initial recipe must reference that variant or pass
`${{ python }}` through the metadata provider's `with`; rattler-build rejects a
late dependency that would silently bypass variant expansion. Generated script
content still receives the normal late-bound build-script rendering. The output
content is included in the package variant hash. After a successful metadata
step, rattler-build prints the effective `build`, `requirements`, and `about`
metadata as YAML before resolving emitted step providers and dependencies, so
the dynamic result is visible without logging unrelated source or context data.

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

## Caching build steps

Each build step receives `RATTLER_BUILD_STEP_CACHE`, pointing to a persistent
file under the build directory. A successful step can write cache conditions to
this file. On later invocations, rattler-build skips the step when all matching
inputs and outputs are unchanged:

```yaml
- name: compile
  run: |
    cmake --build "$SRC_DIR/build"
    cat > "$RATTLER_BUILD_STEP_CACHE" <<'EOF'
    input-hash: CMakeLists.txt
    input-hash: src/**
    output-mtime: build/**
    EOF
```

On Windows, write the same lines to `%RATTLER_BUILD_STEP_CACHE%`. The format is
one `KEY: GLOB` declaration per line (blank lines and `#` comments are ignored):

- `input-hash` / `output-hash` compare matching paths and file contents.
- `input-mtime` / `output-mtime` compare matching paths, sizes, and modification
  times.

Globs use `/` separators, are relative to the step working directory, and may
not be absolute or contain `..`. Every condition must match at least one file;
a missing input or deleted output is a cache miss. Changes to the step's script,
interpreter, effective environment (including secret values), resolved dependency
set, or working directory also invalidate its cache.
Rattler-build stores the fingerprints next to the declaration file after the
step succeeds. It removes the old declaration before a cache-miss execution, so
a step must write the file again to remain cacheable. Failed steps never update
cache state. On a cache hit, the executor also retains that section's previous
`OUTPUT_FILE`, so generated post-build metadata is replayed consistently.

The declaration file is intentionally simple to generate from shell scripts;
the adjacent executor-owned `.state.json` file is an implementation detail and
should not be edited by the step.

See [`examples/step-cache`](https://github.com/prefix-dev/rattler-build/tree/main/examples/step-cache)
for a small cross-platform, two-step example using both hash and mtime checks.
The [`examples/adjacent`](https://github.com/prefix-dev/rattler-build/tree/main/examples/adjacent)
recipe shows the same feature around a real CMake configure/build pipeline.

## Reusable steps

A step can load its executable fields from a small YAML file:

```yaml title="recipe.yaml"
build:
  steps:
    - name: lint
      uses: ./steps/lint.yaml
      requirements:
        inherit: false
        build: [ruff]
```

```yaml title="steps/lint.yaml"
steps:
  - name: check
    run: ruff check .
    env:
      RUFF_NO_CACHE: "1"
  - name: format
    depends_on: [check]
    run: ruff format --check .
```

Local paths are relative to the recipe directory. A reusable file may contain
either one step or a complete `steps:` pipeline. Pipeline DAG ordering and
optional steps are supported. The referencing step may override the interpreter
and working directory and extend/override the environment for every nested
step. Reusable step requirements are preprocessed and included in the recipe's
build or host solve.

Package references use `provider:step` syntax and may include a conda version
constraint after `@`:

```yaml
- uses: cargo:build@>=0.3,<0.4
```

With no explicit `name`, this step is named `cargo:build`, so it can be run as
`rattler-build run cargo:build`. Before solving the recipe environments,
rattler-build resolves `cargo-rattler-build-steps` for the build platform and
installs it into a content-addressed provider prefix under the global cache.
The cache identity includes the platform and complete solved records, channels,
and artifact hashes. Provider packages never enter the recipe build or host
prefix.

Rattler-build loads `etc/rattler-build/steps/cargo/build.yaml` from that
standalone prefix and stores the rendered steps, portable reference, content
SHA-256, and exact provider package version, build, subdir, channel, and SHA-256
in the rendered recipe. Provider installation is data-only: package link scripts
are not executed during preprocessing. Requirements declared by those steps are
added to the recipe solve. An extensionless `build` file is accepted as a
fallback. Provider packages should therefore contain step definitions only;
tools such as `cargo` belong in the reusable step's `requirements.build`.
Complete CMake, Meson, Rust, and Go recipes are available in
[`examples/step-providers`](https://github.com/prefix-dev/rattler-build/tree/main/examples/step-providers).

Reusable pipelines can declare typed inputs and use them in Jinja templates:

```yaml title="provider build.yaml"
inputs:
  extra_args:
    type: list
    default: []
  install:
    type: boolean
    default: true
steps:
  - run: cmake -S "$SRC_DIR" -B "$BUILD_DIR/cmake" ${{ inputs.extra_args | join(' ') }}
  - if: inputs.install
    then:
      - run: cmake --install "$BUILD_DIR/cmake"
```

```yaml title="recipe.yaml"
build:
  steps:
    - uses: cmake:build
      with:
        extra_args: [-DBUILD_TESTING=ON]
        install: false
```

Unknown inputs, missing required inputs, and values of the wrong declared type
are rejected during preprocessing. Inputs may use recipe templates and therefore
participate in normal used-variable tracking. Reusable files use the same valid-YAML
`if` / `then` / `else` preprocessing selectors as recipes; `{% if %}` template
blocks are not supported.

## Post-build metadata outputs

Every build-step section receives a unique `OUTPUT_FILE` environment variable.
A step can write line-oriented metadata to this file after generating files or
inspecting build-system output. Each line contains a dotted field, an optional
`.append` operation, whitespace, and a value. Plain values are strings; lists
and objects use JSON syntax so they remain unambiguous and easy to generate with
`cat`:

```yaml
requirements:
  build: [go, go-licenses]
run: |
  go-licenses save ./... --save_path "$BUILD_DIR/go-dependencies"
  dollar='$'
  cat > "$OUTPUT_FILE" <<EOF
  about.repository https://github.com/example/project
  about.license_file.include.append ["$dollar{{ BUILD_DIR }}/go-dependencies/**"]
  requirements.run.append ["libgcc >=14", "zlib"]
  requirements.run_exports.strong.append ["project-abi >=1,<2"]
  EOF
```

Outputs are applied in step execution order after all build steps finish and
before packaging. Supported requirement collections are `requirements.run`,
`requirements.run_constraints`, and the `noarch`, `strong`, `weak`,
`strong_constraints`, and `weak_constraints` collections below
`requirements.run_exports`. These update `index.json` and `run_exports.json` in
the resulting package. Requirement fields are append-only because replacing an
already finalized dependency set would be ambiguous.

`requirements.build` and `requirements.host` cannot be emitted by a normal
build step: its environments have already been solved and installed. Emit them
from [`build.metadata`](#pre-solve-metadata-step), or declare them statically on
a reusable step so rattler-build can collect them before the final solve.

Post-build output may also update `about.*` and packaging-time fields under
`build.dynamic_linking`, `build.prefix_detection`, `build.files`,
`build.always_copy_files`, `build.always_include_files`, and
`build.post_process`. Append targets are materialized even when omitted from the
recipe. This line format replaces the prototype's RFC 6902 JSON Patch format so
step output remains straightforward to inspect and generate.

!!! warning "Windows multiline steps"
On Windows, a multiline `run: |` block is emitted as one command-list item.
Rattler-Build inserts fail-fast guards between list items, not between the
physical lines inside one multiline scalar, so check `%errorlevel%` yourself
when a multiline `cmd.exe` block needs per-line failure handling.
