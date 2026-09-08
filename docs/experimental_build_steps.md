# Experimental build steps

!!! warning "Experimental"
    Named and reusable build steps may change or be removed. They require
    `--experimental`.

`build.steps` is an experimental alternative to `build.script`. `script` and
`steps` are mutually exclusive; even
`steps: []` explicitly selects steps mode and prevents default `build.sh` /
`build.bat` discovery.

Each step is a scoped section of the generated build wrapper, so step-local
`env` values and `cwd` changes do not leak into later steps. A step supports:

- **`name`** - Optional unique name. Named steps can be selected from the CLI.
- **`optional`** - Exclude the step from normal package builds (default: `false`).
- **`depends_on`** - Names of prerequisite steps, forming a DAG.
- **`requirements.build` / `requirements.host`** - Extra dependencies added
  to the selected step's build or host solve group.
- **`requirements.inherit`** - Whether the solve group includes the parent
  recipe environments. Use `false` to disable both, or a `{build, host}`
  mapping to control them separately.
- **`run`** - Required inline command, multiline string, or command list.
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
Without `--source-dir`, repeated runs reuse prepared sources only while their
recipe path and source definitions match. If they change, execution stops rather
than discarding local edits or using stale sources. Preserve your edits and move
the reported work directory aside to fetch fresh sources on the next run.
Set `requirements.inherit: false` to create a standalone tool environment,
such as for a Python `ruff` lint step, while retaining the step requirements.
Use `inherit: {build: false, host: true}` (or the expanded YAML mapping) to
control the parent environments independently. Isolated solves use their own
deterministic prefixes, preventing packages from an earlier parent-based run
from leaking into the tool environment. See
the [`examples/adjacent`](https://github.com/prefix-dev/rattler-build/tree/main/examples/adjacent)
recipe for an independent lint step and an optional C++ test step.

Without `build.metadata`, `run --render-only` renders recipes without executing
steps, fetching sources, or creating build environments. Dependency solving is disabled unless
`--with-solve` is explicitly supplied, as with `build --render-only`.

## Caching build steps

Each build step receives `RATTLER_BUILD_STEP_CACHE`, pointing to a persistent
declaration file under the build directory. A successful step can write cache
conditions to this file:

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

On Windows, write the same lines to `%RATTLER_BUILD_STEP_CACHE%`. Each line is
`KEY: GLOB`; blank lines and `#` comments are ignored. `input-hash` and
`output-hash` compare matching paths and contents. `input-mtime` and
`output-mtime` compare paths, sizes, and modification times.
Hash conditions include symlink identity and follow directory links using paths
relative to the step working directory. Modification-time conditions inspect
links themselves rather than their targets.

Globs use `/`, are relative to the step working directory, and cannot be
absolute or contain `..`. Every condition must match an entry. Missing inputs,
deleted outputs, or changes to the script, interpreter, effective environment,
working directory, or compiled plan invalidate the cache.

After success, rattler-build stores fingerprints and a checksum-verified copy
of `OUTPUT_FILE` outside disposable `work/`. A hit restores this metadata,
including intentional absence. Missing or altered replay data causes a miss.
Before executing a miss, the previous success record and declaration are
removed: a failed rerun cannot revive stale success. A successful step must
write its declaration again to remain cacheable. The adjacent `.state.json`
and `.output` files belong to the executor and should not be edited.

See [`examples/step-cache`](https://github.com/prefix-dev/rattler-build/tree/main/examples/step-cache)
for a cross-platform example, and
[`examples/adjacent`](https://github.com/prefix-dev/rattler-build/tree/main/examples/adjacent)
for a CMake pipeline.

## Reusable steps

An action is a strict YAML document compiled during recipe rendering:

```yaml title="recipe.yaml"
build:
  steps:
    - name: lint
      uses: ./steps/lint.yaml
      with:
        paths: [src, tests]
```

```yaml title="steps/lint.yaml"
schema_version: 1
action:
  name: Python lint checks
inputs:
  paths:
    type: list
    items: string
    default: ["."]
requirements:
  build: [ruff]
steps:
  - name: check
    run: ruff check ${{ inputs.paths | join(" ") }}
    env:
      RUFF_NO_CACHE: "1"
  - name: format
    depends_on: [check]
    run: ruff format --check ${{ inputs.paths | join(" ") }}
```

Local references must start with `./` or `../` and end in `.yaml` or `.yml`.
Top-level references are relative to the recipe; nested references are relative
to their containing action document. Actions may call other actions. Cycles are
errors, nesting is limited to 64 documents, and repeated invocations are legal.
`steps: []` is valid and still contributes the action's build and host requirements.

Inputs require an explicit `string`, `boolean`, `integer` (signed 64-bit), or
`list` type. Lists declare a scalar `items` type. A static default makes an input
optional; otherwise it is required unless `required: false` is set. Optional
inputs without defaults receive null. Explicit null overrides a default but is
invalid for required inputs. Values are never coerced: quoted strings remain
strings, standalone Jinja expressions preserve native types, and list elements
can contain templates. Unknown fields and undeclared inputs are errors.

An invocation accepts only `uses`, `with`, `name`, `optional`, `depends_on`, and
`if`. Its condition is evaluated before loading the document or validating its
inputs. Run steps own `env`, `cwd`, `interpreter`, and inline requirements.
Action requirements merge into the effective recipe before variant expansion
and solving. Configured variants are available implicitly inside actions, while
recipe-private context is not: pass private values explicitly through `with`.
`python` and `inputs.python` are separate names.

Name invocations explicitly to select their whole group with `rattler-build run`.
Selection and dependencies are resolved before flattening, including empty
groups. Rendered recipes contain only executable run steps and native execution
bindings; rebuilding them does not load action source documents.

## Packaged actions

Package references use `provider:step` syntax and may include a conda version
constraint after `@`:

```yaml
- name: cargo-build
  uses: cargo:build@>=0.3,<0.4
```

The invocation name remains a CLI target: `rattler-build run cargo-build`.
During recipe compilation, an included invocation resolves
`cargo-rattler-build-steps` for the build platform and installs it into a
content-addressed prefix under the global cache. The cache identity includes the
platform and complete solved package records, channels, and artifact checksums,
using MD5 when SHA-256 is unavailable. Provider packages and their dependencies
do not enter the recipe build or host environment.

The compiler loads `etc/rattler-build/steps/cargo/build.yaml` (or `build.yml`)
from that prefix using the same action schema as local documents.
Nested `./helper.yaml` and `../shared/helper.yml` references resolve relative to
their containing document; nested packaged references use the same transport.
An invocation excluded by `if` does not resolve or install its provider.

The rendered recipe contains the flat executable plan, native execution bindings,
and portable provider provenance: reference, document SHA-256, package version,
build, subdir, channel, and available artifact checksums. Cached absolute source
paths are not serialized. Rebuild executes this embedded plan without reading
action sources or resolving providers again.

Provider installation does not execute package link scripts.
Only action-owned `requirements.build` and `requirements.host` participate in
the recipe's normal variant expansion and environment solve. Provider package
dependencies are transport dependencies, not action requirements; tools such as
`cargo` belong in the action document's `requirements.build`.
Complete CMake, Meson, Rust, and Go recipes are available in
[`examples/step-providers`](https://github.com/prefix-dev/rattler-build/tree/main/examples/step-providers).


## Staging steps

Staging actions use the same step execution and cache transactions as package
actions. `RATTLER_BUILD_STEP_CACHE` is available, and successful declarations
are recorded; a whole-stage cache hit still bypasses execution of the stage.

Staging outputs do not have a package identity. Writing package metadata to
`OUTPUT_FILE` from a staging step is rejected with an error before a success
record is committed. Put metadata-producing actions, such as dependency-license
collection, on the inheriting package output instead.

## Post-build outputs

Each build-step section receives a unique `OUTPUT_FILE` (also exposed as
`RATTLER_BUILD_OUTPUT_FILE`). Write one dotted field, an optional `.append`
operation, whitespace, and a value per line. Valid JSON preserves native
booleans, numbers, null, lists, and objects; other values are plain strings.
Quote a JSON-looking value such as `"true"` when a string is intended.

For example, an action can collect dependency licenses after running its tools:

```yaml
schema_version: 1
requirements:
  build: [go, go-licenses]
steps:
  - run: |
      go-licenses save ./... --save_path "$BUILD_DIR/go-dependencies"
      dollar='$'
      cat > "$OUTPUT_FILE" <<EOF
      about.repository https://github.com/example/project
      about.license_file.include.append ["$dollar{{ BUILD_DIR }}/go-dependencies/**"]
      requirements.run.append ["libgcc >=14", "zlib"]
      requirements.run_exports.strong.append ["project-abi >=1,<2"]
      EOF
```

Outputs are applied in execution order after all build steps finish, before
packaging. Supported requirement collections are `requirements.run`,
`requirements.run_constraints`, and the `noarch`, `strong`, `weak`,
`strong_constraints`, and `weak_constraints` collections under
`requirements.run_exports`. They update package `index.json` and
`run_exports.json`. Requirements are append-only: replacing finalized
dependencies would be ambiguous.
Embedded rebuild recipes retain the pre-output state, so rebuilding applies
each emitted directive once rather than accumulating changes.

Runtime output cannot change `requirements.build` or `requirements.host`.
Declare those on the action document so the compiler includes them before
solving and installing the environments.

Post-build output can also update `about.*` and packaging fields under
`build.dynamic_linking`, `build.prefix_detection`, `build.files`,
`build.always_copy_files`, `build.always_include_files`, and
`build.post_process`. Append targets are materialized when omitted from the
recipe.

!!! warning "Windows multiline steps"
    On Windows, a multiline `run: |` block is emitted as one command-list item.
    Rattler-Build inserts fail-fast guards between list items, not between the
    physical lines inside one multiline scalar, so check `%errorlevel%` yourself
    when a multiline `cmd.exe` block needs per-line failure handling.

## Pre-solve metadata step

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
line-oriented format as [post-build outputs](#post-build-outputs):

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

After a successful metadata step, rattler-build prints the effective `build`,
`requirements`, and `about` metadata as YAML. It then prints a compact table of
the final named steps and dependencies for every expanded variant. During execution, each section is announced as
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
