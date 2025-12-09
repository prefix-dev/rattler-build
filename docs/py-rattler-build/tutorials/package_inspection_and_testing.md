# Package Inspection and Testing

This tutorial teaches you how to work with built conda packages:

1. Load packages from `.conda` or `.tar.bz2` files
2. Inspect package metadata (name, version, dependencies)
3. List all files contained in the package
4. Discover and inspect embedded tests
5. Run tests programmatically and capture results

Let's get started!

```python exec="1" source="above" session="package_inspection_and_testing"
import json
import shutil
import tempfile
from pathlib import Path

from rattler_build import Package, RenderConfig, Stage0Recipe, VariantConfig
```

## Step 1: Build a Package with Tests

First, let's build a package that has embedded tests. We'll create a simple noarch Python package with:
- A Python module
- Package content checks

<!--pytest-codeblocks:cont-->
```python exec="1" source="above" result="text" session="package_inspection_and_testing"
# Define a recipe with multiple test types
test_recipe_yaml = """
package:
  name: test-demo-package
  version: "1.0.0"

build:
  number: 0
  noarch: python
  script:
    interpreter: python
    content: |
      import os
      from pathlib import Path

      prefix = Path(os.environ["PREFIX"])

      # Create a Python module (noarch packages use site-packages directly)
      site_packages = prefix / "site-packages"
      site_packages.mkdir(parents=True, exist_ok=True)

      module_file = site_packages / "demo_module.py"
      module_file.write_text('''

      __version__ = "1.0.0"

      def greet(name: str) -> str:
          return f"Hello, {name}!"

      def add(a: int, b: int) -> int:
          return a + b
      ''')
      print(f"Created module at {module_file}")

requirements:
  run:
    - python

tests:
  # Test 1: Python import test (embedded in package)
  - python:
      imports:
        - demo_module

  # Test 2: Package contents check (runs at build time)
  - package_contents:
      files:
        - site-packages/demo_module.py
about:
  license: MIT
"""

# Parse and render the recipe
demo_recipe = Stage0Recipe.from_yaml(test_recipe_yaml)
demo_variants = VariantConfig()
demo_render = RenderConfig()
demo_results = demo_recipe.render(demo_variants, demo_render)

print("Recipe with Tests Created")
print("=" * 60)
print(f"Package: {demo_recipe.package.name}")
print(f"Version: {demo_recipe.package.version}")

# Set up build directories
recipe_tmpdir = Path(tempfile.gettempdir()) / "rattler_build_test_demo"
output_tmpdir = Path(tempfile.gettempdir()) / "rattler_build_test_demo_output"

# Clean up from previous runs
if recipe_tmpdir.exists():
    shutil.rmtree(recipe_tmpdir)
if output_tmpdir.exists():
    shutil.rmtree(output_tmpdir)

recipe_tmpdir.mkdir(parents=True)
output_tmpdir.mkdir(parents=True)

recipe_path = recipe_tmpdir / "recipe.yaml"

# Build the package (skip tests during build, we'll run them manually)
print("\nBuilding package...")
variant = demo_results[0]
from rattler_build import ToolConfiguration

tool_config = ToolConfiguration(test_strategy="skip")
build_result = variant.run_build(
    tool_config=tool_config,
    output_dir=output_tmpdir,
    recipe_path=recipe_path,
)

built_package_path = build_result.packages[0]
print(f"Built: {built_package_path}")
```

## Step 2: Loading a Package

Use `Package.from_file()` to load a built package. This reads the package metadata without extracting the entire archive.

<!--pytest-codeblocks:cont-->
```python exec="1" source="above" result="text" session="package_inspection_and_testing"
# Load the package
pkg = Package.from_file(built_package_path)

print("Package Loaded Successfully!")
print("=" * 60)
print(f"Path: {pkg.path}")
print(f"Type: {type(pkg).__name__}")
print(f"\nString representation: {repr(pkg)}")
```

## Step 3: Inspecting Package Metadata

The `Package` class provides direct access to all metadata from `index.json`:

<!--pytest-codeblocks:cont-->
```python exec="1" source="above" result="text" session="package_inspection_and_testing"
print("Package Metadata")
print("=" * 60)
print(f"Name:           {pkg.name}")
print(f"Version:        {pkg.version}")
print(f"Build string:   {pkg.build_string}")
print(f"Build number:   {pkg.build_number}")
print(f"Subdir:         {pkg.subdir}")
print(f"NoArch:         {pkg.noarch}")
print(f"License:        {pkg.license}")
print(f"Arch:           {pkg.arch}")
print(f"Platform:       {pkg.platform}")
print(f"Timestamp:      {pkg.timestamp}")

print("\nArchive Information")
print("-" * 40)
print(f"Archive type:   {pkg.archive_type}")
print(f"Filename:       {pkg.filename}")

print("\nDependencies")
print("-" * 40)
print("Runtime dependencies (depends):")
for dep in pkg.depends:
    print(f"  - {dep}")

print("\nConstraints (constrains):")
if pkg.constrains:
    for constraint in pkg.constrains:
        print(f"  - {constraint}")
else:
    print("  (none)")
```

<!--pytest-codeblocks:cont-->
```python exec="1" source="above" result="text" session="package_inspection_and_testing"
# Convert to dictionary for programmatic access
metadata_dict = pkg.to_dict()

print("Metadata as Dictionary")
print("=" * 60)

print(json.dumps(metadata_dict, indent=2, default=str))
```

## Step 4: Listing Package Contents

The `files` property lists all files contained in the package (from `paths.json`):

<!--pytest-codeblocks:cont-->
```python exec="1" source="above" result="text" session="package_inspection_and_testing"
print("Package Contents")
print("=" * 60)

files = pkg.files
print(f"Total files: {len(files)}")
print("\nAll files:")

# Group files by directory
from collections import defaultdict

dirs = defaultdict(list)
for f in files:
    parts = f.split("/")
    if len(parts) > 1:
        dirs[parts[0]].append(f)
    else:
        dirs["(root)"].append(f)

for dir_name, dir_files in sorted(dirs.items()):
    print(f"\n  {dir_name}/")
    for f in sorted(dir_files):
        print(f"    {f}")
```

## Step 5: Discovering Embedded Tests

Packages built with rattler-build can include embedded tests in `info/tests/tests.yaml`. Let's inspect them:

<!--pytest-codeblocks:cont-->
```python exec="1" source="above" result="text" session="package_inspection_and_testing"
print("Embedded Tests")
print("=" * 60)
print(f"Number of tests: {pkg.test_count()}")

pkg_tests = pkg.tests
for test in pkg_tests:
    print(f"\nTest {test.index}: {test.kind}")
    print("-" * 40)
    print(f"  Type: {type(test).__name__}")
    print(f"  Kind: {test.kind}")
    print(f"  Index: {test.index}")
    print(f"  Repr: {repr(test)}")
```

## Step 6: Inspecting Specific Test Types

Each test type has specific properties. Use the `as_*_test()` methods to get type-specific details:

<!--pytest-codeblocks:cont-->
```python exec="1" source="above" result="text" session="package_inspection_and_testing"
print("Test Type Details")
print("=" * 60)

for test in pkg_tests:
    print(f"\nTest {test.index}: {test.kind}")
    print("-" * 40)

    if test.kind == "python":
        py_test = test.as_python_test()
        print("  Python Test:")
        print(f"    Imports: {py_test.imports}")
        print(f"    Pip check: {py_test.pip_check}")
        if py_test.python_version:
            pv = py_test.python_version
            if pv.is_none():
                print("    Python version: any")
            elif pv.as_single():
                print(f"    Python version: {pv.as_single()}")
            elif pv.as_multiple():
                print(f"    Python versions: {pv.as_multiple()}")

    elif test.kind == "commands":
        cmd_test = test.as_commands_test()
        print("  Commands Test:")
        print(f"    Script: {cmd_test.script}")
        print(f"    Run requirements: {cmd_test.requirements_run}")
        print(f"    Build requirements: {cmd_test.requirements_build}")

    elif test.kind == "package_contents":
        pc_test = test.as_package_contents_test()
        print("  Package Contents Test:")
        print(f"    Strict mode: {pc_test.strict}")

        sections = [
            ("files", pc_test.files),
            ("site_packages", pc_test.site_packages),
            ("bin", pc_test.bin),
            ("lib", pc_test.lib),
            ("include", pc_test.include),
        ]

        for name, checks in sections:
            if checks.exists or checks.not_exists:
                print(f"    {name}:")
                if checks.exists:
                    print(f"      exists: {checks.exists}")
                if checks.not_exists:
                    print(f"      not_exists: {checks.not_exists}")
```

## Step 7: Running Tests

Now let's run the tests! You can run individual tests by index or all tests at once:

<!--pytest-codeblocks:cont-->
```python exec="1" source="above" result="text" session="package_inspection_and_testing"
print("Running Individual Tests")
print("=" * 60)

for i in range(pkg.test_count()):
    print(f"\nRunning test {i}...")
    result = pkg.run_test(i)

    status = "PASS" if result.success else "FAIL"
    print(f"   {status}")
    print(f"   Test index: {result.test_index}")

    if result.output:
        print(f"   Output ({len(result.output)} lines):")
        for line in result.output[:5]:  # Show first 5 lines
            print(f"     {line}")
        if len(result.output) > 5:
            print(f"     ... and {len(result.output) - 5} more lines")
```

<!--pytest-codeblocks:cont-->
```python exec="1" source="above" result="text" session="package_inspection_and_testing"
print("Running All Tests at Once")
print("=" * 60)

all_results = pkg.run_tests()

print(f"\nTotal tests: {len(all_results)}")
passed = sum(1 for r in all_results if r.success)
failed = len(all_results) - passed

print(f"Passed: {passed}")
print(f"Failed: {failed}")

print("\nResults summary:")
for result in all_results:
    status = "PASS" if result.success else "FAIL"
    print(f"  {status} Test {result.test_index}")

    # TestResult can be used as a boolean
    if result:
        print("     (result is truthy)")
    else:
        print("     (result is falsy)")
```

## Step 8: Using Test Results

The `TestResult` object provides:
- `success`: Boolean indicating pass/fail
- `test_index`: Which test was run
- `output`: List of output/log lines
- Can be used directly as a boolean in conditions

<!--pytest-codeblocks:cont-->
```python exec="1" source="above" result="text" session="package_inspection_and_testing"
print("TestResult Properties")
print("=" * 60)

for result in all_results:
    print(f"\nTest {result.test_index}:")
    print(f"  success:    {result.success}")
    print(f"  test_index: {result.test_index}")
    print(f"  output:     {len(result.output)} lines")
    print(f"  bool():     {bool(result)}")
    print(f"  repr():     {repr(result)}")

# Example: Filter results
print("\n" + "=" * 60)
passed_tests = [r for r in all_results if r]
failed_tests = [r for r in all_results if not r]

print(f"Passed tests: {[r.test_index for r in passed_tests]}")
print(f"Failed tests: {[r.test_index for r in failed_tests]}")
```

## Step 9: Running Tests with Custom Configuration

You can customize test execution with channels, authentication, and other options:

<!--pytest-codeblocks:cont-->
```python exec="1" source="above" result="text" session="package_inspection_and_testing"
print("Test Configuration Options")
print("=" * 60)

print(
    """
Available options for run_test() and run_tests():

- channel: List[str]           # Channels to use for dependencies
                               # e.g., ["conda-forge", "defaults"]

- channel_priority: str        # "disabled", "strict", or "flexible"

- debug: bool                  # Keep test environment for debugging
                               # Default: False

- auth_file: str | Path        # Path to authentication file

- allow_insecure_host: List[str]  # Hosts to allow insecure connections

- compression_threads: int     # Number of compression threads

- use_bz2: bool               # Enable bz2 repodata (default: True)
- use_zstd: bool              # Enable zstd repodata (default: True)
- use_jlap: bool              # Enable JLAP incremental repodata
- use_sharded: bool           # Enable sharded repodata (default: True)
"""
)

# Example with custom channel
print("\nExample: Running test with conda-forge channel:")
result = pkg.run_test(
    0,
    channel=["conda-forge"],
    channel_priority="strict",
)
print(f"  Result: {'PASS' if result.success else 'FAIL'}")
```

## Summary

In this tutorial, you learned how to:

- **Load packages**: Use `Package.from_file()` to load `.conda` or `.tar.bz2` files
- **Inspect metadata**: Access `name`, `version`, `depends`, `license`, etc.
- **Archive information**: Use `archive_type` and `filename` to get package format details
- **List contents**: Use `files` to see all files in the package
- **Discover tests**: Access `tests` to see embedded test definitions
- **Inspect test types**: Use `as_python_test()`, `as_commands_test()`, etc.
- **Run tests**: Use `run_test(index)` or `run_tests()` to execute tests
- **Handle results**: `TestResult` provides `success`, `output`, and works as boolean

The Package API provides a complete interface for inspecting and testing conda packages programmatically!
