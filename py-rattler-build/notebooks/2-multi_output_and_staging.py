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
    ## Example 1: Simple Multi-Output Recipe

    Multi-output recipes allow you to build multiple packages from a single recipe. This is useful when you have a project that produces both a library and tools, or when you want to split debug symbols from the main package.

    Let's create a simple multi-output recipe:
    """)
    return


@app.cell
def _(MultiOutputRecipe, Recipe, RenderConfig, VariantConfig):
    # Simple multi-output recipe
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
            - if: unix
              then: |
                echo "Building library..."
                mkdir -p $PREFIX/lib
                echo "myproject-lib v2.0.0" > $PREFIX/lib/myproject.txt
            - if: win
              then: |
                echo Building library...
                mkdir %PREFIX%\\lib
                echo myproject-lib v2.0.0 > %PREFIX%\\lib\\myproject.txt

      # Second output: Command-line tools
      - package:
          name: ${{ name }}-tools
        build:
          script:
            - if: unix
              then: |
                echo "Building tools..."
                mkdir -p $PREFIX/bin
                echo "#!/bin/sh" > $PREFIX/bin/mytool
                echo "echo 'Hello from mytool!'" >> $PREFIX/bin/mytool
                chmod +x $PREFIX/bin/mytool
            - if: win
              then: |
                echo Building tools...
                mkdir %PREFIX%\\bin
                echo @echo Hello from mytool! > %PREFIX%\\bin\\mytool.bat
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
    ## Example 2: Staging Outputs - Intermediate Build Artifacts

    Staging outputs are temporary build artifacts that aren't packaged but can be used by other outputs. They're perfect for compiled artifacts that multiple packages need.

    Common use case: Compile a C++ library once, then use it in multiple Python packages.
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
      # Staging output: Simulates compiling shared artifacts
      - staging:
          name: shared-build
        build:
          script:
            - if: unix
              then: |
                echo "Building shared artifacts..."
                mkdir -p $PREFIX/lib
                echo "Shared library v1.5.0" > $PREFIX/lib/libshared.txt
            - if: win
              then: |
                echo Building shared artifacts...
                mkdir %PREFIX%\\lib
                echo Shared library v1.5.0 > %PREFIX%\\lib\\libshared.txt

      # Package output 1: Python bindings (uses staging)
      - package:
          name: ${{ name }}-python
        build:
          script:
            - if: unix
              then: |
                echo "Building Python bindings..."
                mkdir -p $PREFIX/lib/python
                echo "Python bindings using shared lib" > $PREFIX/lib/python/bindings.txt
            - if: win
              then: |
                echo Building Python bindings...
                mkdir %PREFIX%\\lib\\python
                echo Python bindings using shared lib > %PREFIX%\\lib\\python\\bindings.txt
        inherit: shared-build

      # Package output 2: CLI tool (uses staging)
      - package:
          name: ${{ name }}-cli
        build:
          script:
            - if: unix
              then: |
                echo "Building CLI tool..."
                mkdir -p $PREFIX/bin
                echo "#!/bin/sh" > $PREFIX/bin/compiled-tool
                echo "echo CLI tool using shared lib" >> $PREFIX/bin/compiled-tool
                chmod +x $PREFIX/bin/compiled-tool
            - if: win
              then: |
                echo Building CLI tool...
                mkdir %PREFIX%\\bin
                echo @echo CLI tool using shared lib > %PREFIX%\\bin\\compiled-tool.bat
        inherit: shared-build
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
