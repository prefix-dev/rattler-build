# Pre-solve metadata step

This example reads conda-specific dependency lists from `pyproject.toml` in a
bootstrap Python environment. The metadata step emits final `build` and `host`
requirements and creates the complete `build.steps` plan before the package
environments are solved.

```console
rattler-build build --recipe examples/metadata-step --experimental
```

The generated build step writes `share/metadata-step-example/generated.txt` to
the package. Rendering also executes the metadata phase because its output is
required to produce the final rendered recipe:

```console
rattler-build build --recipe examples/metadata-step --experimental --render-only
```

Sources are fetched and prepared before metadata runs. `SRC_DIR` points to that
source tree, while `RECIPE_DIR` remains available for recipe-local helpers.
