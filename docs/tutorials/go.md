# Packaging a Go package

This tutorial will guide you through making a Go package with `rattler-build`.

When building a recipe for Go, most Go dependencies are linked statically. That
means, we should collect their licenses and add them in the package. The
`go-licenses` tool can help you with this task - as shown in the example below.

## The different Go compilers

The `conda-forge` ecosystem provides two go compilers: `go-cgo` and `go-nocgo`.

By default, if you do not need to link against C libraries, it's recommended to
use the `go-nocgo` compiler. It generates fat binaries without libc
dependencies. The compiler activation scripts will set your `CC`, `CXX` and
related flags to invalid values.

The `go-cgo` compiler can generate fat binaries that depend on conda-forge's
libc. You should use this compiler if the underlying program needs to link
against other C libraries, in which case make sure to add `${{ compiler('c') }}`
(`cxx`, `fortran`, ...) for unix and the `m2w64` equivalent for windows.

## Example Go recipe

This example shows how to package the [Temporal
CLI](https://github.com/temporalio/cli).

```yaml title="recipe.yaml"
context:
    version: "0.13.1"

package:
  name: temporal
  version: ${{ version }}

source:
  url: https://github.com/temporalio/cli/archive/refs/tags/v${{ version }}.tar.gz
  sha256: 9d8812c96d3404490659fec3915dcd23c4142b421ef4cb7e9622bd9a459e1f74

build:
  number: 0

requirements:
  build:
    - ${{ compiler('go-nocgo') }}
    - go-licenses

tests:
  - script:
      - temporal --version

about:
  homepage: https://temporal.io
  repository: https://github.com/temporalio/cli
  documentation: https://docs.temporal.io/cli
  summary: Temporal CLI
  description: |
    Command-line interface for running Temporal Server and interacting with
    Workflows, Activities, Namespaces, and other parts of Temporal.
  license: MIT
  license_file:
    - LICENSE
    # These license files are generated at build time in the `build.sh` script
    # from all the dependencies of `temporal.io`.
    - license-files/
```

The build script (on Unix) should look something like this:

```sh title="build.sh"
# The LDFLAGS are used to set the version of the `temporal` binary. This is a common practice in Go.
export LDFLAGS="${LDFLAGS} -s -w -X github.com/temporalio/cli/temporalcli.Version=${PKG_VERSION}"

# Build the `temporal` binary and store it in the `$PREFIX/bin` directory.
go build -ldflags "$LDFLAGS" -o $PREFIX/bin/temporal ./cmd/temporal

# Store the license files in a separate directory in the $SRC_DIR. These are embedded in the package
# in the `license_file` section.
go-licenses save ./cmd/temporal --save_path="$SRC_DIR/license-files/" || true
```
