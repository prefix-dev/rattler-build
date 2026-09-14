# Migrating experimental providers

The current action compiler reads strict YAML. It does not render an entire
document through Jinja first. Publish a new compatible provider version when
changing its contract; editing a consumer recipe cannot update an already
published artifact.

## Replace document-level templates

Do not wrap steps in `{% if %}` blocks. Keep valid YAML and put the condition
on the step itself:

```yaml
schema_version: 1
inputs:
  install:
    type: boolean
    default: true
  extra_args:
    type: list
    items: string
    default: []
requirements:
  build: [cmake, ninja]
steps:
  - name: configure
    run: cmake -S "$SRC_DIR" -B "$BUILD_DIR/cmake" -G Ninja ${{ inputs.extra_args | join(' ') }}
  - name: build
    depends_on: [configure]
    run: cmake --build "$BUILD_DIR/cmake"
  - name: install
    if: inputs.install
    depends_on: [build]
    run: cmake --install "$BUILD_DIR/cmake" --prefix "$PREFIX"
```

Every input needs an explicit type; lists also need a scalar `items` type.
An invocation can supply `with`, `name`, `if`, `optional`, and `depends_on`, but
cannot override `env`, `cwd`, `interpreter`, or `requirements`. Those belong to
the action document or its executable steps.

## Declare runtime support explicitly

`RATTLER_BUILD_PROVIDER_PREFIX` is no longer supplied. A provider's downloaded
files are compilation inputs, not an executable environment.

If an action needs an external helper, declare the helper package under
`requirements.build` and load it from `BUILD_PREFIX`. This also applies to
metadata backends. The
[Python metadata provider](https://github.com/prefix-dev/rattler-build/tree/main/examples/python-metadata-backend)
demonstrates an explicit backend-package requirement and a Python wrapper.

## Emit licenses through the output protocol

The old per-step `license_files` field is not part of the action schema.
Package steps should emit `about.license_file.include.append` to `OUTPUT_FILE`;
see [post-build metadata](outputs.md) for a complete example. Preserve a
late-bound `${{ BUILD_DIR }}` expression when referencing generated files,
rather than emitting an unrestricted absolute license path.

Staging steps cannot emit package metadata. Move license collection and other
metadata-producing actions onto the inheriting package output.

## Compatible example versions

The examples use these provider versions on
`https://beta.prefix.dev/wolfv/rattler-build-steps`:

| Provider | Current example reference | Older incompatible series |
| --- | --- | --- |
| CMake | `cmake:build@0.4.*` | `0.3.*` |
| Meson | `meson:build@0.4.*` | `0.3.*` |
| Rust | `rust:build@0.4.*` | `0.3.*` |
| Go | `go:build@0.5.*` | `0.4.*` |
| Python metadata | `python:metadata@0.2.*` | `0.1.*` |

Python provider `0.2.0` build `1` also removes old wheels before a persistent
build, so a project version change does not leave multiple install candidates.

Sources and packaging recipes for the four non-Python providers are checked in
under `examples/step-providers/providers`. See their
[build and publication instructions](https://github.com/prefix-dev/rattler-build/tree/main/examples/step-providers).
A local channel lets you test a migrated provider before publishing it remotely.

## Boundaries to account for

- Pre-solve metadata is currently limited to single-output recipes. It cannot
  change package identity, sources, or the output list.
- The CLI performs metadata execution and provider resolution. The low-level
  synchronous recipe renderer is not a substitute for that full pipeline.
- `run` refuses to reuse prepared sources whose recipe path or source definitions
  changed. Preserve edits and move the work directory aside, or explicitly
  choose an already-prepared checkout with `--source-dir`.
- Cache declarations verify artifacts, not restore them. Include output globs
  when the step creates files needed by later operations.
