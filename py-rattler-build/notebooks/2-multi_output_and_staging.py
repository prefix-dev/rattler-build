"""
Educational notebook: Multi-Output Recipes and Staging Caches

This notebook explores advanced recipe features:
- Multi-output recipes (building multiple packages from one recipe)
- Staging outputs (temporary build artifacts)
- Output inheritance (reusing build configurations)
- Stage1 staging_caches inspection
- Real-world multi-output scenarios
"""

import marimo

__generated_with = "0.17.6"
app = marimo.App(width="medium")


@app.cell(hide_code=True)
def _(mo):
    mo.md(r"""
    # 📦 Multi-Output Recipes and Staging Caches

    This notebook teaches you about advanced recipe structures:

    1. Multi-output recipes - Build multiple packages from one source
    2. Staging outputs - Create temporary build artifacts
    3. Output inheritance - Reuse build configurations
    4. Inspecting Stage1 staging caches
    5. Variants with multi-output recipes
    6. Complete build pipeline visualization

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

    from rattler_build.render import RenderConfig
    from rattler_build.stage0 import MultiOutputRecipe, Recipe
    from rattler_build.variant_config import VariantConfig

    return (
        MultiOutputRecipe,
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
    ## Example 1: Multi-Output with Inter-Output Dependencies

    Multi-output recipes build multiple packages from one recipe. When one output needs another, you list it as a **host** (or build) **dependency**. The dependency package is installed into the build environment, and your build script can use its files.

    In this example:
    - `myproject-lib` creates a Python module
    - `myproject-tools` depends on `myproject-lib` as a host dependency and reads that file during its build
    """)
    return


@app.cell
def _(MultiOutputRecipe, Recipe, RenderConfig, VariantConfig):
    # Multi-output recipe with inter-output dependency
    multi_output_yaml = """
    schema_version: 1

    context:
      name: myproject
      version: "2.0.0"

    recipe:
      version: ${{ version }}

    outputs:
      # First output: The library
      - package:
          name: ${{ name }}-lib
        build:
          script:
            interpreter: python
            content: |
              import os
              from pathlib import Path

              prefix = Path(os.environ["PREFIX"])
              lib_dir = prefix / "lib" / "python"
              lib_dir.mkdir(parents=True, exist_ok=True)

              (lib_dir / "myproject_lib.py").write_text('VERSION = "2.0.0"')
              print(f"Created library at {lib_dir}")
        requirements:
          build:
            - python

      # Second output: Uses the library as a host dependency
      - package:
          name: ${{ name }}-tools
        build:
          script:
            interpreter: python
            content: |
              import os
              from pathlib import Path

              prefix = Path(os.environ["PREFIX"])

              # Read and print the lib file (installed as host dependency)
              lib_file = prefix / "lib" / "python" / "myproject_lib.py"
              print(f"Reading library from: {lib_file}")
              print(lib_file.read_text())
        requirements:
          build:
            - python
          host:
            - ${{ name }}-lib
    """

    multi_recipe = Recipe.from_yaml(multi_output_yaml)

    print("📦 Multi-Output Recipe Loaded")
    print("=" * 60)
    print(f"Recipe type: {type(multi_recipe).__name__}")
    print(f"Is multi-output: {isinstance(multi_recipe, MultiOutputRecipe)}")
    print(f"Number of outputs: {len(multi_recipe.outputs)}")

    print("\nOutputs:")
    for _idx, _output in enumerate(multi_recipe.outputs, 1):
        print(f"  {_idx}. {_output.to_dict()['package']['name']}")

    # Render the recipe
    mo_variants = VariantConfig()
    mo_render = RenderConfig()
    mo_results = multi_recipe.render(mo_variants, mo_render)

    print(f"\n✨ Rendered {len(mo_results)} package(s):")
    print("=" * 60)

    for _result in mo_results:
        _stage1 = _result.recipe()
        print(f"\n📦 Package: {_stage1.package.name} {_stage1.package.version}")
        print(f"   Build script: {_stage1.build.script}")
        print(f"   Run requirements: {_stage1.requirements.run}")
    return (mo_results,)


@app.cell
def _(Path, mo_results, shutil, tempfile):
    # Create persistent temp directories (clean up from previous runs)
    _recipe_tmpdir = Path(tempfile.gettempdir()) / "rattler_build_multi_output"
    _output_tmpdir = Path(tempfile.gettempdir()) / "rattler_build_multi_output_output"

    # Clean up from previous runs
    if _recipe_tmpdir.exists():
        shutil.rmtree(_recipe_tmpdir)
    if _output_tmpdir.exists():
        shutil.rmtree(_output_tmpdir)

    # Create the directories
    _recipe_tmpdir.mkdir(parents=True)
    _output_tmpdir.mkdir(parents=True)

    # Define dummy recipe path
    _recipe_path = _recipe_tmpdir / "recipe.yaml"

    # Build each variant
    print("🔨 Building Example 1 packages...")
    print("=" * 60)
    print(f"Recipe directory: {_recipe_tmpdir}")
    print(f"Output directory: {_output_tmpdir}")

    for _i, _variant in enumerate(mo_results, 1):
        print(f"\n📦 Building variant {_i}/{len(mo_results)}")
        _stage1_recipe = _variant.recipe()
        _package = _stage1_recipe.package
        _build = _stage1_recipe.build

        print(f"  Package: {_package.name}")
        print(f"  Version: {_package.version}")
        print(f"  Build string: {_build.string}")

        _result = _variant.run_build(
            progress_callback=None,
            keep_build=False,
            output_dir=_output_tmpdir,
            recipe_path=_recipe_path,
        )

        # Display build result information
        print(f"  ✅ Build complete in {_result.build_time:.2f}s!")
        print(f"  📦 Package: {_result.packages[0]}")
        if _result.variant:
            print(f"  🔧 Variant: {_result.variant}")

        # Display build log
        if _result.log:
            print(f"  📋 Build log: {len(_result.log)} messages captured")

    print("\n" + "=" * 60)
    print("🎉 Example 1 builds completed successfully!")
    print(f"\n📦 Built packages are available in: {_output_tmpdir}")
    return


@app.cell(hide_code=True)
def _(mo):
    mo.md(r"""
    ## Example 2: Staging Outputs - Shared Build Artifacts

    Staging is different from regular dependencies (Example 1). A staging output runs its build script once, then **copies its files directly into each inheriting package's prefix**. Since these are "new" files in the prefix, they will be included in the final package.

    Use the `files` field to select which subset of files to include from the staging prefix.

    In this example:
    - `shared-build` staging creates both `/lib/shared.py` AND `/bin/tool.py`
    - `compiled-project-python` inherits and uses `files: [lib/**]` to only include lib files
    - `compiled-project-cli` inherits and uses `files: [bin/**]` to only include bin files
    """)
    return


@app.cell
def _(Recipe, RenderConfig, VariantConfig, json):
    # Recipe with staging output
    staging_yaml = """
    schema_version: 1

    context:
      name: compiled-project
      version: "1.5.0"

    recipe:
      version: ${{ version }}

    outputs:
      # Staging output: Creates shared artifacts for multiple packages
      - staging:
          name: shared-build
        build:
          script:
            interpreter: python
            content: |
              import os
              from pathlib import Path

              prefix = Path(os.environ["PREFIX"])

              # Create lib files
              lib_dir = prefix / "lib"
              lib_dir.mkdir(parents=True, exist_ok=True)
              (lib_dir / "shared.py").write_text('SHARED_VERSION = "1.5.0"')
              print(f"Created shared library at {lib_dir}")

              # Create bin files
              bin_dir = prefix / "bin"
              bin_dir.mkdir(parents=True, exist_ok=True)
              (bin_dir / "tool.py").write_text('#!/usr/bin/env python\\nprint("CLI tool")')
              print(f"Created CLI tool at {bin_dir}")
        requirements:
          build:
            - python

      # Package output 1: Python bindings (inherits lib files from staging)
      - package:
          name: ${{ name }}-python
        inherit: shared-build
        build:
          files:
            - lib/**

      # Package output 2: CLI tool (inherits bin files from staging)
      - package:
          name: ${{ name }}-cli
        inherit: shared-build
        build:
          files:
            - bin/**
    """

    staging_recipe = Recipe.from_yaml(staging_yaml)

    print("🏗️  Recipe with Staging Output")
    print("=" * 60)
    print(f"Total outputs defined: {len(staging_recipe.outputs)}")

    print("\nOutput types:")
    for _idx, _output in enumerate(staging_recipe.outputs, 1):
        _output_dict = _output.to_dict()
        if "staging" in _output_dict:
            print(f"  {_idx}. Staging: {_output_dict['staging']['name']}")
        elif "package" in _output_dict:
            _pkg_name = _output_dict["package"]["name"]
            _inherits = _output_dict.get("inherits_from", None)
            print(f"  {_idx}. Package: {_pkg_name}", end="")
            if _inherits:
                print(f" (inherits from: {_inherits})")
            else:
                print()

    # Render the recipe
    staging_variants = VariantConfig()
    staging_render = RenderConfig()
    staging_results = staging_recipe.render(staging_variants, staging_render)

    print(f"\n📦 Rendered {len(staging_results)} package(s)")
    print("(Staging outputs don't produce packages)")
    print("=" * 60)

    for _result in staging_results:
        _stage1 = _result.recipe()
        print(f"\n📦 {_stage1.package.name} {_stage1.package.version}")

        # Check for staging caches
        if _stage1.staging_caches:
            print(f"   Uses {len(_stage1.staging_caches)} staging cache(s):")
            for _cache in _stage1.staging_caches:
                print(f"     - {_cache.name}")
                print(f"       Build script: {_cache.build.script}")

        # Check inheritance
        if _stage1.inherits_from:
            print(f"   Inherits from: {json.dumps(_stage1.inherits_from, indent=6)}")
    return (staging_results,)


@app.cell
def _(Path, shutil, staging_results, tempfile):
    # Create persistent temp directories (clean up from previous runs)
    _recipe_tmpdir = Path(tempfile.gettempdir()) / "rattler_build_staging"
    _output_tmpdir = Path(tempfile.gettempdir()) / "rattler_build_staging_output"

    # Clean up from previous runs
    if _recipe_tmpdir.exists():
        shutil.rmtree(_recipe_tmpdir)
    if _output_tmpdir.exists():
        shutil.rmtree(_output_tmpdir)

    # Create the directories
    _recipe_tmpdir.mkdir(parents=True)
    _output_tmpdir.mkdir(parents=True)

    # Define dummy recipe path
    _recipe_path = _recipe_tmpdir / "recipe.yaml"

    # Build each variant
    print("🔨 Building Example 2 packages (with staging)...")
    print("=" * 60)
    print(f"Recipe directory: {_recipe_tmpdir}")
    print(f"Output directory: {_output_tmpdir}")

    for _i, _variant in enumerate(staging_results, 1):
        print(f"\n📦 Building variant {_i}/{len(staging_results)}")
        _stage1_recipe = _variant.recipe()
        _package = _stage1_recipe.package
        _build = _stage1_recipe.build

        print(f"  Package: {_package.name}")
        print(f"  Version: {_package.version}")
        print(f"  Build string: {_build.string}")

        if _stage1_recipe.staging_caches:
            print(f"  Staging caches: {[c.name for c in _stage1_recipe.staging_caches]}")

        _result = _variant.run_build(
            progress_callback=None,
            keep_build=False,
            output_dir=_output_tmpdir,
            recipe_path=_recipe_path,
        )

        # Display build result information
        print(f"  ✅ Build complete in {_result.build_time:.2f}s!")
        print(f"  📦 Package: {_result.packages[0]}")
        if _result.variant:
            print(f"  🔧 Variant: {_result.variant}")

        # Display build log
        if _result.log:
            print(f"  📋 Build log: {len(_result.log)} messages captured")

    print("\n" + "=" * 60)
    print("🎉 Example 2 builds completed successfully!")
    print(f"\n📦 Built packages are available in: {_output_tmpdir}")
    return


@app.cell(hide_code=True)
def _(mo):
    mo.md(r"""
    ## Summary

    In this notebook, you learned about multi-output recipes and staging:

    - **Multi-Output Recipes**: Build multiple packages from one recipe using the `outputs` list
    - **Staging Outputs**: Create temporary build artifacts with `staging:` that other packages can inherit
    """)
    return


if __name__ == "__main__":
    app.run()
