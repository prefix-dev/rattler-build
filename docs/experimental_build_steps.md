# Build steps (experimental)

Build steps split a package build into named operations. Reusable **actions**
share those operations between recipes; **metadata steps** can discover recipe
requirements before the final environments are solved.

!!! warning "Experimental interface"
    All features in this guide require `--experimental`. Their schemas and CLI
    may change. Keep `build.script` for simple builds that do not need these
    capabilities—you do not need to migrate every recipe.

## Start with a named build

Save this as `recipe.yaml`:

```yaml
schema_version: 1
package:
  name: step-hello
  version: "1.0"
build:
  noarch: generic
  steps:
    - name: prepare
      requirements:
        build: [python]
      interpreter: python
      run: |
        import os
        from pathlib import Path
        Path(os.environ["PREFIX"], "greeting.txt").write_text("hello")
    - name: check
      optional: true
      depends_on: [prepare]
      interpreter: python
      run: |
        import os
        from pathlib import Path
        assert Path(os.environ["PREFIX"], "greeting.txt").read_text() == "hello"
```

Run a development check without packaging:

```console
rattler-build run check --recipe . --experimental
```

`check` pulls in `prepare` and its Python requirement. Each step executes in its
own scope. The environment and work directory are retained between `run`
commands. Use `--source-dir .` to work directly in an already-prepared checkout.

Create the package with:

```console
rattler-build build --recipe . --experimental
```

A normal build executes `prepare` and skips optional `check`. Optional development
steps are **not package tests**; use the recipe's `tests` section for tests that
must run when packaging.

## Choose the right feature

| You want to… | Use | Read |
| --- | --- | --- |
| Run one operation and its prerequisites | Named `build.steps` and `run NAME` | [Named steps](build_steps/named_steps.md) |
| Share a build pipeline with typed parameters | A local or packaged `uses` action | [Actions and providers](build_steps/actions.md) |
| Read sources to discover dependencies or generate a pipeline | `build.metadata` | [Pre-solve recipe generation](build_steps/metadata.md) |
| Add licenses or runtime dependencies after compilation | A step's `OUTPUT_FILE` | [Post-build metadata](build_steps/outputs.md) |
| Avoid rerunning unchanged work | `RATTLER_BUILD_STEP_CACHE` | [Caching and staging](build_steps/caching.md) |
| Update an older experimental provider | A newly versioned action package | [Migration](build_steps/migration.md) |

## How the pieces fit together

```text
recipe + variants
    │
    ├─ if build.metadata is present:
    │    compile bootstrap action → fetch sources → solve bootstrap environment
    │    → run metadata → patch recipe source
    │
    └─ compile final actions + select named steps + expand final variants
         → solve build/host environments → execute flat steps
         → apply post-build metadata → package and test
```

A packaged provider is a **source transport**, not a runtime environment. Its
YAML compiles through the same compiler as a local action. Action-owned
requirements supply the tools used by the final build. Rendered plans contain
executable steps and provider provenance, so rebuilds do not need the original
action documents.

!!! warning "Metadata executes during rendering"
    `build.metadata` runs recipe code even with `--render-only`, because its output
    is needed to discover the final recipe. Do not render untrusted experimental
    metadata recipes. Provider documents may also need downloading during rendering.

## Reference topics

The following headings preserve links to the original single-page guide.

### Caching build steps

See [cache declarations, invalidation, and replay](build_steps/caching.md).

### Reusable steps

See [typed inputs and local action composition](build_steps/actions.md#reusable-steps).

### Packaged actions

See [provider packages and provenance](build_steps/actions.md#packaged-actions).

### Staging steps

See [staging cache execution and metadata boundaries](build_steps/caching.md#staging-steps).

### Post-build outputs

See [the output protocol and permitted fields](build_steps/outputs.md).

### Pre-solve metadata step

See [bootstrap environments and generated recipes](build_steps/metadata.md).
