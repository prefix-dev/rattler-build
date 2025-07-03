# Creating patches

When packaging software, you often need to make small source code changes – fixing a typo, applying a bug fix, or adapting build scripts. Instead of maintaining a fork of the upstream project, you can create patch files that are applied during the build process.

`rattler-build` provides a streamlined workflow for creating patches using the `debug` and `create-patch` commands.

## How it works

The `debug` command sets up a build environment and downloads sources without running the actual build script. This gives you a clean workspace to make changes. The `create-patch` command then compares your modified files against the original sources and generates a unified diff patch.

## Basic workflow

```bash
# Set up debug environment (downloads sources, no build)
rattler-build debug --recipe recipe.yaml

# Edit files in the work directory
cd output/bld/rattler-build_<package>_*/work
vim some_file.c

# Generate patch
rattler-build create-patch --directory . --name fix-typo

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

- `--directory <DIR>` - Work directory containing the modified sources (required)
- `--name <NAME>` - Patch filename without .patch extension (default: "changes")
- `--patch-dir <DIR>` - Directory to write the patch file (default: recipe directory)
- `--exclude <PATTERNS>` - Files to exclude from the patch (comma-separated glob patterns)
- `--dry-run` - Preview changes without creating a file

## Examples

Generate a patch with a custom name:

```bash
rattler-build create-patch --directory work/ --name fix-build-system
```

Preview changes before creating the patch:

```bash
rattler-build create-patch --directory work/ --dry-run
```

Create a patch in a dedicated patches folder:

```bash
rattler-build create-patch --directory work/ \
                           --name fix-compilation \
                           --patch-dir patches/
```

## Supported source types

Currently, the `create-patch` command supports:

- **URL sources** - Creates patches for extracted archives (tar.gz, zip, etc.)
- **Git sources** - ⚠️ Not yet implemented
- **Path sources** - ⚠️ Not yet implemented
