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
    import yaml
    import tempfile
    from pathlib import Path
    return (
        MultiOutputRecipe,
        Path,
        Recipe,
        RenderConfig,
        SingleOutputRecipe,
        VariantConfig,
        json,
        mo,
        pprint,
        render_recipe,
        tempfile,
        yaml,
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
    # Define a simple recipe in YAML format with Jinja templates
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
        - python ${{ python }}.*
        - numpy ${{ numpy }}.*
      run:
        - python
        - numpy >=${{ numpy }}

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
    return simple_recipe, simple_recipe_yaml


@app.cell(hide_code=True)
def _(mo):
    mo.md(r"""
    ## Example 2: Creating a Recipe from a Python Dictionary

    You can also create recipes from Python dictionaries. Let's verify that `Recipe.from_yaml()` and `Recipe.from_dict()` produce the same result when given the same data:
    """)
    return


@app.cell
def _(Recipe, simple_recipe, simple_recipe_yaml, yaml):
    # Parse the same YAML as a Python dictionary
    recipe_dict = yaml.safe_load(simple_recipe_yaml)

    # Create Recipe from dictionary
    dict_recipe = Recipe.from_dict(recipe_dict)

    print("Recipe created from dictionary!")

    # Assert that both recipes are the same
    yaml_dict = simple_recipe.to_dict()
    dict_dict = dict_recipe.to_dict()
    assert yaml_dict == dict_dict, "Recipes should be identical!"

    print("\n✅ Both recipes are identical!")
    return


@app.cell(hide_code=True)
def _(mo):
    mo.md(r"""
    ## Example 3: Understanding VariantConfig with Zip Keys

    Variants allow you to build the same package with different configurations (e.g., different Python versions, compilers, or dependencies). By default, VariantConfig creates all possible combinations (Cartesian product), but we can use `zip_keys` to pair specific variants together:
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

    print("✨ Variant Configuration")
    print("=" * 60)
    print(f"Variant keys: {variant_config.keys()}")
    print(f"Python versions: {variant_config.get_values('python')}")
    print(f"Numpy versions: {variant_config.get_values('numpy')}")

    print("\n📊 WITHOUT zip_keys (Cartesian product):")
    print(f"Total combinations: {len(variant_config.combinations())} (3 × 3)")
    print("\nAll possible combinations:")
    pprint.pprint(variant_config.combinations())

    # Zip python and numpy together (same index)
    variant_config.zip_keys = [["python", "numpy"]]

    print("\n" + "=" * 60)
    print("🔗 WITH zip_keys (paired by index):")
    print(f"Zip keys: {variant_config.zip_keys}")
    print(f"Total combinations: {len(variant_config.combinations())} (paired)")
    print("\nPaired combinations:")
    pprint.pprint(variant_config.combinations())

    print("\nVariant config as dict:")
    print(json.dumps(variant_config.to_dict(), indent=2))
    return (variant_config,)


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
    return (render_config,)


@app.cell(hide_code=True)
def _(mo):
    mo.md(r"""
    ## Example 5: Rendering Recipe with Variants

    Now let's put it all together! We'll use the recipe from Example 1, the variant config from Example 3, and the render config from Example 4 to render our recipe with multiple variants.

    **Stage0** is the parsed recipe with Jinja templates still intact (e.g., `${{ python }}`).
    **Stage1** is the fully evaluated recipe with all templates resolved to actual values.
    """)
    return


@app.cell
def _(json, render_config, render_recipe, simple_recipe, variant_config):
    # Render the recipe with all the configurations we've created
    rendered_variants = render_recipe(simple_recipe, variant_config, render_config)

    print("📋 STAGE 0 (Parsed, templates intact)")
    print("=" * 60)
    print(f"Package name (raw): {simple_recipe.package.name}")
    print(f"Package version (raw): {simple_recipe.package.version}")
    print(f"Host requirements (raw): {simple_recipe.requirements.host}")

    print(f"\n🎯 Rendered {len(rendered_variants)} variant(s)")
    print("=" * 60)

    for _i, _rendered_variant in enumerate(rendered_variants, 1):
        _variant_values = _rendered_variant.variant()
        _stage1_recipe = _rendered_variant.recipe()

        print(f"\n📦 STAGE 1 - Variant {_i} (Rendered, templates evaluated)")
        print("-" * 60)
        print(f"  Variant config: {json.dumps(_variant_values, indent=4)}")
        print(f"  Package name: {_stage1_recipe.package.name}")
        print(f"  Package version: {_stage1_recipe.package.version}")
        print(f"  Python: {_variant_values.get('python')}")
        print(f"  Numpy: {_variant_values.get('numpy')}")
        print(f"  Host requirements: {_stage1_recipe.requirements.host}")
        print(f"  Run requirements: {_stage1_recipe.requirements.run}")
        print(f"  Build string: {_stage1_recipe.build.string}")

    print("\n" + "=" * 60)
    print("✅ Recipe rendering complete!")
    return (rendered_variants,)


@app.cell(hide_code=True)
def _(mo):
    mo.md(r"""
    ## Example 6: Building the Package

    Finally, let's actually build the package! We'll take the rendered variants and build them into conda packages:
    """)
    return


@app.cell
def _(Path, rendered_variants, simple_recipe_yaml, tempfile):
    import shutil

    # Create persistent temp directories (clean up from previous runs)
    _recipe_tmpdir = Path(tempfile.gettempdir()) / "rattler_build_recipe"
    _output_tmpdir = Path(tempfile.gettempdir()) / "rattler_build_output"

    # Clean up from previous runs
    if _recipe_tmpdir.exists():
        shutil.rmtree(_recipe_tmpdir)
    if _output_tmpdir.exists():
        shutil.rmtree(_output_tmpdir)

    # Create the directories
    _recipe_tmpdir.mkdir(parents=True, exist_ok=True)
    _output_tmpdir.mkdir(parents=True, exist_ok=True)

    # Write recipe file
    _recipe_path = _recipe_tmpdir / "recipe.yaml"
    _recipe_path.write_text(simple_recipe_yaml)

    # Build each variant
    print("🔨 Building packages...")
    print("=" * 60)
    print(f"Recipe directory: {_recipe_tmpdir}")
    print(f"Output directory: {_output_tmpdir}")

    for _i, _variant in enumerate(rendered_variants, 1):
        print(f"\n📦 Building variant {_i}/{len(rendered_variants)}")
        _stage1_recipe = _variant.recipe()
        _package = _stage1_recipe.package
        _build = _stage1_recipe.build

        print(f"  Package: {_package.name}")
        print(f"  Version: {_package.version}")
        print(f"  Build string: {_build.string}")

        _variant.run_build(
            progress_callback=None,
            keep_build=False,
            output_dir=_output_tmpdir,
            recipe_path=_recipe_path,
        )
        print("  ✅ Build complete!")

    print("\n" + "=" * 60)
    print("🎉 All builds completed successfully!")
    print(f"\n📦 Built packages are available in: {_output_tmpdir}")
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
    - **Building**: Use `variant.run_build()` to build conda packages

    Next, explore the other notebooks to learn about:
    - Advanced Jinja templating and conditional variants
    - Multi-output recipes and staging caches
    """)
    return


if __name__ == "__main__":
    app.run()
