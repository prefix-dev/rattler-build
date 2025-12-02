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

    import marimo as mo

    from rattler_build.render import RenderConfig
    from rattler_build.stage0 import MultiOutputRecipe, Recipe
    from rattler_build.tool_config import PlatformConfig
    from rattler_build.variant_config import VariantConfig

    return (
        MultiOutputRecipe,
        PlatformConfig,
        Recipe,
        RenderConfig,
        VariantConfig,
        json,
        mo,
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
            - echo "Building library..."
        requirements:
          host:
            - python
          run:
            - python

      # Second output: Command-line tools
      - package:
          name: ${{ name }}-tools
        build:
          script:
            - echo "Building tools..."
        requirements:
          run:
            - ${{ name }}-lib
            - click
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
      # Staging output: Compile the C++ library
      - staging:
          name: cpp-build
        build:
          script:
            - echo "Compiling C++ library..."
            - echo "g++ -c library.cpp -o library.o"
        requirements:
          build:
            - ${{ compiler('cxx') }}
          host:
            - python

      # Package output 1: Python bindings (uses staging)
      - package:
          name: ${{ name }}-python
        build:
          script:
            - echo "Building Python bindings..."
            - echo "Using compiled artifacts from cpp-build"
        requirements:
          host:
            - python
            - pybind11
          run:
            - python
        inherit: cpp-build

      # Package output 2: CLI tool (uses staging)
      - package:
          name: ${{ name }}-cli
        build:
          script:
            - echo "Building CLI tool..."
            - echo "Using compiled artifacts from cpp-build"
        requirements:
          run:
            - ${{ name }}-python
        inherit: cpp-build
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
                print(f"       Build requirements: {_cache.requirements.build}")

        # Check inheritance
        if _stage1.inherits_from:
            print(f"   Inherits from: {json.dumps(_stage1.inherits_from, indent=6)}")
    return


@app.cell(hide_code=True)
def _(mo):
    mo.md(r"""
    ## Example 3: Complete Pipeline Visualization

    Let's create a comprehensive example showing the entire pipeline from Stage0 to Stage1 for a multi-output recipe:
    """)
    return


@app.cell
def _(PlatformConfig, Recipe, RenderConfig, VariantConfig, json):
    # Comprehensive recipe
    complete_yaml = """
    schema_version: 1

    context:
      name: complete-example
      version: "4.2.0"
      description: A complete pipeline example

    recipe:
      version: ${{ version }}

    build:
      number: 0

    about:
      homepage: https://example.com/complete
      license: MIT
      summary: ${{ description }}

    outputs:
      # Stage 1: Build C extension
      - staging:
          name: c-extension
        build:
          script:
            - gcc -shared -o extension.so extension.c
        requirements:
          build:
            - ${{ compiler('c') }}
        source:
          - url: https://example.com/source.tar.gz

      # Package 1: Python library
      - package:
          name: py-${{ name }}
        build:
          script:
            - pip install .
          noarch: python
        requirements:
          host:
            - python ${{ python }}.*
            - pip
          run:
            - python
        inherit: c-extension

      # Package 2: Command-line interface
      - package:
          name: ${{ name }}-cli
        build:
          script:
            - install -m 755 cli.py $PREFIX/bin/${{ name }}
          python:
            entry_points:
              - ${{ name }} = cli:main
        requirements:
          run:
            - python
            - py-${{ name }}
            - click >=8.0
        inherit: c-extension
    """

    # STAGE 0: Parse the recipe
    print("🔵 STAGE 0: Parsing Recipe (Templates Intact)")
    print("=" * 60)
    complete_stage0 = Recipe.from_yaml(complete_yaml)

    print(f"Recipe type: {type(complete_stage0).__name__}")
    print(f"Context: {complete_stage0.context}")
    print(f"Number of outputs: {len(complete_stage0.outputs)}")
    print("\nStage0 structure (with templates):")
    stage0_dict = complete_stage0.to_dict()
    print(json.dumps(stage0_dict, indent=2)[:1000] + "...")

    # Create variants and render config
    print("\n\n🟡 CONFIGURATION: Variants and Render Config")
    print("=" * 60)
    complete_variants = VariantConfig({"python": ["3.10", "3.11"]})
    platform_config = PlatformConfig("linux-64")
    complete_render = RenderConfig(platform=platform_config)

    print(f"Variants: {complete_variants.to_dict()}")
    print(f"Target platform: {complete_render.target_platform}")
    print(f"Number of variant combinations: {len(complete_variants.combinations())}")

    # RENDERING: Stage0 → Stage1
    print("\n\n⚙️  RENDERING: Stage0 → Stage1")
    print("=" * 60)
    complete_results = complete_stage0.render(complete_variants, complete_render)
    print(f"Rendered {len(complete_results)} package variant(s)")

    # STAGE 1: Examine rendered recipes
    print("\n\n🟢 STAGE 1: Fully Evaluated Recipes")
    print("=" * 60)

    for _idx, _result in enumerate(complete_results, 1):
        _variant = _result.variant()
        _stage1 = _result.recipe()

        print(f"\n{'=' * 60}")
        print(f"Package {_idx}: {_stage1.package.name}")
        print(f"{'=' * 60}")

        print("\n  Variant used:")
        print(f"    {json.dumps(_variant, indent=4)}")

        print("\n  Package info:")
        print(f"    Name: {_stage1.package.name}")
        print(f"    Version: {_stage1.package.version}")

        print("\n  Build info:")
        print(f"    Number: {_stage1.build.number}")
        print(f"    Script: {_stage1.build.script}")
        print(f"    Noarch: {_stage1.build.noarch}")

        print("\n  Requirements:")
        print(f"    Build: {_stage1.requirements.build}")
        print(f"    Host: {_stage1.requirements.host}")
        print(f"    Run: {_stage1.requirements.run}")

        if _stage1.staging_caches:
            print(f"\n  Staging caches ({len(_stage1.staging_caches)}):")
            for _cache in _stage1.staging_caches:
                print(f"    - {_cache.name}")
                print(f"      Script: {_cache.build.script}")
                print(f"      Build deps: {_cache.requirements.build}")

        if _stage1.inherits_from:
            print("\n  Inherits from:")
            print(f"    {json.dumps(_stage1.inherits_from, indent=4)}")

    print("\n\n✅ PIPELINE COMPLETE")
    print("=" * 60)
    print("Summary:")
    print("  - Started with 1 multi-output recipe")
    print(f"  - Applied {len(complete_variants.combinations())} variant combination(s)")
    print(f"  - Produced {len(complete_results)} package build(s)")
    print("  - Each with staging cache for shared build artifacts")
    return


@app.cell(hide_code=True)
def _(mo):
    mo.md(r"""
    ## Summary

    In this notebook, you learned about multi-output recipes and staging:

    - **Multi-Output Recipes**: Build multiple packages from one recipe using the `outputs` list
    - **Staging Outputs**: Create temporary build artifacts with `staging:` that other packages can inherit
    - **Complete Pipeline**: Understand the full Stage0 → Rendering → Stage1 workflow

    Key takeaways:
    - Staging outputs reduce redundant compilation
    - Multi-output recipes keep related packages together
    - Inheritance allows flexible package composition
    - Variants multiply outputs (N variants × M outputs = N×M packages)

    You now have a comprehensive understanding of the rattler-build Python bindings!
    """)
    return


if __name__ == "__main__":
    app.run()
