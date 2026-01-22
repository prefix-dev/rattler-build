# Staging outputs for multiple packages

!!!note

    Staging outputs are different from a compilation cache. If you look for tips and tricks on how to use `sccache` or `ccache` with `rattler-build`, please refer to the [tips and tricks section](tips_and_tricks.md#using-sccache-or-ccache-with-rattler-build).

Sometimes you build a package and want to split the contents into multiple sub-packages.
For example, when building a C/C++ package, you might want to create multiple packages for the
runtime requirements (library), and the development time requirements such as header files.

Staging outputs make this easy. A staging output runs its build script once, then copies its files directly into each inheriting package's prefix. Since these are "new" files in the prefix, they will be included in the output package.

Let's take a look at an example:

```yaml title="recipe.yaml"
recipe:
  name: mypackage
  version: '0.1.0'

source:
  - url: https://example.com/library.tar.gz
    sha256: 1234567890abcdef1234567890abcdef1234567890abcdef1234567890abcdef

outputs:
  # Staging output - builds once and caches results
  - staging:
      name: mypackage-build
    requirements:
      build:
        - ${{ compiler('c') }}
    build:
      script:
        - mkdir -p $PREFIX/lib
        - mkdir -p $PREFIX/include
        - echo "This is the library" > $PREFIX/lib/library.txt
        - echo "This is the header" > $PREFIX/include/header.txt

  # First package output inheriting from staging
  - package:
      name: mypackage-library
    inherit: mypackage-build
    build:
      files:
        - lib/*

  # Second package output inheriting from staging
  - package:
      name: mypackage-headers
    inherit: mypackage-build
    build:
      files:
        - include/*
```

!!!note

    Since this is an experimental feature, you need to pass the `--experimental` flag to enable parsing of staging outputs.

In this example, we have a staging output called `mypackage-build` that creates files during its build. The two package outputs `mypackage-library` and `mypackage-headers` inherit from it using the `inherit:` key.

When building, the staging output runs first and creates files in `$PREFIX`. These files are then copied into the `$PREFIX` of each inheriting output package.
The easiest way to select a subset of the files in the prefix is by using the `files` field in the output definition.
You can use a list of globs to select only the files that you want.

For something more complicated you can also use `include` and `exclude` fields in the `files` selector. Please refer to the [the build options documentation](build_options.md#include-only-certain-files-in-the-package).

### Run exports from staging

Since the staging output also has build- and host requirements we need to additionally take care of eventual "run-exports" from the staging output.
Run exports from the staging dependencies are handled very similar to the run exports from a given output. We append any run exports to the inheriting outputs.

You can control whether run exports are inherited using the extended `inherit:` syntax:

```yaml
# Simple inherit (includes run exports by default)
inherit: staging-name

# Extended inherit with run exports control
inherit:
  from: staging-name
  run_exports: false  # Disable inheriting run exports
```

If the staging output has an "ignore run exports" section, those filters are applied at the staging level. If an inheriting output ignores any run exports, then we also ignore the run-exports if they would come from the staging.

### Source code with staging

The top-level `source` section provides source code that is available to both the staging output and all package outputs. For every output, the (dirty) source is restored from the staging directory. Outputs can layer additional files on top of the staging source.

If you already ran `cmake` in the staging output, you can continue from where the build left off in subsequent outputs. This is useful when you want to e.g. build additional components (such as Python bindings) on top of the already-built library.


## C++ Example that builds Python bindings on top of a library

You can find an example (with source code) here: [Link](https://github.com/wolfv/rattler-build-cache-test/).

```yaml title="variants.yaml"
python:
  - "3.12.*"
  - "3.11.*"
```

And the corresponding recipe:

```yaml title="recipe.yaml"
recipe:
  name: calculator
  version: 1.0.0

source:
  path: ../

outputs:
  # Staging output - builds the C++ library once
  - staging:
      name: calculator-build
    requirements:
      build:
        - ${{ compiler('cxx') }}
        - cmake
        - ninja
    build:
      script:
        # make sure that `alternative_name.md` is not present
        - test ! -f ./alternative_name.md
        - mkdir build
        - cd build
        - cmake $SRC_DIR -GNinja ${CMAKE_ARGS}
        - ninja install

  # This output inherits all files installed during the staging build
  - package:
      name: libcalculator
    inherit: calculator-build
    requirements:
      run_exports:
        - ${{ pin_subpackage('libcalculator') }}

  # This output builds Python bindings on top of the staged build
  - package:
      name: py-calculator
    inherit: calculator-build
    source:
      - path: ../README.md
        file_name: alternative_name.md

    requirements:
      build:
        - ${{ compiler('cxx') }}
        - cmake
        - ninja
      host:
        - pybind11
        - python
        - libcalculator

    build:
      script:
        # assert that the README.md file is present
        - test -f ./alternative_name.md
        - cd build
        - cmake $SRC_DIR -GNinja ${CMAKE_ARGS} -DBUILD_PYTHON_BINDINGS=ON
        - ninja install
```
