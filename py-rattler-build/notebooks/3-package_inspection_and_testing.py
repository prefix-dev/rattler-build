"""
Educational notebook: Package Inspection and Testing

This notebook explores the Package inspection and testing API:
- Loading and inspecting built conda packages
- Examining package metadata from index.json
- Listing package contents from paths.json
- Discovering embedded tests from tests.yaml
- Running tests individually or all at once
- Understanding different test types
"""

import marimo

__generated_with = "0.18.1"
app = marimo.App(width="medium")


@app.cell(hide_code=True)
def _(mo):
    mo.md(r"""
    # 🔍 Package Inspection and Testing

    This notebook teaches you how to work with built conda packages:

    1. Load packages from `.conda` or `.tar.bz2` files
    2. Inspect package metadata (name, version, dependencies)
    3. List all files contained in the package
    4. Discover and inspect embedded tests
    5. Run tests programmatically and capture results

    Let's get started!
    """)
    return


@app.cell
def _():
    import json
    import shutil
    import tempfile
    from pathlib import Path

    import marimo as mo

    from rattler_build import Package
    from rattler_build.render import RenderConfig
    from rattler_build.stage0 import Recipe
    from rattler_build.variant_config import VariantConfig
    return (
        Package,
        Path,
        Recipe,
        RenderConfig,
        VariantConfig,
        json,
        mo,
        shutil,
        tempfile,
    )


@app.cell(hide_code=True)
def _(mo):
    mo.md(r"""
    ## Step 1: Build a Package with Tests

    First, let's build a package that has embedded tests. We'll create a simple noarch Python package with:
    - A Python module
    - Package content checks
    """)
    return


@app.cell
def _(Path, Recipe, RenderConfig, VariantConfig, shutil, tempfile):
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
    demo_recipe = Recipe.from_yaml(test_recipe_yaml)
    demo_variants = VariantConfig()
    demo_render = RenderConfig()
    demo_results = demo_recipe.render(demo_variants, demo_render)

    print("📦 Recipe with Tests Created")
    print("=" * 60)
    print(f"Package: {demo_recipe.package.name}")
    print(f"Version: {demo_recipe.package.version}")

    # Set up build directories
    _recipe_tmpdir = Path(tempfile.gettempdir()) / "rattler_build_test_demo"
    _output_tmpdir = Path(tempfile.gettempdir()) / "rattler_build_test_demo_output"

    # Clean up from previous runs
    if _recipe_tmpdir.exists():
        shutil.rmtree(_recipe_tmpdir)
    if _output_tmpdir.exists():
        shutil.rmtree(_output_tmpdir)

    _recipe_tmpdir.mkdir(parents=True)
    _output_tmpdir.mkdir(parents=True)

    _recipe_path = _recipe_tmpdir / "recipe.yaml"

    # Build the package (skip tests during build, we'll run them manually)
    print("\n🔨 Building package...")
    _variant = demo_results[0]
    from rattler_build import ToolConfiguration

    _tool_config = ToolConfiguration(test_strategy="skip")
    _build_result = _variant.run_build(
        tool_config=_tool_config,
        output_dir=_output_tmpdir,
        recipe_path=_recipe_path,
    )

    built_package_path = _build_result.packages[0]
    print(f"✅ Built: {built_package_path}")
    return (built_package_path,)


@app.cell(hide_code=True)
def _(mo):
    mo.md(r"""
    ## Step 2: Loading a Package

    Use `Package.from_file()` to load a built package. This reads the package metadata without extracting the entire archive.
    """)
    return


@app.cell
def _(Package, built_package_path):
    # Load the package
    pkg = Package.from_file(built_package_path)

    print("📦 Package Loaded Successfully!")
    print("=" * 60)
    print(f"Path: {pkg.path}")
    print(f"Type: {type(pkg).__name__}")
    print(f"\nString representation: {repr(pkg)}")
    return (pkg,)


@app.cell(hide_code=True)
def _(mo):
    mo.md(r"""
    ## Step 3: Inspecting Package Metadata

    The `Package` class provides direct access to all metadata from `index.json`:
    """)
    return


@app.cell
def _(pkg):
    print("📋 Package Metadata")
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

    print("\n📦 Archive Information")
    print("-" * 40)
    print(f"Archive type:   {pkg.archive_type}")
    print(f"Filename:       {pkg.filename}")

    print("\n📦 Dependencies")
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
    return


@app.cell
def _(json, pkg):
    # Convert to dictionary for programmatic access
    metadata_dict = pkg.to_dict()

    print("📊 Metadata as Dictionary")
    print("=" * 60)

    print(json.dumps(metadata_dict, indent=2, default=str))
    return


@app.cell(hide_code=True)
def _(mo):
    mo.md(r"""
    ## Step 4: Listing Package Contents

    The `files` property lists all files contained in the package (from `paths.json`):
    """)
    return


@app.cell
def _(pkg):
    print("📁 Package Contents")
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
    return


@app.cell(hide_code=True)
def _(mo):
    mo.md(r"""
    ## Step 5: Discovering Embedded Tests

    Packages built with rattler-build can include embedded tests in `info/tests/tests.yaml`. Let's inspect them:
    """)
    return


@app.cell
def _(pkg):
    print("🧪 Embedded Tests")
    print("=" * 60)
    print(f"Number of tests: {pkg.test_count()}")

    pkg_tests = pkg.tests
    for _test in pkg_tests:
        print(f"\n📋 Test {_test.index}: {_test.kind}")
        print("-" * 40)
        print(f"  Type: {type(_test).__name__}")
        print(f"  Kind: {_test.kind}")
        print(f"  Index: {_test.index}")
        print(f"  Repr: {repr(_test)}")
    return (pkg_tests,)


@app.cell(hide_code=True)
def _(mo):
    mo.md(r"""
    ## Step 6: Inspecting Specific Test Types

    Each test type has specific properties. Use the `as_*_test()` methods to get type-specific details:
    """)
    return


@app.cell
def _(pkg_tests):
    print("🔬 Test Type Details")
    print("=" * 60)

    for _test in pkg_tests:
        print(f"\n📋 Test {_test.index}: {_test.kind}")
        print("-" * 40)

        if _test.kind == "python":
            _py_test = _test.as_python_test()
            print("  Python Test:")
            print(f"    Imports: {_py_test.imports}")
            print(f"    Pip check: {_py_test.pip_check}")
            if _py_test.python_version:
                _pv = _py_test.python_version
                if _pv.is_none():
                    print("    Python version: any")
                elif _pv.as_single():
                    print(f"    Python version: {_pv.as_single()}")
                elif _pv.as_multiple():
                    print(f"    Python versions: {_pv.as_multiple()}")

        elif _test.kind == "commands":
            _cmd_test = _test.as_commands_test()
            print("  Commands Test:")
            print(f"    Script: {_cmd_test.script}")
            print(f"    Run requirements: {_cmd_test.requirements_run}")
            print(f"    Build requirements: {_cmd_test.requirements_build}")

        elif _test.kind == "package_contents":
            _pc_test = _test.as_package_contents_test()
            print("  Package Contents Test:")
            print(f"    Strict mode: {_pc_test.strict}")

            _sections = [
                ("files", _pc_test.files),
                ("site_packages", _pc_test.site_packages),
                ("bin", _pc_test.bin),
                ("lib", _pc_test.lib),
                ("include", _pc_test.include),
            ]

            for _name, _checks in _sections:
                if _checks.exists or _checks.not_exists:
                    print(f"    {_name}:")
                    if _checks.exists:
                        print(f"      exists: {_checks.exists}")
                    if _checks.not_exists:
                        print(f"      not_exists: {_checks.not_exists}")
    return


@app.cell(hide_code=True)
def _(mo):
    mo.md(r"""
    ## Step 7: Running Tests

    Now let's run the tests! You can run individual tests by index or all tests at once:
    """)
    return


@app.cell
def _(pkg):
    print("🚀 Running Individual Tests")
    print("=" * 60)

    for i in range(pkg.test_count()):
        print(f"\n▶️  Running test {i}...")
        _result = pkg.run_test(i)

        _status = "✅ PASS" if _result.success else "❌ FAIL"
        print(f"   {_status}")
        print(f"   Test index: {_result.test_index}")

        if _result.output:
            print(f"   Output ({len(_result.output)} lines):")
            for _line in _result.output[:5]:  # Show first 5 lines
                print(f"     {_line}")
            if len(_result.output) > 5:
                print(f"     ... and {len(_result.output) - 5} more lines")
    return


@app.cell
def _(pkg):
    print("🚀 Running All Tests at Once")
    print("=" * 60)

    all_results = pkg.run_tests()

    print(f"\nTotal tests: {len(all_results)}")
    _passed = sum(1 for r in all_results if r.success)
    _failed = len(all_results) - _passed

    print(f"Passed: {_passed}")
    print(f"Failed: {_failed}")

    print("\nResults summary:")
    for _result in all_results:
        _status = "✅" if _result.success else "❌"
        print(f"  {_status} Test {_result.test_index}")

        # TestResult can be used as a boolean
        if _result:
            print("     (result is truthy)")
        else:
            print("     (result is falsy)")
    return (all_results,)


@app.cell(hide_code=True)
def _(mo):
    mo.md(r"""
    ## Step 8: Using Test Results

    The `TestResult` object provides:
    - `success`: Boolean indicating pass/fail
    - `test_index`: Which test was run
    - `output`: List of output/log lines
    - Can be used directly as a boolean in conditions
    """)
    return


@app.cell
def _(all_results):
    print("📊 TestResult Properties")
    print("=" * 60)

    for _result in all_results:
        print(f"\nTest {_result.test_index}:")
        print(f"  success:    {_result.success}")
        print(f"  test_index: {_result.test_index}")
        print(f"  output:     {len(_result.output)} lines")
        print(f"  bool():     {bool(_result)}")
        print(f"  repr():     {repr(_result)}")

    # Example: Filter results
    print("\n" + "=" * 60)
    _passed_tests = [r for r in all_results if r]
    _failed_tests = [r for r in all_results if not r]

    print(f"Passed tests: {[r.test_index for r in _passed_tests]}")
    print(f"Failed tests: {[r.test_index for r in _failed_tests]}")
    return


@app.cell(hide_code=True)
def _(mo):
    mo.md(r"""
    ## Step 9: Running Tests with Custom Configuration

    You can customize test execution with channels, authentication, and other options:
    """)
    return


@app.cell
def _(pkg):
    print("⚙️  Test Configuration Options")
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
    _result = pkg.run_test(
        0,
        channel=["conda-forge"],
        channel_priority="strict",
    )
    print(f"  Result: {'PASS' if _result.success else 'FAIL'}")
    return


@app.cell(hide_code=True)
def _(mo):
    mo.md(r"""
    ## Summary

    In this notebook, you learned how to:

    - **Load packages**: Use `Package.from_file()` to load `.conda` or `.tar.bz2` files
    - **Inspect metadata**: Access `name`, `version`, `depends`, `license`, etc.
    - **Archive information**: Use `archive_type` and `filename` to get package format details
    - **List contents**: Use `files` to see all files in the package
    - **Discover tests**: Access `tests` to see embedded test definitions
    - **Inspect test types**: Use `as_python_test()`, `as_commands_test()`, etc.
    - **Run tests**: Use `run_test(index)` or `run_tests()` to execute tests
    - **Handle results**: `TestResult` provides `success`, `output`, and works as boolean

    The Package API provides a complete interface for inspecting and testing conda packages programmatically!
    """)
    return


if __name__ == "__main__":
    app.run()
