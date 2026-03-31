# Creating patches

When packaging software, you often need to make small source code changes – fixing a typo, applying a bug fix, or adapting build scripts. Instead of maintaining a fork of the upstream project, you can create patch files that are applied during the build process.

Rattler-Build provides a streamlined workflow for creating patches using the `debug` and `create-patch` commands.

## How it works

The `debug` command sets up a build environment and downloads sources without running the actual build script. This gives you a clean workspace to make changes. The `create-patch` command then compares your modified files against the original sources and generates a unified diff patch.

## Basic workflow

```bash
# Set up debug environment and enter the debug shell
rattler-build debug setup --recipe recipe.yaml
rattler-build debug shell

# You're now in the work directory with the build environment sourced.
# Edit files directly:
vim some_file.c

# Generate patch (from inside the debug shell, the directories are auto-detected)
rattler-build debug create-patch --name fix-typo

# Add to recipe
```

```yaml title="recipe.yaml"
source:
  - url: https://example.com/package.tar.gz
    sha256: abc123...
    patches:
      - fix-typo.patch
```

## Command options

The `create-patch` command supports the following options:

- `--directory <DIR>` - Directory where we want to create the patch (default: current directory)
- `--name <NAME>` - Patch filename without .patch extension (default: "changes")
- `--overwrite` - Whether to overwrite the patch file if it already exists
- `--patch-dir <DIR>` - Directory to write the patch file (default: recipe directory)
- `--exclude <PATTERNS>` - Files to exclude from the patch (comma-separated glob patterns)
- `--add <ADD>` - Include new files matching these glob patterns (e.g., "*.txt", "src/**/*.rs")
- `--include <INCLUDE>` - Only include modified files matching these glob patterns (e.g., "*.c", "src/**/*.rs") (default: all modified files are included)
- `--dry-run` - Preview changes without creating a file

## Examples

Generate a patch with a custom name:

```bash
rattler-build debug create-patch --directory work/ --name fix-build-system
```

Preview changes before creating the patch:

```bash
rattler-build debug create-patch --directory work/ --dry-run
```

Create a patch in a dedicated patches folder:

```bash
rattler-build debug create-patch --directory work/ \
                                 --name fix-compilation \
                                 --patch-dir patches/
```

## Supported source types

Currently, the `create-patch` command supports:

- **URL sources** - Creates patches for extracted archives (tar.gz, zip, etc.)
- **Git sources** - ⚠️ Not yet implemented
- **Path sources** - ⚠️ Not yet implemented
