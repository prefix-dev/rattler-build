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

- `cmake:build@0.4.*`
- `meson:build@0.4.*`
- `rust:build@0.4.*`
- `go:build@0.5.*`

## Building the provider packages

Provider sources and package recipes live in `providers/{cmake,meson,rust,go}`.
From the repository root, build and publish a provider with:

```console
rattler-build build --recipe examples/step-providers/providers/cmake --output-dir output/providers
rattler-build publish output/providers/noarch/cmake-rattler-build-steps-0.4.0-*.conda --to prefix://beta.prefix.dev/wolfv/rattler-build-steps
```

For a local channel, replace `--to` with a directory path and pass its `file://`
URL as the consumer's first channel. Publishing remotely requires upload access.

These versions replace the old whole-document Jinja providers. Conditions now
use step `if` selectors, and list inputs declare their scalar `items` type.
The Go license collector emits the post-build `OUTPUT_FILE` protocol and requires
rattler-build's post-build metadata support. It belongs on a package output,
not a staging output.
