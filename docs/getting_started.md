# Getting started

This tutorial walks you through building and publishing your first conda package with `rattler-build`.

## Creating a recipe

A recipe is a YAML file that describes how to build a package. Create a file called `recipe.yaml`:

```yaml title="recipe.yaml"
context:
  version: "1.0.0"

package:
  name: hello-world
  version: ${{ version }}

build:
  number: 0
  script:
    - mkdir -p $PREFIX/bin
    - echo '#!/bin/bash' > $PREFIX/bin/hello
    - echo 'echo "Hello, World!"' >> $PREFIX/bin/hello
    - chmod +x $PREFIX/bin/hello

tests:
  - script:
      - hello

about:
  summary: A simple hello world package
  license: MIT
```

This recipe creates a simple shell script that prints "Hello, World!".

## Building the package

Build the package with the `build` command:

```bash
rattler-build build --recipe recipe.yaml
```

The build process will:

1. Create an isolated build environment
2. Run the build script
3. Package the result into a `.conda` file
4. Run the tests to verify the package works

The output package will be in the `output/` directory (e.g., `output/linux-64/hello-world-1.0.0-h123abc_0.conda`).

### Adding dependencies

Most real-world packages need dependencies. Add them under the `requirements` section:

```yaml
requirements:
  build:
    - ${{ compiler('c') }}     # C compiler for compiled code
    - cmake                    # Build tools
    - ninja                    # Another build tool
  host:
    - openssl                  # Libraries to link against
    - python                   # Python for the build environment
  run:
    - python                   # Runtime dependencies
    - numpy >=1.20
```

- **build**: Tools needed to build (compilers, cmake, make)
- **host**: Libraries to link against during the build
- **run**: Dependencies needed when the package is installed

For complete examples, see the [Examples](tutorials/index.md) section covering
[Python](tutorials/python.md), [Rust](tutorials/rust.md), [C++](tutorials/cpp.md),
[Go](tutorials/go.md), and more.

### Common build options

```bash
# Add channels for dependencies (note: conda-forge is the default)
rattler-build build -c conda-forge -c bioconda

# Use variant configurations
rattler-build build -m variants.yaml
```

### Debugging failed builds

When a build fails, use `debug-shell` to enter an interactive shell in the build environment:

```bash
rattler-build debug-shell
```

This opens a shell with:

- All environment variables set (like `$PREFIX`, `$SRC_DIR`)
- The build and host environments activated
- The source code extracted and patches applied

From here, you can manually run the build script to debug issues:

```bash
# Run the build script that rattler-build generated
./conda_build.sh

# Open VSCode to edit files or the build script (don't forget to transfer the changes)
code .

# Create a patch after editing the files
rattler-build create-patch ...
```

This lets you inspect the environment, test commands interactively, and iterate quickly
without re-running the full build process. To learn more about debugging failed builds visit [Debugging Builds](debugging_builds.md).

## Publishing the package

Once your package is built, publish it to a channel with the `publish` command:

```bash
# Publish to prefix.dev
rattler-build publish recipe.yaml --to https://prefix.dev/my-channel

# Publish to anaconda.org
rattler-build publish recipe.yaml --to https://anaconda.org/my-username

# Publish to an S3 bucket
rattler-build publish recipe.yaml --to s3://my-bucket/my-channel

# Publish to a local directory
rattler-build publish recipe.yaml --to /path/to/local/channel
```

The `publish` command combines building and uploading in one step. You can also publish
pre-built packages directly:

```bash
rattler-build publish output/linux-64/hello-world-1.0.0-h123abc_0.conda --to https://prefix.dev/my-channel
```

### Authentication

Before publishing, authenticate with your channel:

```bash
# prefix.dev
rattler-build auth login prefix.dev --token <your-token>

# anaconda.org
rattler-build auth login anaconda.org --conda-token <your-token>
```

See [Server authentication](authentication_and_upload.md) for more details.

### Bumping the build number

When republishing a package with the same version (e.g., to pick up updated dependencies),
increment the build number:

```bash
# Automatically increment from the highest build number in the channel
rattler-build publish recipe.yaml --to https://prefix.dev/my-channel --build-number=+1

# Set an explicit build number
rattler-build publish recipe.yaml --to https://prefix.dev/my-channel --build-number=5
```

## Updating to a new version

When a new upstream version is released, use `bump-recipe` to update your recipe:

```bash
# Auto-detect latest version from source URL (GitHub, PyPI, crates.io)
rattler-build bump-recipe --recipe recipe.yaml

# Specify a version explicitly
rattler-build bump-recipe --recipe recipe.yaml --version 1.2.0

# Check for updates without modifying
rattler-build bump-recipe --recipe recipe.yaml --check-only
```

This command updates both the version and the SHA256 checksum in your recipe.

## Setting up GitHub Actions

Automate your builds with the [rattler-build-action](https://github.com/prefix-dev/rattler-build-action).

### Basic workflow

```yaml title=".github/workflows/build.yml"
name: Build Package

on:
  push:
    branches: [main]
  pull_request:

jobs:
  build:
    runs-on: ubuntu-latest
    steps:
      - uses: actions/checkout@v4
      - name: Build conda package
        uses: prefix-dev/rattler-build-action@v0.2.34
```

By default, this builds the recipe at `conda.recipe/recipe.yaml` and uploads the
built package as a GitHub Actions artifact.

### Multi-platform builds

Build for multiple platforms using a matrix:

```yaml title=".github/workflows/build.yml"
name: Build Package

on:
  push:
    branches: [main]

jobs:
  build:
    runs-on: ${{ matrix.os }}
    strategy:
      matrix:
        include:
          - os: ubuntu-latest
            target-platform: linux-64
          - os: macos-latest
            target-platform: osx-arm64
          - os: windows-latest
            target-platform: win-64
    steps:
      - uses: actions/checkout@v4
      - name: Build conda package
        uses: prefix-dev/rattler-build-action@v0.2.34
        with:
          artifact-name: package-${{ matrix.target-platform }}
          build-args: --target-platform ${{ matrix.target-platform }}
```

### Publishing with OIDC (no secrets required)

For prefix.dev, you can use trusted publishing with OIDC - no API keys needed:

```yaml title=".github/workflows/publish.yml"
name: Publish Package

on:
  release:
    types: [published]

permissions:
  contents: read
  id-token: write  # Required for OIDC

jobs:
  publish:
    runs-on: ubuntu-latest
    steps:
      - uses: actions/checkout@v4
      - name: Build conda package
        uses: prefix-dev/rattler-build-action@v0.2.34

      - name: Publish to prefix.dev
        run: |
          for pkg in $(find output -type f \( -name "*.conda" -o -name "*.tar.bz2" \)); do
            rattler-build upload prefix -c my-channel "$pkg"
          done
```

First, configure trusted publishing in your prefix.dev channel settings by adding
your GitHub repository and workflow.

### Publishing to anaconda.org

```yaml title=".github/workflows/publish.yml"
name: Publish Package

on:
  release:
    types: [published]

jobs:
  publish:
    runs-on: ubuntu-latest
    steps:
      - uses: actions/checkout@v4
      - name: Build conda package
        uses: prefix-dev/rattler-build-action@v0.2.34

      - name: Publish to anaconda.org
        run: |
          for pkg in $(find output -type f \( -name "*.conda" -o -name "*.tar.bz2" \)); do
            rattler-build upload anaconda -o my-org "$pkg"
          done
        env:
          ANACONDA_API_KEY: ${{ secrets.ANACONDA_API_KEY }}
```

### Action options

| Option | Description | Default |
|--------|-------------|---------|
| `recipe-path` | Path to the recipe file | `conda.recipe/recipe.yaml` |
| `build-args` | Additional arguments for `rattler-build build` | |
| `upload-artifact` | Upload built packages as artifacts | `true` |
| `artifact-name` | Name for the artifact (use with matrix builds) | `package` |
| `rattler-build-version` | Version of rattler-build to use | latest |

## Next steps

- Learn about [variants](variants.md) for building multiple configurations
- Explore [testing](testing.md) options for your packages
- See language-specific examples in the [Examples](tutorials/index.md) section
