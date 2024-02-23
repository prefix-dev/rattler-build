# Building a Rust package

Building a Rust package is very straightforward with `rattler-build`. In this
example, we build the a package for the `cargo-edit` utility, which is a utility
for managing Cargo dependencies from the command line.

One tiny challenge is that the Rust compiler is not "pre-configured" and we need to
add a `variant_config.yaml` file to the package:

```yaml title="variant_config.yaml"
rust_compiler: rust
```

This will tell `rattler-build` what to insert for the `${{ compiler('rust') }}` Jinja function.

!!! note
    The `${{ compiler(...) }}` functions are very useful in the context of
    cross-compilation. When the function is evaluated it will insert the correct
    compiler (as selected with the variant config) as well the
    `target_platform`. The "rendered" compiler will look like `rust_linux-64`
    when you are targeting the `linux-64` platform.

    You can read more about this in the [cross-compilation](../compilers.md) section.

Then we can write the recipe for the package like so:

```yaml title="recipe.yaml"
# yaml-language-server: $schema=https://raw.githubusercontent.com/prefix-dev/recipe-format/main/schema.json

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
    # we bundle all the licenses of the dependencies into a THIRDPARTY.yml file and include it in the package
    - cargo-bundle-licenses --format yaml --output ${SRC_DIR}/THIRDPARTY.yml
    - $BUILD_PREFIX/bin/cargo install --locked --bins --root ${PREFIX} --path .

requirements:
  build:
    - ${{ compiler('rust') }}
    - cargo-bundle-licenses

tests:
  - script:
      - cargo-upgrade --help

about:
  homepage: https://github.com/killercup/cargo-edit
  license: MIT
  license_file:
    - LICENSE
    - THIRDPARTY.yml
  description: "A utility for managing cargo dependencies from the command line."
  summary: "A utility for managing cargo dependencies from the command line."
```

To build this recipe, simply run:

```bash
rattler-build build \
    --recipe ./cargo-edit/recipe.yaml \
    --variant-config ./cargo-edit/variant_config.yaml
```
