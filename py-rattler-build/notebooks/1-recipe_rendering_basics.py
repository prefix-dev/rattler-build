"""
Educational notebook: Recipe Rendering Basics

This notebook introduces the core concepts of rattler-build recipe rendering:
- Loading recipes from YAML and Python dicts
- Understanding VariantConfig and RenderConfig
- The difference between Stage0 (templates) and Stage1 (evaluated) recipes
- Rendering recipes with variants
"""

import marimo

__generated_with = "0.17.6"
app = marimo.App(width="medium")


@app.cell(hide_code=True)
def _(mo):
    mo.md(r"""
    # 📦 Recipe Rendering Basics

    Welcome to the rattler-build Python bindings tutorial! This notebook will teach you how to:

    1. Load recipes from YAML strings and Python dictionaries
    2. Configure variants (different build configurations)
    3. Render recipes to produce fully evaluated build specifications
    4. Understand the difference between Stage0 (template) and Stage1 (evaluated) recipes

    Let's get started!
    """)
    return


@app.cell
def _():
    import marimo as mo
    from rattler_build.stage0 import (
        Recipe,
        SingleOutputRecipe,
        MultiOutputRecipe,
    )
    from rattler_build.variant_config import VariantConfig
    from rattler_build.render import RenderConfig, render_recipe
    import json
    import pprint

    return (
        MultiOutputRecipe,
        Recipe,
        RenderConfig,
        SingleOutputRecipe,
        VariantConfig,
        json,
        mo,
        pprint,
        render_recipe,
    )


@app.cell(hide_code=True)
def _(mo):
    mo.md(r"""
    ## Example 1: Loading a Simple Recipe from YAML

    The most common way to define a recipe is using YAML format. Let's create a simple package recipe:
    """)
    return


@app.cell
def _(MultiOutputRecipe, Recipe, SingleOutputRecipe, json):
    # Define a simple recipe in YAML format
    simple_recipe_yaml = """
    package:
      name: my-simple-package
      version: "1.0.0"

    build:
      number: 0
      script:
        - echo "Building my package"

    requirements:
      host:
        - python
      run:
        - python

    about:
      homepage: https://github.com/example/my-package
      license: MIT
      summary: A simple example package
    """

    # Parse the YAML into a Recipe object (Stage0)
    simple_recipe = Recipe.from_yaml(simple_recipe_yaml)

    print("Recipe loaded successfully!")
    print(f"Type: {type(simple_recipe).__name__}")
    print(f"Is single output: {isinstance(simple_recipe, SingleOutputRecipe)}")
    print(f"Is multi output: {isinstance(simple_recipe, MultiOutputRecipe)}")
    print("\nRecipe structure (as dict):")
    print(json.dumps(simple_recipe.to_dict(), indent=2))
    return


@app.cell(hide_code=True)
def _(mo):
    mo.md(r"""
    ## Example 2: Creating a Recipe from a Python Dictionary

    You can also create recipes programmatically using Python dictionaries. This is useful when generating recipes dynamically:
    """)
    return


@app.cell
def _(Recipe, json):
    # Define the same recipe as a Python dictionary
    recipe_dict = {
        "package": {"name": "my-dict-package", "version": "2.0.0"},
        "build": {"number": 1, "script": ["pip install ."]},
        "requirements": {"host": ["python", "pip", "setuptools"], "run": ["python", "numpy"]},
        "about": {
            "homepage": "https://example.com",
            "license": "Apache-2.0",
            "summary": "A package created from a Python dict",
        },
    }

    # Create Recipe from dictionary
    dict_recipe = Recipe.from_dict(recipe_dict)

    print("Recipe created from dictionary!")
    print(f"Package name: {dict_recipe.package.name}")
    print(f"Package version: {dict_recipe.package.version}")
    print(f"Build number: {dict_recipe.build.number}")
    print(f"Host requirements: {dict_recipe.requirements.host}")
    print(f"Run requirements: {dict_recipe.requirements.run}")
    print("\nFull recipe:")
    print(json.dumps(dict_recipe.to_dict(), indent=2))
    return


@app.cell(hide_code=True)
def _(mo):
    mo.md(r"""
    ## Example 3: Understanding VariantConfig

    Variants allow you to build the same package with different configurations (e.g., different Python versions, compilers, or dependencies). Let's explore how VariantConfig works:
    """)
    return


@app.cell
def _(VariantConfig, json, pprint):
    # Create a VariantConfig from a dictionary
    variant_dict = {
        "python": ["3.9", "3.10", "3.11"],
        "numpy": ["1.21", "1.22", "1.23"],
    }
    variant_config = VariantConfig(variant_dict)

    print("✨ Variant Configuration Created")
    print("=" * 60)
    print(f"Variant keys: {variant_config.keys()}")
    print(f"Python versions: {variant_config.get_values('python')}")
    print(f"Numpy versions: {variant_config.get_values('numpy')}")
    print(f"\nTotal number of variant combinations: {len(variant_config.combinations())}")
    print("\nAll possible combinations:")
    pprint.pprint(variant_config.combinations())
    print("\nVariant config as dict:")
    print(json.dumps(variant_config.to_dict(), indent=2))
    return


@app.cell(hide_code=True)
def _(mo):
    mo.md(r"""
    ### Zip Keys: Synchronizing Variants

    By default, VariantConfig creates all possible combinations (Cartesian product). But sometimes you want to pair specific variants together. That's what `zip_keys` is for:
    """)
    return


@app.cell
def _(VariantConfig, pprint):
    # Create variant config with paired values
    paired_variant = VariantConfig({"python": ["3.9", "3.10", "3.11"], "numpy": ["1.21", "1.22", "1.23"]})

    # Zip python and numpy together (same index)
    paired_variant.zip_keys = [["python", "numpy"]]

    print("🔗 Zipped Variants (python paired with numpy)")
    print("=" * 60)
    print("Without zip_keys: 3 × 3 = 9 combinations")
    print("With zip_keys: 3 combinations (paired by index)")
    print("\nPaired combinations:")
    pprint.pprint(paired_variant.combinations())
    return


@app.cell(hide_code=True)
def _(mo):
    mo.md(r"""
    ## Example 4: RenderConfig - Controlling the Build Environment

    RenderConfig lets you specify the target platform and add custom context variables for recipe rendering:
    """)
    return


@app.cell
def _(RenderConfig, json):
    # Create a render config with custom settings
    render_config = RenderConfig(
        target_platform="linux-64",
        build_platform="linux-64",
        host_platform="linux-64",
        experimental=False,
    )

    # Add custom context variables (available in Jinja templates)
    render_config.set_context("custom_var", "custom_value")
    render_config.set_context("build_timestamp", "2024-01-01")
    render_config.set_context("my_number", 42)

    print("🔧 Render Configuration")
    print("=" * 60)
    print(f"Target platform: {render_config.target_platform}")
    print(f"Build platform: {render_config.build_platform}")
    print(f"Host platform: {render_config.host_platform}")
    print(f"Experimental: {render_config.experimental}")
    print("\nCustom context variables:")
    print(json.dumps(render_config.get_all_context(), indent=2))
    return


@app.cell(hide_code=True)
def _(mo):
    mo.md(r"""
    ## Example 5: Stage0 vs Stage1 - Templates vs Evaluated Recipes

    **Stage0** is the parsed recipe with Jinja templates still intact (e.g., `${{ python }}`).
    **Stage1** is the fully evaluated recipe with all templates resolved to actual values.

    Let's see the difference:
    """)
    return


@app.cell
def _(Recipe, json):
    # Create a recipe with Jinja templates
    templated_recipe_yaml = """
    context:
      name: templated-package
      version: "1.5.0"

    package:
      name: ${{ name }}
      version: ${{ version }}

    build:
      number: 0

    requirements:
      host:
        - python ${{ python }}.*
      run:
        - python
        - numpy >=${{ numpy }}
    """

    # This is Stage0 - templates are NOT evaluated yet
    stage0_recipe = Recipe.from_yaml(templated_recipe_yaml)

    print("📋 STAGE 0 (Parsed, templates intact)")
    print("=" * 60)
    print(f"Package name (raw): {stage0_recipe.package.name}")
    print(f"Package version (raw): {stage0_recipe.package.version}")
    print(f"Host requirements (raw): {stage0_recipe.requirements.host}")
    print("\nStage0 recipe structure:")
    print(json.dumps(stage0_recipe.to_dict(), indent=2))
    return (stage0_recipe,)


@app.cell
def _(RenderConfig, VariantConfig, json, render_recipe, stage0_recipe):
    # Now let's render it with variants
    stage0_variant = VariantConfig({"python": ["3.10"], "numpy": ["1.22"]})
    stage0_render = RenderConfig()

    rendered_variants = render_recipe(stage0_recipe, stage0_variant, stage0_render)

    # Get the first (and only) variant
    rendered = rendered_variants[0]
    stage1_recipe = rendered.recipe()

    print("\n" + "=" * 60)
    print("📦 STAGE 1 (Rendered, templates evaluated)")
    print("=" * 60)
    print(f"Package name (evaluated): {stage1_recipe.package.name}")
    print(f"Package version (evaluated): {stage1_recipe.package.version}")
    print(f"Host requirements (evaluated): {stage1_recipe.requirements.host}")
    print(f"Run requirements (evaluated): {stage1_recipe.requirements.run}")
    print(f"\nUsed variant: {stage1_recipe.used_variant}")
    print("\nStage1 recipe structure:")
    print(json.dumps(stage1_recipe.to_dict(), indent=2))
    return


@app.cell(hide_code=True)
def _(mo):
    mo.md(r"""
    ## Example 6: Rendering Multiple Variants

    Now let's put it all together and render a recipe with multiple variants to see how different configurations are generated:
    """)
    return


@app.cell
def _(Recipe, RenderConfig, VariantConfig, json, render_recipe):
    # Recipe with variant placeholders
    multi_variant_yaml = """
    context:
      name: multi-variant-pkg
      version: "3.0.0"

    package:
      name: ${{ name }}
      version: ${{ version }}

    build:
      number: 0

    requirements:
      host:
        - python ${{ python }}.*
        - numpy ${{ numpy }}.*
      run:
        - python
        - numpy >=${{ numpy }}
    """

    mv_recipe = Recipe.from_yaml(multi_variant_yaml)

    # Create variant config with multiple python and numpy versions
    mv_variants = VariantConfig({"python": ["3.9", "3.10", "3.11"], "numpy": ["1.21", "1.22"]})

    mv_render_config = RenderConfig(target_platform="linux-64")

    # Render all variants
    all_rendered = render_recipe(mv_recipe, mv_variants, mv_render_config)

    print(f"🎯 Rendered {len(all_rendered)} variant(s)")
    print("=" * 60)

    for i, rendered_variant in enumerate(all_rendered, 1):
        variant_values = rendered_variant.variant()
        stage1 = rendered_variant.recipe()

        print(f"\nVariant {i}:")
        print(f"  Variant config: {json.dumps(variant_values, indent=4)}")
        print(f"  Package name: {stage1.package.name}")
        print(f"  Package version: {stage1.package.version}")
        print(f"  Python: {variant_values.get('python')}")
        print(f"  Numpy: {variant_values.get('numpy')}")
        print(f"  Host requirements: {stage1.requirements.host}")
        print(f"  Run requirements: {stage1.requirements.run}")

    print("\n" + "=" * 60)
    print("✅ Recipe rendering complete!")
    return


@app.cell(hide_code=True)
def _(mo):
    mo.md(r"""
    ## Summary

    In this notebook, you learned:

    - **Recipe Creation**: Load recipes from YAML strings (`Recipe.from_yaml()`) or Python dicts (`Recipe.from_dict()`)
    - **VariantConfig**: Define build variants and use `zip_keys` to pair specific combinations
    - **RenderConfig**: Configure target platforms and add custom context variables
    - **Stage0 vs Stage1**: Understand the difference between parsed templates and evaluated recipes
    - **Rendering**: Use `render_recipe()` to transform Stage0 → Stage1 with variants

    Next, explore the other notebooks to learn about:
    - Advanced Jinja templating and conditional variants
    - Multi-output recipes and staging caches
    """)
    return


if __name__ == "__main__":
    app.run()
