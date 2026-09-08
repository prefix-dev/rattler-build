# Reusable step-provider examples

These small, self-contained recipes demonstrate the CMake, Meson, Rust, and Go
providers published to `beta.prefix.dev/wolfv/rattler-build-steps`.

Build one with the experimental reusable-step support enabled:

```bash
rattler-build build \
  --experimental \
  --channel https://beta.prefix.dev/wolfv/rattler-build-steps \
  --channel conda-forge \
  --recipe examples/step-providers/cmake
```

Replace `cmake` with `meson`, `rust`, or `go` for the other examples. Provider
packages are resolved into independent build-platform environments. Tools
declared in each action document's `requirements.build`, such as CMake or Cargo,
are added to the recipe build environment; provider package dependencies are not.
Provider documents must use the strict action schema (`inputs`, action-owned
`requirements`, and a required `steps` list). Older provider documents using
shorthand inputs or invocation-level run fields must be updated before use.

The references use compatible version ranges so provider updates are explicit
and reproducible in the rendered recipe and package hash:

- `cmake:build@0.3.*`
- `meson:build@0.3.*`
- `rust:build@0.3.*`
- `go:build@0.4.*`
