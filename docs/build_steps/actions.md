# Reusable actions and providers

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



See [provider migration](migration.md) for schema changes and compatible example versions.
