# Building a Rust package

We're using `rattler-build` to build a Rust package for the `cargo-edit` utility.
This utility manages Cargo dependencies from the command line.

```yaml title="recipe.yaml"
context:
  version: "0.11.9"

package:
  name: cargo-edit
  version: ${{ version }}

source:
  url: https://github.com/killercup/cargo-edit/archive/refs/tags/v${{ version }}.tar.gz
  sha256: 46670295e2323fc2f826750cdcfb2692fbdbea87122fe530a07c50c8dba1d3d7

build:
  script:
    - cargo-bundle-licenses --format yaml --output ${SRC_DIR}/THIRDPARTY.yml  # !(1)
    - $BUILD_PREFIX/bin/cargo install --locked --bins --root ${PREFIX} --path .

requirements:
  build:
    - ${{ compiler('rust') }}
    - cargo-bundle-licenses

tests:
  - script:
      - cargo-upgrade --help # !(2)

about:
  homepage: https://github.com/killercup/cargo-edit
  license: MIT
  license_file:
    - LICENSE
    - THIRDPARTY.yml
  description: "A utility for managing cargo dependencies from the command line."
  summary: "A utility for managing cargo dependencies from the command line."
```

!!! note
    The `${{ compiler(...) }}` functions are very useful in the context of cross-compilation.
    When the function is evaluated it will insert the correct compiler (as selected with the variant config) as well the `target_platform`.
    The "rendered" compiler will look like `rust_linux-64` when you are targeting the `linux-64` platform.

    You can read more about this in the [cross-compilation](../compilers.md) section.

1. The `cargo-bundle-licenses` utility is used to bundle all the licenses of the dependencies into a `THIRDPARTY.yml` file.
   This file is then included in the package. You should always include this file in your package when you are redistributing it.
2. Running scripts in `bash` or `cmd.exe` to test the package build well, expects an exit code of `0` to pass the test.


To build this recipe, simply run:

```bash
rattler-build build \
    --recipe ./cargo-edit/recipe.yaml
```
