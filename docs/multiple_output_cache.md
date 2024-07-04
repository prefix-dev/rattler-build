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

## Run exports from the cache

Since the cache output also has build- and host requirements we need to additionally take care of eventual "run-exports" from the cache output.
Run exports from the cache-dependencies are handled very similar to the run exports from a given output. We append any run exports to the outputs.

If the cache has an "ignore run exports" section, than we apply those filters at the cache level. If the output ignores any run exports, then we also ignore the run-exports if they would come from the cache.

## Caching in the $SRC_DIR

If you used `conda-build` a lot, you might have noticed that a top-level build is also caching the changes in the `$SRC_DIR`. This is not the case for `rattler-build` yet.

You could try to work around by e.g. copying files into the `$PREFIX` and restoring them in each output.
