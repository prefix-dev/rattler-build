# Bumping recipe versions

When maintaining conda recipes, you often need to update packages to newer versions. This involves changing the version number in your recipe and updating the SHA256 checksum for the new source archive. `rattler-build` provides the `bump-recipe` command to automate this process.

## How it works

The `bump-recipe` command:

1. Reads the recipe file and extracts the current version and source URL(s)
2. Detects the version provider (GitHub, PyPI, crates.io) from the source URL
3. Either uses a specified version or auto-detects the latest version from the provider
4. Downloads the new source archive and computes its SHA256 checksum
5. Updates the recipe file using simple string replacement (preserving formatting and comments)

## Basic usage

Auto-detect and bump to the latest version:

```bash
rattler-build bump-recipe --recipe recipe.yaml
```

Bump to a specific version:

```bash
rattler-build bump-recipe --recipe recipe.yaml --version 2.0.0
```

Check if updates are available without modifying the recipe:

```bash
rattler-build bump-recipe --recipe recipe.yaml --check-only
```

Preview changes without writing to the file:

```bash
rattler-build bump-recipe --recipe recipe.yaml --dry-run
```

## Command options

| Option | Description |
|--------|-------------|
| `-r, --recipe <PATH>` | Path to the recipe file (default: current directory) |
| `--version <VERSION>` | Specific version to bump to (auto-detects if not specified) |
| `--check-only` | Only check for updates, don't modify the recipe |
| `--dry-run` | Show what would change without writing to the file |
| `--include-prerelease` | Include pre-release versions (alpha, beta, rc) when auto-detecting |
| `--keep-build-number` | Keep the current build number instead of resetting to 0 |

## Supported providers

The command auto-detects the version provider from your source URL:

| Provider | URL patterns | API used |
|----------|--------------|----------|
| **GitHub** | `github.com/owner/repo/archive/...`<br>`github.com/owner/repo/releases/download/...`<br>`api.github.com/repos/owner/repo/tarball/...` | GitHub Releases API (falls back to Tags) |
| **PyPI** | `pypi.io/packages/source/...`<br>`files.pythonhosted.org/packages/source/...` | PyPI JSON API |
| **crates.io** | `crates.io/api/v1/crates/...`<br>`static.crates.io/crates/...` | crates.io API |

For URLs that don't match any known provider, you must specify the version manually with `--version`.

## Recipe format

The command works with recipes that use Jinja2 templating in the context section:

```yaml title="recipe.yaml"
context:
  name: mypackage
  version: "1.0.0"

package:
  name: ${{ name }}
  version: ${{ version }}

source:
  url: https://github.com/owner/${{ name }}/archive/v${{ version }}.tar.gz
  sha256: abc123...

build:
  number: 5
```

After running `rattler-build bump-recipe --recipe recipe.yaml --version 2.0.0`:

```yaml title="recipe.yaml (updated)"
context:
  name: mypackage
  version: "2.0.0"

package:
  name: ${{ name }}
  version: ${{ version }}

source:
  url: https://github.com/owner/${{ name }}/archive/v${{ version }}.tar.gz
  sha256: def456...  # automatically updated

build:
  number: 0  # Note: build number was reset back to `0`
```

## Derived context variables

The command supports context variables that depend on other variables:

```yaml title="recipe.yaml"
context:
  name: mypackage
  version: "1.0.0"
  version_underscore: ${{ version | replace('.', '_') }}

source:
  url: https://example.com/${{ name }}-${{ version_underscore }}.tar.gz
  sha256: ...
```

When bumping to version `2.0.0`, the URL will correctly resolve to `https://example.com/mypackage-2_0_0.tar.gz` because the Jinja expressions are properly evaluated with the new version.

## Build number reset

When bumping the version, the command automatically resets the build number to 0. It detects the build number in these locations:

- `context.number` or `context.build_number`
- `build.number`

```yaml title="Before bump"
context:
  version: "1.0.0"
  build_number: 5

build:
  number: ${{ build_number }}
```

```yaml title="After bump"
context:
  version: "2.0.0"
  build_number: 0

build:
  number: ${{ build_number }}
```

## Limitations

- The version must be defined as a literal value in the `context` section (not as a Jinja expression)
- For generic URLs without a known provider, you must specify `--version` manually
