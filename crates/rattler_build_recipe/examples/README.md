# Recipe Parser Examples

This directory contains examples demonstrating the `rattler_build_recipe` crate functionality.

## Running Examples

### Easy Method (Recommended)

The repository includes cargo aliases that automatically enable all required features:

**From the crate directory** (`crates/rattler_build_recipe`):
```bash
cargo example parse_recipe -- recipe.yaml
```

**From the workspace root**:
```bash
cargo example-recipe parse_recipe -- recipe.yaml
```

### Manual Method

If the cargo alias doesn't work or you prefer manual control:

```bash
# With all features (recommended)
cargo run --example parse_recipe --all-features -- <recipe.yaml> [OPTIONS]

# With specific features
cargo run --example parse_recipe --features "miette,variant-config" -- <recipe.yaml> [OPTIONS]
```

**Note:** Examples require the `miette` and `variant-config` features to be enabled.

## Parse Recipe Example

The `parse_recipe` binary demonstrates parsing and evaluating recipe YAML files.

### Usage

```bash
cargo example parse_recipe -- <recipe.yaml> [OPTIONS]
```

### Command-line Options

- `-v, --variants <FILE>`: Path to variant configuration file (e.g., `variants.yaml`)
- `-D, --define <KEY=VALUE>`: Define context variables (can be used multiple times)

### Examples

**Basic usage (no variables):**
```bash
cargo example parse_recipe -- examples/simple_recipe.yaml
```

**With variable substitution:**
```bash
cargo example parse_recipe -- examples/simple_recipe.yaml \
  -Dname=mypackage \
  -Dversion=1.2.3 \
  -Dorg=myorg \
  -Dmaintainer="Bob Smith" \
  -Dunix=true \
  -Dpython_version=3.11 \
  -Dadjective=cool
```

**With variant configuration (build matrices):**
```bash
cargo example parse_recipe -- recipe.yaml --variants variants.yaml
```

**Combining variants with extra context:**
```bash
cargo example parse_recipe -- recipe.yaml --variants variants.yaml -Dunix=true
```

**With conditionals (unix vs windows):**
```bash
# Unix build
cargo example parse_recipe -- examples/simple_recipe.yaml \
  -Dname=foo -Dversion=1.0.0 -Dunix=true -Dpython_version=3.11

# Windows build
cargo example parse_recipe -- examples/simple_recipe.yaml \
  -Dname=foo -Dversion=1.0.0 -Dpython_version=3.11
```

### What it shows

The binary demonstrates the full parsing and evaluation pipeline:

1. **Stage 0 (Parsing)**: Shows the recipe with templates (`${{ }}`) and conditionals (`if/then/else`)
2. **Variable Detection**: Lists all variables used in the recipe
3. **Evaluation Context**: Shows the provided variable values
4. **Stage 1 (Evaluation)**: Shows the evaluated recipe with:
   - Templates rendered to concrete values
   - Conditionals flattened based on context
   - Types validated (PackageName, Version, Url, SPDX License)

### Output Format

The binary outputs:
- **JSON** for Stage0 recipe (with templates/conditionals)
- **Summary** of evaluated Stage1 recipe (package, dependencies, etc.)
- **Debug format** showing complete Stage1 structure with validated types

### Sample Recipes

- **`simple_recipe.yaml`**: Basic recipe with templates and conditionals
- **`compiler_recipe.yaml`**: Recipe demonstrating Jinja function calls like `compiler('c')`
- **`build_recipe.yaml`**: Recipe with basic build configuration (number, string, script, noarch)
- **`advanced_build_recipe.yaml`**: Recipe with advanced build options (file management, dynamic linking)
- **`test_recipe.yaml`**: Advanced recipe with various features

### Supported Build Options

The `build` section supports the following fields:

**Basic Options:**
- `number`: Build number (increments with each rebuild)
- `string`: Build string override (usually auto-generated)
- `script`: Build commands or script file path
- `noarch`: Platform-independent package type (`python` or `generic`)
- `skip`: Condition to skip building

**Python Options:**
- `python.entry_points`: Python console script entry points
- `python.skip_pyc_compilation`: Skip pyc compilation for specific files (glob patterns)
- `python.use_python_app_entrypoint`: Use Python.app on macOS
- `python.version_independent`: Mark package as Python version independent (abi3)
- `python.site_packages_path`: Site-packages path for the python package itself

**File Management (with validated glob patterns):**
- `always_copy_files`: Files to always copy (glob patterns, validated)
- `always_include_files`: Files to always include (glob patterns, validated)
- `files`: Files to package (glob patterns, validated)
- `merge_build_and_host_envs`: Merge build and host environments (boolean)

**Dynamic Linking (Linux/macOS):**
- `dynamic_linking.rpaths`: Runtime library search paths
- `dynamic_linking.binary_relocation`: Binary relocation (true/false or glob patterns)
- `dynamic_linking.missing_dso_allowlist`: Allowed missing shared libraries (glob patterns, validated)
- `dynamic_linking.rpath_allowlist`: Allowed rpath locations (glob patterns, validated)
- `dynamic_linking.overdepending_behavior`: What to do on overdepending (`ignore` or `error`)
- `dynamic_linking.overlinking_behavior`: What to do on overlinking (`ignore` or `error`)

**Variant Configuration:**
- `variant.use_keys`: Variant keys to use
- `variant.ignore_keys`: Variant keys to ignore
- `variant.down_prioritize_variant`: Down-prioritize variant (negative integer)

**Prefix Detection:**
- `prefix_detection.force_file_type.text`: Force files to be treated as text (glob patterns)
- `prefix_detection.force_file_type.binary`: Force files to be treated as binary (glob patterns)
- `prefix_detection.ignore`: Files to ignore for prefix replacement (true/false or glob patterns)
- `prefix_detection.ignore_binary_files`: Ignore binary files for prefix replacement (boolean, Unix only)

**Post-Processing:**
- `post_process`: List of regex-based replacements
  - `files`: Files to process (glob patterns, validated)
  - `regex`: Regular expression pattern to match
  - `replacement`: Replacement string

### Supported Jinja Functions

The evaluation system supports the following Jinja functions:

- **`compiler(language)`**: Returns the appropriate compiler package for the language (e.g., `compiler('c')` → `gcc_linux-64`)
- **`cdt(package_name)`**: Returns the Core Dependency Tree package for Linux
- **`match(value, spec)`**: Tests if a version matches a version specification
- **`is_linux(platform)`**, **`is_osx(platform)`**, **`is_windows(platform)`**, **`is_unix(platform)`**: Platform checking functions

### Notes

- Missing variables will be reported (known function names are excluded from this warning)
- Invalid values (bad URLs, SPDX expressions, package names) will show clear error messages
- Conditionals evaluate based on variable existence (present = true, absent = false)
- Jinja functions are configured with platform defaults (based on the current system)
