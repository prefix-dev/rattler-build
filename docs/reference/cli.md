# The CLI

Rattler-build comes with a command line interface (CLI) that can be used to interact with the tool.

## Global options
The options that work for all subcommands are:

- `--verbose (-v|vv|vvv)` Increase the verbosity of the output messages, the -v|vv|vvv increases the level of verbosity respectively.
- `--quiet (-q)`: Decreases the amount of output.
- `--log-style <STYLE>`: The style of the log output [env: `RATTLER_BUILD_LOG_STYLE=`] [default: `fancy`] [possible values: `fancy`, `json`, `plain`].
- `--color <COLOR>`: Whether the log needs to be colored [env: `RATTLER_BUILD_COLOR=`] [default: `auto`] [possible values: `always`, `never`, `auto`].
  Rattler-build also honors the `CLICOLOR` and `CLICOLOR_FORCE` environment variables.
- `--help (-h)` Shows help information, use `-h` to get the short version of the help.
- `--version (-V)`: shows the version of pixi that is used.

## `build`
Build a package from a recipe.

##### Options
- `--recipe <RECIPE> (-r)`: The recipe file or directory containing `recipe.yaml`. Defaults to the current directory [default: .]
- `--recipe-dir <RECIPE_DIR>`: The directory containing the recipe.
- `--up-to <UP_TO>`: Build recipes up to the specified package.
- `--build-platform <BUILD_PLATFORM>`: The build platform to use for the build (e.g. for building with emulation, or rendering) [default: the platform of the current system]
- `--target-platform <TARGET_PLATFORM>`: The target platform to build for [default: the platform of the current system]
- `--channel <CHANNEL> (-c)`: Add a channel to search for dependencies in [default: `conda-forge`]
- `--variant-config <VARIANT_CONFIG> (-m)`: The variant configuration for the build.
- `--render-only`: Render the recipe files without executing the build.
- `--with-solve`: Render the recipe files with solving the dependencies.
- `--keep-build`: Keep intermediate build artifacts after the build.
- `--no-build-id`: Don't use build id(timestamp) when creating build directory name.
- `--package-format <PACKAGE_FORMAT>`: The package format to use for the build. Can be one of `tar-bz2` or `conda`. 
    You can also add a compression level to the package format, e.g. `tar-bz2:<number>` (from 1 to 9) or `conda:<number>` (from -7 to 22 or `max`) [default: conda]
- `--compression-threads <COMPRESSION_THREADS>`: The number of threads to use for compression (only relevant when also using `--package-format conda`)
- `--no-include-recipe`: Don't store the recipe in the final package.
- `--no-test`: Don't run the tests after building the package.
- `--color-build-log`: Don't force colors in the output of the build script.
- `--output-dir <OUTPUT_DIR>`: Output directory for build artifacts. [default: `./output`] [env: CONDA_BLD_PATH=]
- `--experimental`: Enable [experimental features](../experimental_features.md). [env: RATTLER_BUILD_EXPERIMENTAL=] 
- `--skip-existing [<SKIP_EXISTING>]`: Whether to skip packages that already exist in any channel [default: none] [possible values: none, local, all]

When you are testing parts of your build you could try some of the following commands:
```shell
# Only render the recipe files into raw recipes
rattler-build build --render-only
# Render the recipe files and solve the dependencies
rattler-build build --with-solve
# All previous commands but don't run the tests
rattler-build build --no-test
# Build all packages but stop at a specific package, based on a topological sort of the dependencies.
rattler-build build --up-to package_name
```

If you want to modify the outcome of the build, these are the options that modify the build process:
```shell
# Keep the build directory after the build
rattler-build build --keep-build
# Don't include the recipe in the final package
rattler-build build --no-include-recipe
# Don't use the build id when creating the build directory name
rattler-build build --no-build-id
# Skip packages that already exist, either locally or in any channel
rattler-build build --skip-existing all
# Change the package format to tar-bz2 with a compression level of 5
rattler-build build --package-format tar-bz2:5
```

For cross compilation or emulation, you can use the following options:
```shell
# Build for a specific platform
rattler-build build --build-platform linux-64
# Build for a specific target platform
rattler-build build --target-platform linux-64
```


