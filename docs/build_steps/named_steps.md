# Named steps and local execution

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

