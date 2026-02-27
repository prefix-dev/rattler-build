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

If the staging output has an `ignore_run_exports` section, those filters are
applied at the staging level before run exports reach any inheriting package.
If an inheriting output also ignores run exports, those filters are applied
additionally.

You can filter run exports at the staging level using `from_package` or
`by_name`:

```yaml
outputs:
  - staging:
      name: build-stage
    requirements:
      host:
        - some-dep
      ignore_run_exports:
        from_package:
          - some-dep       # ignore run exports originating from some-dep
        # alternatively:
        # by_name:
        #   - some-dep     # ignore run exports matching the name "some-dep"

  - package:
      name: mypkg
    inherit: build-stage
```

### Source code with staging

The top-level `source` section provides source code that is available to both the staging output and all package outputs. For every output, the (dirty) source is restored from the staging directory. Outputs can layer additional files on top of the staging source.

If you already ran `cmake` in the staging output, you can continue from where the build left off in subsequent outputs. This is useful when you want to e.g. build additional components (such as Python bindings) on top of the already-built library.

### Work directory caching

The staging cache preserves not just prefix files but also the **entire work
directory** from the staging build. When a package output inherits from staging,
both the prefix and the work directory are restored. This means that build
artifacts like compiled object files, CMake build directories, and generated
configuration files are available to the inheriting package's build script.

For example, if your staging output runs `cmake` and `make`, an inheriting
package can `cd build && make install` additional targets without recompiling
from scratch.

### Top-level inheritance

In recipes that have both a top-level `build:` section and staging outputs,
package outputs can choose where to inherit from. By default, listing
`inherit: cache-name` inherits from a staging cache. To inherit from the
top-level build instead, use `inherit: null`:

```yaml
build:
  script:
    - if: unix
      then: |
        mkdir -p $PREFIX/share
        echo "data" > $PREFIX/share/data.txt

outputs:
  - staging:
      name: compile-stage
    build:
      script:
        - if: unix
          then: |
            mkdir -p $PREFIX/lib
            echo "compiled.so" > $PREFIX/lib/compiled.so

  # Inherits compiled library from staging
  - package:
      name: compiled-pkg
    inherit: compile-stage
    build:
      files:
        - lib/**

  # Inherits data files from the top-level build
  - package:
      name: data-pkg
    inherit: null
    build:
      files:
        - share/**
```

### Multiple staging caches

A recipe can define multiple independent staging outputs. Each staging output is
built and cached separately, and different package outputs can inherit from
different staging caches:

```yaml
outputs:
  # First staging output - builds core C library
  - staging:
      name: core-build
    requirements:
      build:
        - ${{ compiler('c') }}
        - cmake
      host:
        - zlib
    build:
      script:
        - cmake -B build && cmake --build build --target install

  # Second staging output - builds Python bindings
  - staging:
      name: python-build
    requirements:
      build:
        - python
        - setuptools
      host:
        - python
    build:
      script:
        - python -m pip install . --prefix=$PREFIX

  # Inherits from core-build
  - package:
      name: libcore
    inherit: core-build
    build:
      files:
        - lib/**

  # Inherits from core-build (different file selection)
  - package:
      name: core-headers
    inherit: core-build
    build:
      files:
        - include/**

  # Inherits from python-build
  - package:
      name: python-mycore
    inherit: python-build
    requirements:
      run:
        - python
```


### Variants and staging

Staging caches interact with [variant configuration](variants.md). The cache
key includes only the variant variables that are referenced in the staging
output's requirements. This means:

- Different variants produce different staging caches
- The staging build is only rerun when its relevant variant keys change
- Inheriting packages can add their own variant dimensions (e.g. a `python`
  version) on top of the staging cache

For example, if a staging output depends on `libfoo` and `libfoo` has variants
`[1.0, 2.0]`, the staging build runs once per `libfoo` variant. An inheriting
package that additionally depends on `python` expands the matrix further (one
package per `libfoo` × `python` combination), but the staging cache is reused
across `python` variants.

### How caching works

The staging cache is keyed by a SHA256 hash over:

- The staging output's resolved requirements (build and host dependencies)
- Relevant variant variables (only those referenced in the staging requirements)
- `host_platform` and `build_platform` (always included)

Staging caches are stored under `output/build_cache/staging_<hash>/`. Each
cache directory contains:

```txt
output/build_cache/staging_<sha256>/
├─ metadata.json    # Cache metadata (deps, sources, file lists, variant)
├─ prefix/          # Cached prefix files (only files added by the build script)
└─ work_dir/        # Cached work directory
```

On a **cache hit**, the staging build script is skipped entirely — the cached
prefix and work directory files are restored directly. On a **cache miss**, the
full build runs and the results are cached for future use.

To force a staging cache rebuild, delete the corresponding directory under
`output/build_cache/`.

### Symlink handling

Symlinks created during the staging build are preserved in the cache. Both
relative and absolute symlinks are cached and restored correctly, including
broken symlinks (symlinks whose target does not exist). On Unix systems,
symbolic links are used; on Windows, junction points are created where
applicable.

### File capture

Only files **added** by the build script are cached — files that were already
present in the host environment from dependencies are excluded. This means the
staging cache contains exactly the files that the build script installed into
`$PREFIX`, not the entire environment.


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
