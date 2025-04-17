# The cache for multiple outputs

!!!note

    The "multi-output" cache is a little bit different from a compilation cache. If you look for tips and tricks on how to use `sccache` or `ccache` with `rattler-build`, please refer to the [tips and tricks section](tips_and_tricks.md#using-sccache-or-ccache).

Sometimes you build a package and want to split the contents into multiple sub-packages.
For example, when building a C/C++ package, you might want to create multiple packages for the
runtime requirements (library), and the development time requirements such as header files.

The "cache" output makes this easy. It allows you to specify a single top-level cache that can produce arbitrary
files, that can then be used in other packages.

Let's take a look at an example:

```yaml title="recipe.yaml"
recipe:
  name: mypackage
  version: '0.1.0'

cache:
  source:
    - url: https://example.com/library.tar.gz
      sha256: 1234567890abcdef1234567890abcdef1234567890abcdef1234567890abcdef

  requirements:
    build:
      - ${{ compiler('c') }}

  build:
    script:
      - mkdir -p $PREFIX/lib
      - mkdir -p $PREFIX/include
      - echo "This is the library" > lib/library.txt
      - echo "This is the header" > include/header.txt

outputs:
  - package:
      name: mypackage-library
    build:
      files:
        - lib/*

  - package:
      name: mypackage-headers
    build:
      files:
        - include/*
```

!!!note

    Since this is an experimental feature, you need to pass the `--experimental` flag to enable parsing of the `cache` top-level section.

In this example, we have a single package called `mypackage` that creates two outputs: `mypackage-library` and `mypackage-headers`.
The cache output will run like a regular output, but after the build is finished, the files will be copied to a "cache" directory (in your output folder, under `output/build_cache`).

The files in the cache folder are then copied into the `$PREFIX` of each output package. Since they are "new" files in the prefix, they will be included in the output package.
The easiest way to select a subset of the files in the prefix is by using the `files` field in the output definition.
You can use a list of globs to select only the files that you want.

For something more complicated you can also use `include` and `exclude` fields in the `files` selector. Please refer to the [the build options documentation](build_options.md#include-only-certain-files-in-the-package).

### Run exports from the cache

Since the cache output also has build- and host requirements we need to additionally take care of eventual "run-exports" from the cache output.
Run exports from the cache-dependencies are handled very similar to the run exports from a given output. We append any run exports to the outputs.

If the cache has an "ignore run exports" section, than we apply those filters at the cache level. If the output ignores any run exports, then we also ignore the run-exports if they would come from the cache.

### Source code in the cache

The cache output has its own `source` section. For every output, the (dirty) source is restored from the cache directory. Outputs can layer additional files on top of the cache source.
However, if you already ran `cmake` in the cache output, you can continue from where the build left off. This is useful when you want to e.g. build additional components (such as Python bindings) on top of the already-built library.


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

cache:
  source:
    path: ../

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

outputs:
  # this first output will include all files installed during the cache build
  - package:
      name: libcalculator

    requirements:
      run_exports:
        - ${{ pin_subpackage('libcalculator') }}
  # This output will build the Python bindings using CMake and then create new
  # packages with the Python bindings
  - package:
      name: py-calculator
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
