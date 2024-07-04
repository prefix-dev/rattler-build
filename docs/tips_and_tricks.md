# Tips and tricks for rattler-build

This section contains some tips and tricks for using `rattler-build`.

## Using sccache or ccache with `rattler-build`

When debugging a recipe it can help a lot to use `sccache` or `ccache`. You can
install both tools e.g. with `pixi global install sccache`.

To use them with a CMake project, you can use the following variables:

```sh
export CMAKE_C_COMPILER_LAUNCHER=sccache
export CMAKE_CXX_COMPILER_LAUNCHER=sccache

# or more generally

export C="sccache $C"
export CXX="sccache $CXX"
```

However, both `ccache` and `sccache` are sensitive to changes in the build
location. Since `rattler-build`, by default, always creates a new build
directory with the timestamp, you need to use the `--no-build-id` flag. This
will disable the time stamp in the build directory and allow `ccache` and
`sccache` to cache the build.

```sh
rattler-build build --no-build-id --recipe ./path/to/recipe.yaml
```

## Building your own "forge"

You might want to publish your own software packages to a channel you control.
These might be packages that are not available in the main conda-forge channel,
or proprietary packages, or packages that you have modified in some way.

Doing so is pretty straightforward with `rattler-build` and a CI provider of
your choice. We have a number of example repositories for "custom" forges:

- [rust-forge](https://github.com/wolfv/rust-forge): This repository builds a
  number of Rust packages for Windows, macOS and Linux on top of Github Actions.
- [r-forge](https://github.com/wolfv/r-forge): The same idea, but for `R`
  packages

### Directory structure

To create your own forge, you should create a number of sub-directories where
each sub-directory should contain at most one recipe. With the `--recipe-dir`
flag of rattler-build, the program will go and collect all recipes it finds in
the given directory or sub-directories.

We can combine this with the `--skip-existing=all` flag which will skip all
packages that are already built locally or in the channel (if you upload them).
Using `all` will also look at the `repodata.json` file in the channel to see if
the package is already there. Packages are skipped based on their complete name,
including the version and build string.

To note: the build string changes if the variant configuration changes! So if
you update a package in the variant configuration, the packages that need
rebuilding should be rebuilt.

!!!note

    You can generate recipes for different ecosystems with the `rattler-build generate-recipe` command.
    Read more about it in the [Generating recipes](recipe_generation.md) section.

### CI setup

As an example, the following is the CI setup for `rust-forge`. The workflow uses
`rattler-build` to build and upload packages to a custom channel on
[https://prefix.dev](https://prefix.dev) – but you can also use `rattler-build`
to upload to your own `quetz` instance, or a channel on `anaconda.org`.

??? tip "Example CI setup for `rust-forge`"

    The following is an example of a Github Actions workflow for `rust-forge`:

    ```yaml title=".github/workflows/forge.yml"
    name: Build all packages

    on:
      push:
        branches:
          - main
      workflow_dispatch:
      pull_request:
        branches:
          - main

    jobs:
      build:
        strategy:
          matrix:
            include:
              - { target: linux-64, os: ubuntu-20.04 }
              - { target: win-64, os: windows-latest }
              # force older macos-13 to get x86_64 runners
              - { target: osx-64, os: macos-13 }
              - { target: osx-arm64, os: macos-14 }
          fail-fast: false

        runs-on: ${{ matrix.os }}
        steps:
          - uses: actions/checkout@v4
            with:
              fetch-depth: 2
          - uses: prefix-dev/setup-pixi@v0.5.1
            with:
              pixi-version: v0.24.2
              cache: true

          - name: Run code in changed subdirectories
            shell: bash
            env:
              TARGET_PLATFORM: ${{ matrix.target }}

            run: |
              pixi run rattler-build build --recipe-dir . \
                --skip-existing=all --target-platform=$TARGET_PLATFORM \
                -c conda-forge -c https://prefix.dev/rust-forge

          - name: Upload all packages
            shell: bash
            # do not upload on PR
            if: github.event_name == 'push'
            env:
              PREFIX_API_KEY: ${{ secrets.PREFIX_API_KEY }}
            run: |
              # ignore errors because we want to ignore duplicate packages
              for file in output/**/*.conda; do
                pixi run rattler-build upload prefix -c rust-forge "$file" || true
              done
    ```
