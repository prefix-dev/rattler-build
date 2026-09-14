# Pre-solve metadata step

This example reads conda-specific dependency lists from `pyproject.toml` in a
bootstrap Python environment. The metadata step emits final `build` and `host`
requirements and creates the complete `build.steps` plan before the package
environments are solved.

```console
rattler-build build --recipe examples/metadata-step --experimental
```

The backend generates a named `install` step. The recipe deliberately defines
its own `install` step, which replaces that generated default and writes
`share/metadata-step-example/generated.txt` to the package. Rendering also
executes the metadata phase because its output is required to expand variants
and produce the final rendered recipe:

```console
rattler-build build --recipe examples/metadata-step --experimental --render-only
```

The render output includes the generated YAML and an effective-step table. At
build time each named step is announced before execution.

Sources are fetched and prepared before metadata runs. `SRC_DIR` points to that
source tree, while `RECIPE_DIR` remains available for recipe-local helpers.
