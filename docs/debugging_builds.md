# Debugging Builds

This guide covers how to debug conda package builds with rattler-build when things go wrong. It's designed for both humans and AI agents working with recipes.

!!! note
    The `debug-shell` is an experimental feature and might be merged into `debug`.

## Quick Start

When a build fails:

```bash
# Open an interactive debug shell in the last failed build
rattler-build debug-shell

# You're now in the work directory with the build environment loaded
# Try running commands manually to see what's failing
bash -x conda_build.sh
```

## Inspecting and Extracting Packages

The `rattler-build package` subcommand provides utilities for inspecting and extracting built packages, which is useful for debugging package contents.

### Inspecting Packages

Use `package inspect` to view package metadata without extracting:

```bash
# Basic package information
rattler-build package inspect mypackage-1.0-h12345.conda

# Show all information including file listing
rattler-build package inspect mypackage-1.0-h12345.conda --all

# Show specific sections
rattler-build package inspect mypackage-1.0-h12345.conda --paths      # File listing with hashes
rattler-build package inspect mypackage-1.0-h12345.conda --about      # Extended about info
rattler-build package inspect mypackage-1.0-h12345.conda --run-exports # Run exports

# Output as JSON for scripting
rattler-build package inspect mypackage-1.0-h12345.conda --json
```

### Extracting Packages

Use `package extract` to extract a package to a directory for inspection:

```bash
# Extract to a directory named after the package
rattler-build package extract mypackage-1.0-h12345.conda

# Extract to a custom destination
rattler-build package extract mypackage-1.0-h12345.conda -d my-extracted

# Extract directly from a URL (supports authenticated channels)
rattler-build package extract https://conda.anaconda.org/conda-forge/linux-64/python-3.11.0-h12345.conda
```

After extraction, the command reports the SHA256/MD5 checksums and file size, which is useful for verifying package integrity.

Both `.conda` and `.tar.bz2` package formats are supported.

## Build Directory Structure

When rattler-build builds a package, it creates:

```txt
output/
└─ rattler-build-log.txt            # Append-only log of build directories (latest at bottom)
└─ bld/                             # Build directories
│   └─ rattler-build_<name>_<timestamp>/
│       └─ work/                    # Source code and working directory
│       │   └─ .source_info.json    # Source information (extracted folders, etc.)
│       │   └─ build_env.sh         # Environment setup script
│       │   └─ conda_build.sh       # The actual build script (sources `build_env.sh`)
│       │   └─ conda_build.log      # Complete build output
│       └─ host_env_placehold_.../  # Host environment (runtime dependencies)
│       └─ build_env/               # Build environment (build-time dependencies)
└─ src_cache/                       # Downloaded and extracted sources
└─ build_cache/                     # Staging cache - experimental feature
└─ <platform>/                      # Built packages
```

## The debug-shell Command

The `debug-shell` command opens an interactive shell in the build environment, automatically:

1. Finds the latest build directory from `<output-dir>/rattler-build-log.txt`
2. Sources `build_env.sh` to set up all environment variables (like `$PREFIX`, ...)
3. Sets additional environment variables for convenience

### Usage

```bash
# Use the last build (reads from output/rattler-build-log.txt)
rattler-build debug-shell
```

### Environment Variables Available in debug-shell

Inside the debug shell, you have access to:

| Variable                     | Description                                |
| ---------------------------- | ------------------------------------------ |
| `$PREFIX`                    | Host prefix (where packages get installed)  |
| `$BUILD_PREFIX`              | Build prefix (tools for building)           |
| `$SRC_DIR`                   | Source directory (same as work directory)  |
| `$RATTLER_BUILD_DIRECTORIES` | Full JSON with all directory info          |
| `$RATTLER_BUILD_RECIPE_PATH` | Path to the recipe file                     |
| `$RATTLER_BUILD_RECIPE_DIR`  | Directory containing the recipe            |
| `$RATTLER_BUILD_BUILD_DIR`   | The build directory root                   |

## Debugging Workflow

### Step 1: Build with `debug`

You can use `rattler-build debug` to setup the build environments without executing the build scripts for manual debugging.
If your recipe succeeds, but you still want to enter the debug-shell, you can use `--keep-build` to prevent cleanup:

```bash
# set up the build environment, but do not execute build script
rattler-build debug --recipe recipe.yaml
# build recipe normally, but keep build environments even if everything succeeds
rattler-build build --recipe recipe.yaml --keep-build
```

### Step 2: Enter the Debug Shell

```bash
rattler-build debug-shell
```

### Step 3: Debug Interactively

```bash
# Run the full build script with tracing
bash -x conda_build.sh

# Or run individual commands
./configure --prefix=$PREFIX
make VERBOSE=1
make install
```

### Step 4: Create a Patch for Fixes

After fixing issues in the source code:

```bash
# Create a patch from your changes
rattler-build create-patch \
  --directory . \
  --name my-fix \
  --exclude "*.o,*.so,*.pyc"

# Preview what would be included
rattler-build create-patch \
  --directory . \
  --name my-fix \
  --dry-run
```

To include new files:

```bash
rattler-build create-patch \
  --directory . \
  --name my-fix \
  --add "*.txt,src/new_file.c"
```

### Step 5: Update Recipe and Rebuild

Add the patch to your recipe:

```yaml
source:
  - url: https://example.com/source.tar.gz
    sha256: ...
    patches:
      # this needs to be manually added
      - my-fix.patch
```

Then rebuild:

```bash
rattler-build build --recipe recipe.yaml
```

## Common Debugging Scenarios

### Compilation Failures

```bash
rattler-build debug-shell

# Run with verbose output
bash -x conda_build.sh 2>&1 | less

# Or specific build commands
make VERBOSE=1
cmake --build . --verbose
```

### Missing Files or Dependencies

```bash
# Check source information
cat .source_info.json | jq .

# List what's in the work directory
find . -type f | head -30

# Check what's in the environments
ls $PREFIX/lib/
ls $BUILD_PREFIX/bin/
```

### Library Not Found Errors

```bash
# Check if the library exists in PREFIX
find $PREFIX -name "lib*.so*" -o -name "lib*.dylib*"

# Check pkg-config paths
echo $PKG_CONFIG_PATH
pkg-config --libs --cflags libfoo
```

### Build Log Analysis

All build output is saved to `conda_build.log`:

```bash
# View the full log
less conda_build.log

# Search for errors
grep -i error conda_build.log
grep -i "undefined reference" conda_build.log
```

## Understanding Relocatability

rattler-build makes packages relocatable through:

1. **RPATH patching** - Changes `.dylib` and `.so` files to use relative paths (`$ORIGIN`, `@loader_path`) using `patchelf` or `install_name_tool`

2. **Placeholder replacement** - At install time, replaces placeholder strings in binaries and text files with the actual prefix

**Important**: The placeholder is a long string (`placehold_placehol_...`). If your code has small buffer optimization or assumes static string lengths for file paths, you may need to adjust it. The `$PREFIX` length will differ at installation time. The placeholder replacement in binary files will overwrite the placeholder string, move the remainder until `\0` is found in the original string, and pad with `\0` bytes.

## Useful Commands Reference

```bash
# Build commands
rattler-build build --recipe recipe.yaml --keep-build
rattler-build build --recipe recipe.yaml --channel conda-forge --no-test

# Debug commands
rattler-build debug-shell
rattler-build debug-shell --work-dir /path/to/build_folder
rattler-build debug --recipe recipe.yaml  # Set up environment without running build

# Patch commands
rattler-build create-patch --directory . --name fix --include "*.c"
rattler-build create-patch --directory . --name fix --exclude "*.o"
rattler-build create-patch --directory . --name fix --add "*.txt" --dry-run

# Test commands
rattler-build test --package-file output/linux-64/mypackage-1.0.tar.bz2
```