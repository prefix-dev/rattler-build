# Debugging Builds

This guide covers how to debug conda package builds with rattler-build when things go wrong. It's designed for both humans and AI agents working with recipes.

## Quick Start

When a build fails:

```bash
# Set up the environment and open an interactive debug shell
rattler-build debug --recipe recipe.yaml

# You're now in the work directory with the build environment loaded
# Try running commands manually to see what's failing
bash -x conda_build.sh
```

If you've already run a build with `--keep-build`, you can open a debug shell directly:

```bash
rattler-build debug shell
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

## The `debug` Command

The `debug` command sets up the build environment from a recipe and drops you into an interactive debug shell:

1. Resolves and installs build/host dependencies
2. Downloads and extracts sources, applies patches
3. Creates the build script (`conda_build.sh` / `conda_build.bat`)
4. Opens an interactive shell in the work directory with `build_env.sh` sourced

### Usage

```bash
# Set up environment from a recipe and enter the debug shell
rattler-build debug --recipe recipe.yaml

# Set up environment only (no shell)
rattler-build debug --recipe recipe.yaml --no-shell
```

### Debug Subcommands

The `debug` command also provides subcommands for working with existing debug environments:

```bash
# Open a debug shell in the last build (reads from output/rattler-build-log.txt)
rattler-build debug shell

# Open a shell in a specific build directory
rattler-build debug shell --work-dir /path/to/build_folder

# Add packages to the host environment
rattler-build debug host-add numpy pandas

# Add packages to the build environment
rattler-build debug build-add cmake ninja
```

### Environment Variables Available in the Debug Shell

Inside the debug shell, you have access to:

| Variable                       | Description                                |
| ------------------------------ | ------------------------------------------ |
| `$PREFIX`                      | Host prefix (where packages get installed) |
| `$BUILD_PREFIX`                | Build prefix (tools for building)          |
| `$SRC_DIR`                     | Source directory (same as work directory)   |
| `$RATTLER_BUILD_DIRECTORIES`   | Full JSON with all directory info           |
| `$RATTLER_BUILD_RECIPE_PATH`   | Path to the recipe file                    |
| `$RATTLER_BUILD_RECIPE_DIR`    | Directory containing the recipe            |
| `$RATTLER_BUILD_BUILD_DIR`     | The build directory root                   |
| `$RATTLER_BUILD_HOST_PREFIX`   | Path to the host prefix                    |
| `$RATTLER_BUILD_BUILD_PREFIX`  | Path to the build prefix                   |

## Debugging Workflow

### Step 1: Enter the Debug Environment

Use `rattler-build debug` to set up build environments and enter a debug shell in one step:

```bash
# set up the build environment and enter the debug shell
rattler-build debug --recipe recipe.yaml
```

If your recipe succeeds but you still want to debug, you can use `--keep-build` to prevent cleanup and then open a debug shell:

```bash
# build recipe normally, but keep build environments even if everything succeeds
rattler-build build --recipe recipe.yaml --keep-build

# open a debug shell in the last build
rattler-build debug shell
```

### Step 2: Debug Interactively

```bash
# Run the full build script with tracing
bash -x conda_build.sh

# Or run individual commands
./configure --prefix=$PREFIX
make VERBOSE=1
make install
```

### Step 3: Modify Dependencies (Optional)

If you need additional packages in the host or build environment, you can add them without re-running the full setup:

```bash
# Add packages to the host environment
rattler-build debug host-add libfoo libbar

# Add build tools
rattler-build debug build-add gdb valgrind
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
rattler-build debug shell

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

# Add a missing dependency on the fly
rattler-build debug host-add libmissing
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
rattler-build debug --recipe recipe.yaml            # Set up env + enter shell
rattler-build debug --recipe recipe.yaml --no-shell  # Set up env only
rattler-build debug shell                            # Open shell in last build
rattler-build debug shell --work-dir /path/to/work   # Open shell in specific build
rattler-build debug host-add python numpy            # Add packages to host env
rattler-build debug build-add cmake                  # Add packages to build env

# Patch commands
rattler-build create-patch --directory . --name fix --include "*.c"
rattler-build create-patch --directory . --name fix --exclude "*.o"
rattler-build create-patch --directory . --name fix --add "*.txt" --dry-run

# Test commands
rattler-build test --package-file output/linux-64/mypackage-1.0.tar.bz2
```
