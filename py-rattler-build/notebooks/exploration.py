import marimo

__generated_with = "0.17.6"
app = marimo.App()


@app.cell(hide_code=True)
def _(mo):
    mo.md(r"""
    ## 1. Load a Recipe from a YAML String
    """)
    return


@app.cell
def _():
    from rattler_build.stage0 import Recipe
    from rattler_build.variant_config import VariantConfig
    from rattler_build.render import RenderConfig
    from rattler_build import JinjaConfig
    import json

    # Define a simple recipe as YAML
    recipe_yaml = """
    schema_version: 1

    context:
      version: "1.2.3"
      name: "my-package"
      template: ${{ name }}-${{ version }}

    package:
      name: ${{ name }}
      version: ${{ version }}

    build:
      number: 0
      script: echo "Building ${{ name }} version ${{ version }}"

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

    # Load the recipe
    recipe = Recipe.from_yaml(recipe_yaml)
    print(f"Recipe loaded: {recipe}")

    # NEW: Direct access to package without needing as_single_output()!
    print(f"Package name: {recipe.package.name}")
    print(f"Package version: {recipe.package.version}")

    # NEW: Direct access to context, about, build, requirements
    print(f"Context: {recipe.context}")
    print(f"Summary: {recipe.about.summary}")
    print(f"License: {recipe.about.license}")
    print(f"Build number: {recipe.build.number}")

    # NEW: Direct access to requirements fields
    print(f"Host requirements: {recipe.requirements.host}")
    print(f"Run requirements: {recipe.requirements.run}")
    return JinjaConfig, Recipe, RenderConfig, VariantConfig, json, recipe


@app.cell
def _(recipe):
    # Get a dictionary representation of the recipe
    recipe.to_dict()
    return


@app.cell(hide_code=True)
def _(mo):
    mo.md(r"""
    ## 2. Work with Variant Configurations

    Variants allow you to build multiple versions of a package with different dependencies.
    """)
    return


@app.cell
def _(VariantConfig):
    # Create a simple variant config
    variant_config = VariantConfig()
    variant_config.set_values("python", ["3.9", "3.10", "3.11"])
    variant_config.set_values("numpy", ["1.20", "1.21", "1.22"])

    print(f"Variant keys: {variant_config.keys()}")
    print(f"Python versions: {variant_config.get_values('python')}")
    print(f"Numpy versions: {variant_config.get_values('numpy')}")
    print(f"\nTotal combinations (without zip_keys): {len(variant_config.combinations())}")

    # Show all combinations
    print("\nAll combinations:")
    for i, combo in enumerate(variant_config.combinations(), 1):
        print(f"  {i}. python={combo['python']}, numpy={combo['numpy']}")
    return (variant_config,)


@app.cell
def _(variant_config):
    # Use zip_keys to synchronize variants
    variant_config.zip_keys = [["python", "numpy"]]
    print(f"With zip_keys: {len(variant_config.combinations())} combinations\n")
    print("Synchronized combinations:")
    for i_1, combo_1 in enumerate(variant_config.combinations(), 1):
        print(f"  {i_1}. python={combo_1['python']}, numpy={combo_1['numpy']}")
    return


@app.cell(hide_code=True)
def _(mo):
    mo.md(r"""
    ## 3. Load Variants from YAML
    """)
    return


@app.cell
def _(VariantConfig):
    # Create variant config from YAML
    variant_yaml = '\npython:\n  - "3.9"\n  - "3.10"\n  - "3.11"\nnumpy:\n  - "1.20"\n  - "1.21"\n  - "1.22"\nc_compiler:\n  - gcc\n  - clang\nzip_keys:\n  - [python, numpy]\n'
    config = VariantConfig.from_yaml(variant_yaml)
    print(f"Keys: {config.keys()}")
    print(f"Zip keys: {config.zip_keys}")
    print(f"Number of combinations: {len(config.combinations())}")
    print("\nCombinations:")
    for combo_2 in config.combinations():
        print(f"  python={combo_2['python']}, numpy={combo_2['numpy']}, c_compiler={combo_2['c_compiler']}")
    return


@app.cell(hide_code=True)
def _(mo):
    mo.md(r"""
    ## 4. Render a Recipe with Variants

    Now let's combine recipes and variants to see how they get rendered.
    """)
    return


@app.cell
def _(Recipe, RenderConfig, VariantConfig):
    # Create a recipe that uses variants
    recipe_with_variants_yaml = '\nschema_version: 1\n\ncontext:\n  version: "2.0.0"\n\npackage:\n  name: numpy-example\n  version: ${{ version }}\n\nbuild:\n  number: 0\n\nrequirements:\n  host:\n    - python ${{ python }}\n    - numpy ${{ numpy }}\n  run:\n    - python ${{ python }}\n    - numpy ${{ numpy }}\n\nabout:\n  summary: Example package that depends on specific Python and NumPy versions\n'
    recipe_1 = Recipe.from_yaml(recipe_with_variants_yaml)
    variant_config_1 = VariantConfig()
    variant_config_1.set_values("python", ["3.10.*", "3.11.*"])
    variant_config_1.set_values("numpy", ["1.23.*", "1.24.*"])
    variant_config_1.zip_keys = [["python", "numpy"]]
    render_config = RenderConfig()
    rendered_variants = recipe_1.render(variant_config_1, render_config)
    print(f"Generated {len(rendered_variants)} rendered variants:\n")
    for i_2, variant in enumerate(rendered_variants, 1):
        print(f"{i_2}. {variant.recipe().package.name}-{variant.recipe().package.version}")
        print(f"   Build string: {variant.recipe().build.string}")
        print(f"   Variant: {variant.variant()}")
        print()
    # Load recipe
    # Create variant config with just two Python versions to keep it simple
    # Render the recipe with the variants
    print(rendered_variants)
    return (rendered_variants,)


@app.cell(hide_code=True)
def _(mo):
    mo.md(r"""
    ## 5. Inspect Rendered Recipe Details

    Let's look at the details of one of the rendered variants.
    """)
    return


@app.cell
def _(rendered_variants):
    # Let's inspect the first rendered variant
    first_variant = rendered_variants[0]

    print("=== Rendered Variant Details ===\n")
    print(f"Name: {first_variant.recipe().package.name}")
    print(f"Version: {first_variant.recipe().package.version}")
    print(f"Build string: {first_variant.recipe().build.string}")
    print(f"Build number: {first_variant.recipe().build.number}")
    print("\nVariant configuration:")
    for key, value in first_variant.variant().items():
        print(f"  {key}: {value}")

    print("\nHost requirements:")
    for req in first_variant.recipe().requirements.host:
        print(f"  - {req}")

    print("\nRun requirements:")
    for req in first_variant.recipe().requirements.run:
        print(f"  - {req}")

    # Access the recipe as a Python object
    recipe_obj = first_variant.recipe()
    print("\nAbout section:")
    print(f"  Summary: {recipe_obj.about.summary}")
    return (first_variant,)


@app.cell(hide_code=True)
def _(mo):
    mo.md(r"""
    ## 6. Convert Recipe to Dictionary

    You can also convert recipes to dictionaries for easy inspection or serialization.
    """)
    return


@app.cell
def _(first_variant, json):
    # Convert the recipe to a dictionary
    recipe_dict = first_variant.recipe().to_dict()

    # Pretty print the dictionary
    print("Recipe as dictionary:")
    print(json.dumps(recipe_dict, indent=2))
    return


@app.cell(hide_code=True)
def _(mo):
    mo.md(r"""
    ## 7. Working with Conditional Variants

    You can use conditionals in variant configurations to set different values based on platform.
    """)
    return


@app.cell
def _(JinjaConfig, VariantConfig):
    # Create conditional variant YAML
    conditional_yaml = "\nc_compiler:\n  - if: unix\n    then: gcc\n  - if: win\n    then: msvc\ncxx_compiler:\n  - if: unix\n    then: gxx\n  - if: win\n    then: msvc\n"
    for platform in ["linux-64", "win-64", "osx-arm64"]:
        jinja_config = JinjaConfig(target_platform=platform)
        config_1 = VariantConfig.from_yaml_with_context(conditional_yaml, jinja_config)
        print(f"\nPlatform: {platform}")
        print(f"  c_compiler: {config_1.get_values('c_compiler')}")
        # Test on different platforms
        print(f"  cxx_compiler: {config_1.get_values('cxx_compiler')}")
    return


@app.cell(hide_code=True)
def _(mo):
    mo.md(r"""
    ## 8. Load Recipe from File

    Let's check if there are any example recipes in the workspace we can load.
    """)
    return


@app.cell
def _(Recipe):
    from pathlib import Path

    # Look for example recipes
    recipes_dir = Path("/Users/wolfv/Programs/rattler-build/py-rattler-build/tests/data/recipes")

    if recipes_dir.exists():
        recipe_files = list(recipes_dir.rglob("recipe.yaml"))
        print(f"Found {len(recipe_files)} recipe files:")
        for recipe_file in recipe_files[:5]:  # Show first 5
            print(f"  - {recipe_file.relative_to(recipes_dir.parent)}")

        # Load one if available
        if recipe_files:
            print(f"\nLoading first recipe: {recipe_files[0].name}")
            try:
                with open(recipe_files[0]) as f:
                    loaded_recipe = Recipe.from_yaml(f.read())

                print("Successfully loaded recipe!")

                print(f"Build number: {loaded_recipe.build.number}")
                print(f"Summary: {loaded_recipe.about.summary}")

            except Exception as e:
                print(f"Error loading recipe: {e}")
                import traceback

                traceback.print_exc()
    else:
        print("Recipe directory not found in expected location")
    return


@app.cell(hide_code=True)
def _(mo):
    mo.md(r"""
    ## 9. Create Recipe from Dictionary

    You can also create recipes programmatically from a Python dictionary.
    """)
    return


@app.cell
def _(Recipe):
    # Create a recipe from a dictionary
    recipe_dict_1 = {
        "schema_version": 1,
        "context": {"name": "my-tool", "version": "3.4.5", "jinja": "${{ name }}"},
        "package": {"name": "my-tool", "version": "3.4.5"},
        "build": {"number": 0, "script": "pip install ."},
        "requirements": {"host": ["python", "pip"], "run": ["python >=3.8"]},
        "about": {
            "homepage": "https://example.com",
            "license": "Apache-2.0",
            "summary": "A useful tool",
        },
        "tests": [],
    }
    recipe_from_dict = Recipe.from_dict(recipe_dict_1)
    print(f"Created recipe: {recipe_from_dict.package.name} v{recipe_from_dict.package.version}")
    print(f"Build number: {recipe_from_dict.build.number}")
    print(f"Host requirements: {recipe_from_dict.requirements.host}")
    print(f"Run requirements: {recipe_from_dict.requirements.run}")
    print(f"Summary: {recipe_from_dict.about.summary}")
    print(f"License: {recipe_from_dict.about.license}")
    # NEW: Direct field access - no need for as_single_output()!
    # NEW: Direct access to all nested fields
    print(f"Homepage: {recipe_from_dict.about.homepage}")
    return


app._unparsable_cell(
    r"""
    RenderedVariant.build(\"...\")

    # everything topologically sorted
    build([RenderedVariants...], options)
    """,
    name="_",
)


@app.cell
def _():
    import marimo as mo

    return (mo,)


if __name__ == "__main__":
    app.run()
