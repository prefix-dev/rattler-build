# Debugging Builds

This guide covers how to debug conda package builds with rattler-build when
things go wrong. It's designed for both humans and AI agents working with
recipes.

## Debugging Workflow

Suppose you have a recipe that fails to build:

```yaml title="recipe.yaml"
package:
  name: test
  version: "1.0"

build:
  script:
    - exit 1
```

Running `rattler-build build` will fail. When a build
fails, the build directory is automatically preserved so you can investigate.
There are multiple things you can do to investigate

### Enter the Debug Shell

Jump straight into the failed build environment:

```bash
rattler-build debug shell
```

This opens an interactive shell in the work directory with the build environment
loaded. All environment variables (`$PREFIX`, `$BUILD_PREFIX`, etc.) are set up
exactly as they were during the build.

Now you can modify files and run individual commands to isolate the issue:

```bash
./configure --prefix=$PREFIX
make VERBOSE=1
make install
```

### Re-run the Build Script

Use `debug run` to re-execute the build script with the full environment already
loaded:

```bash
# Re-run the build script
rattler-build debug run

# Re-run with shell tracing (bash -x) for verbose output
rattler-build debug run --trace
```

You can find the working directory by running the following:

```
rattler-build debug workdir
```

Modify files inside that directory and run `rattler-build debug run` to check whether that fixed the problem.

### Modify Dependencies

If you need additional packages in the host or build environment, you can add
them without re-running the full setup:

```bash
# Add packages to the host environment
rattler-build debug host-add libfoo libbar

# Add build tools
rattler-build debug build-add gdb valgrind
```

Remember to add them to your recipe.yaml once you found the right set of dependencies.


### Create a Patch for Fixes

After fixing issues in the source code:

```bash
# Create a patch from your changes
rattler-build debug create-patch \
  --directory . \
  --name my-fix \
  --exclude "*.o,*.so,*.pyc"

# Preview what would be included
rattler-build debug create-patch \
  --directory . \
  --name my-fix \
  --dry-run
```

To include new files:

```bash
rattler-build debug create-patch \
  --directory . \
  --name my-fix \
  --add "*.txt,src/new_file.c"
```

In the end, you will have a patch file that you can include in your recipe


### Update Recipe and Rebuild

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

### Debugging a Successful Build

If your recipe builds *successfully* but you still want to inspect the
environment, use `--keep-build` to prevent cleanup:

```bash
rattler-build build --recipe recipe.yaml --keep-build
rattler-build debug shell
```


### Setting Up a Debug Environment Without Building

If you want to prepare a debug environment without running the build script at
all, use `debug setup`. This resolves dependencies, downloads sources, and
creates the build script — but doesn't execute it:

```bash
rattler-build debug setup --recipe recipe.yaml
rattler-build debug shell
```

This is useful when you want to inspect or modify sources before running the
build for the first time.

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

## Environment Variables Available in the Debug Shell

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

## Common Debugging Scenarios

### Compilation Failures

```bash
# Re-run the build script with tracing to see where it fails
rattler-build debug run --trace

# Or enter the shell and run specific build commands
rattler-build debug shell
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

## Debugging with AI Agents

The `debug` subcommands are designed to work well with AI coding agents (Claude
Code, Codex, etc.) that cannot use interactive shells. The key principle is:
**set up once, then iterate fast by re-running the build script**.

### Agent Workflow

```bash
# 1. Set up the debug environment (slow, only once)
rattler-build debug setup --recipe recipe.yaml

# 2. Get the work directory
rattler-build debug workdir

# 3. Edit source files in work directory to fix the issue
#    (agent edits files directly)

# 4. Re-run the build script (fast — no dependency resolution)
rattler-build debug run

# 5. If the build fails, go back to step 3
# 6. Once it works, create a patch
rattler-build debug create-patch --name my-fix
```

### Key Points for Agents

- **`debug setup`** is non-interactive — it sets up everything and exits. Use
  this instead of `debug shell` which opens an interactive shell.
- **`debug workdir`** prints the work directory path to stdout — no `jq` or log
  parsing needed.
- **`debug run`** re-runs the build script with the environment already set up.
  Use `--trace` for verbose `bash -x` output. This is the fast inner loop — it
  takes seconds, not minutes, because dependencies are already installed.
- **`debug host-add` / `debug build-add`** let agents add missing dependencies
  without re-running the full setup.
- **`debug create-patch`** generates a unified diff from changes in the work
  directory. The agent can then add the patch to the recipe.

### Parsing the Build Log

The last line of `output/rattler-build-log.txt` is JSON:

```json
{
  "work_dir": "/path/to/output/bld/rattler-build_pkg_1234/work",
  "build_dir": "/path/to/output/bld/rattler-build_pkg_1234",
  "host_prefix": "/path/to/output/bld/rattler-build_pkg_1234/host_env_placehold_...",
  "build_prefix": "/path/to/output/bld/rattler-build_pkg_1234/build_env",
  "recipe_dir": "/path/to/recipe/dir",
  "recipe_path": "/path/to/recipe.yaml",
  "output_dir": "/path/to/output"
}
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
rattler-build debug setup --recipe recipe.yaml         # Set up environment
rattler-build debug shell                              # Open shell in last build
rattler-build debug shell --work-dir /path/to/work     # Open shell in specific build
rattler-build debug workdir                            # Print work directory path
rattler-build debug run                                # Re-run build script
rattler-build debug run --trace                        # Re-run with bash -x tracing
rattler-build debug host-add python numpy              # Add packages to host env
rattler-build debug build-add cmake                    # Add packages to build env
rattler-build debug create-patch --name fix            # Create patch from changes
rattler-build debug create-patch --name fix --dry-run  # Preview patch

# Test commands
rattler-build test --package-file output/linux-64/mypackage-1.0.tar.bz2
```
