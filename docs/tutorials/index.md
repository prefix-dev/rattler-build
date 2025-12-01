# Examples

This section contains examples for packaging software in different languages with `rattler-build`. Each example walks you through creating a recipe for a specific language ecosystem.

| Example | Description |
|---------|-------------|
| [Python](python.md) | Build pure Python packages with `noarch: python` and compiled packages like NumPy |
| [C++](cpp.md) | Package header-only and compiled C++ libraries using CMake |
| [JavaScript](javascript.md) | Create packages for NodeJS applications using NPM |
| [Rust](rust.md) | Build Rust packages with proper license bundling using `cargo-bundle-licenses` |
| [Go](go.md) | Package Go applications with `go-cgo` or `go-nocgo` compilers |
| [Perl](perl.md) | Build Perl packages from CPAN with `noarch: generic` support |
| [R](r.md) | Package R libraries from CRAN |
| [Repackaging](repackaging.md) | Repackage existing pre-built binaries for distribution |
| [Converting from conda-build](../converting_from_conda_build.md) | Migrate existing `meta.yaml` recipes to the `rattler-build` format |
