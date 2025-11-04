import marimo

__generated_with = "0.17.6"
app = marimo.App()


@app.cell(hide_code=True)
def _(mo):
    mo.md(r"""
    # Multi-output Recipe Rendering Exploration

    This notebook explores the `rattler_build` Python API for rendering recipes with variants.

    ## Key concepts:

    1. **Stage0 Recipe**: The raw recipe with Jinja templates (use `rattler_build.stage0.Recipe.from_yaml()`)
    2. **VariantConfig**: Configuration for variants like python versions, build matrices
    3. **Rendering**: Convert Stage0 → Stage1 with `render_recipe()` which evaluates Jinja and creates variants
    4. **Stage1 Recipe**: Fully evaluated recipe ready for building

    Let's explore multi-output recipes and variant rendering!
    """)
    return


@app.cell
def _():
    # Import the necessary modules
    import pprint
    from pathlib import Path

    from rattler_build.render import RenderConfig, render_recipe
    from rattler_build.stage0 import MultiOutputRecipe, Recipe, SingleOutputRecipe
    from rattler_build.variant_config import VariantConfig

    print("✅ Imported stage0, variant_config, and render modules")
    print("\nKey classes:")
    print("- Recipe (stage0): Base recipe class")
    print("- SingleOutputRecipe, MultiOutputRecipe: Specific recipe types")
    print("- VariantConfig: Variant configuration")
    print("- RenderConfig: Rendering configuration")
    print("- render_recipe(): Main rendering function")
    print("- RenderedVariant: Result of rendering")
    return (
        MultiOutputRecipe,
        Path,
        Recipe,
        RenderConfig,
        SingleOutputRecipe,
        VariantConfig,
        pprint,
        render_recipe,
    )


@app.cell
def _(MultiOutputRecipe, Recipe, SingleOutputRecipe, pprint):
    # Example 1: Load and inspect a simple single-output recipe (Stage0)
    simple_recipe_yaml = """
    package:
      name: my-simple-package
      version: 1.0.0

    build:
      number: 0

    requirements:
      host:
        - python >=3.8
      run:
        - python >=3.8

    about:
      summary: A simple test package
      license: MIT
    """

    print("=" * 60)
    print("EXAMPLE 1: Single-output recipe (Stage0)")
    print("=" * 60)

    # Parse the recipe - this creates a Stage0 recipe (with Jinja unexpanded)
    recipe = Recipe.from_yaml(simple_recipe_yaml)

    print(f"\nRecipe type: {type(recipe)}")
    print(f"Is single output? {isinstance(recipe, SingleOutputRecipe)}")
    print(f"Is multi output? {isinstance(recipe, MultiOutputRecipe)}")

    # Access Stage0 recipe properties
    print(f"\nPackage name: {recipe.package.to_dict()}")
    print(f"Build info: {recipe.build.to_dict()['number']}")
    print(f"Context: {recipe.context}")

    print("\n📦 Full recipe dict (Stage0):")
    pprint.pprint(recipe.to_dict(), depth=2)
    return (simple_recipe_yaml,)


@app.cell
def _(MultiOutputRecipe, Recipe):
    # Example 2: Multi-output recipe (Stage0)
    multi_output_yaml = '\nschema_version: 1\n\ncontext:\n  name: my-multi-pkg\n  version: "2.5.0"\n\nrecipe:\n  version: ${{ version }}\n\nbuild:\n  number: 0\n\noutputs:\n  - package:\n      name: ${{ name }}-lib\n    build:\n      noarch: generic\n    requirements:\n      run:\n        - libfoo\n    about:\n      summary: Library package\n\n  - package:\n      name: ${{ name }}-tools\n    build:\n      noarch: generic\n    requirements:\n      run:\n        - ${{ name }}-lib\n        - python\n    about:\n      summary: Tools package\n'
    print("=" * 60)
    print("EXAMPLE 2: Multi-output recipe (Stage0)")
    print("=" * 60)
    multi_recipe = Recipe.from_yaml(multi_output_yaml)
    print(f"\nRecipe type: {type(multi_recipe)}")
    print(f"Is multi output? {isinstance(multi_recipe, MultiOutputRecipe)}")
    print(f"\nRecipe metadata: {multi_recipe.recipe}")
    print(f"Number of outputs: {len(multi_recipe.outputs)}")
    print(f"\nContext variables: {multi_recipe.context}")
    print("\n📦 Output names (still with Jinja templates at Stage0):")
    for _i, output in enumerate(multi_recipe.outputs):
        print(f"  {_i + 1}. {output.package.to_dict()}")
    print("\n📦 Full multi-output recipe dict (Stage0, truncated):")
    import json

    full_dict = json.dumps(multi_recipe.to_dict(), indent=2)
    # For multi-output, access via .recipe, .outputs, etc
    print(full_dict[:1500] + "..." if len(full_dict) > 1500 else full_dict)
    return (multi_recipe,)


@app.cell
def _(Recipe, VariantConfig, multi_recipe, render_recipe, simple_recipe_yaml):
    # Example 3: Render recipes (Stage0 → Stage1)
    print("=" * 60)
    print("EXAMPLE 3: Rendering - Stage0 → Stage1")
    print("=" * 60)
    variant_config = VariantConfig()
    # Create an empty variant config (no variants)
    print("\n🔧 Rendering single-output recipe from Example 1...")
    simple_recipe = Recipe.from_yaml(simple_recipe_yaml)
    # First, render the simple single-output recipe from Example 1
    simple_rendered = render_recipe(simple_recipe, variant_config)
    print(f"✨ Rendered {len(simple_rendered)} variant(s) for single-output recipe")
    if simple_rendered:
        _variant = simple_rendered[0]
        print(f"\nType: {type(_variant)}")
        stage1_recipe = _variant.recipe()
        # Explore the first rendered variant
        print(f"Package name: {stage1_recipe.package.name}")
        print(f"Package version: {stage1_recipe.package.version}")
        variant_dict = _variant.variant()
        print(f"Variant values: {variant_dict}")
        print("\n📦 Stage1 recipe (excerpt):")  # Get the Stage1 recipe (fully evaluated)
        stage1_dict = stage1_recipe.to_dict()
        print(f"  package: {stage1_dict['package']}")
        print(f"  build.number: {stage1_dict['build']['number']}")
        print(f"  requirements: {stage1_dict.get('requirements', {})}")
    print("\n\n🔧 Rendering multi-output recipe from Example 2...")  # Get variant info (build matrix values)
    rendered_variants = render_recipe(multi_recipe, variant_config)
    print(f"✨ Rendered {len(rendered_variants)} variant(s) for multi-output recipe")
    for _i, _variant in enumerate(rendered_variants):
        print(f"\n--- Output {_i + 1} ---")
        stage1_recipe = _variant.recipe()
        print(f"Package name: {stage1_recipe.package.name}")
        print(f"Package version: {stage1_recipe.package.version}")
        hash_info = _variant.hash_info()
        if hash_info:
            # Now render the multi-output recipe from Example 2
            print(f"Hash: {hash_info.hash[:16]}..., Prefix: {hash_info.prefix}")
        pins = _variant.pin_subpackages()
        if pins:
            print(f"Pin subpackages: {list(pins.keys())}")
        # Explore each rendered variant (one per output)
        print(
            f"Requirements (run): {stage1_recipe.requirements.to_dict().get('run', [])}"
        )  # Get the Stage1 recipe (fully evaluated)  # Get hash info  # Get pin_subpackages
    return


@app.cell
def _(Recipe, VariantConfig, render_recipe):
    # Example 4: Rendering with variants (multiple Python versions)
    print("=" * 60)
    print("EXAMPLE 4: Rendering with variants (Python matrix)")
    print("=" * 60)
    variant_recipe_yaml = "\npackage:\n  name: py-test-package\n  version: 0.1.0\n\nrequirements:\n  host:\n    - python ${{ python }}.*\n  run:\n    - python\n\nbuild:\n  number: 0\n"
    # Create a recipe that uses Python variants
    variant_yaml = '\npython:\n  - "3.9"\n  - "3.10"\n  - "3.11"\n  - "3.12"\n'
    print("\n📝 Recipe with Python variant: ${{ python }}")
    print("📝 Variant config: python = [3.9, 3.10, 3.11, 3.12]")
    py_recipe = Recipe.from_yaml(variant_recipe_yaml)
    py_variant_config = VariantConfig.from_yaml(variant_yaml)
    py_rendered = render_recipe(py_recipe, py_variant_config)
    print(f"\n✨ Rendered {len(py_rendered)} variants!")
    for _i, _variant in enumerate(py_rendered):
        _stage1 = _variant.recipe()
        variant_vals = _variant.variant()
        print(f"\n--- Variant {_i + 1}: Python {variant_vals.get('python', 'N/A')} ---")
        print(f"Package: {_stage1.package.name} {_stage1.package.version}")
        print(f"Host requirements: {_stage1.requirements.to_dict().get('host', [])}")
        # Define variant configuration with multiple Python versions
        # Parse recipe and variants
        # Render - this will create 4 variants (one per Python version)
        # Inspect each variant
        print(f"Full variant dict: {variant_vals}")
    return (py_rendered,)


@app.cell
def _(Path, Recipe, RenderConfig, VariantConfig, render_recipe):
    # Example 5: Load recipe from file and use RenderConfig
    print("=" * 60)
    print("EXAMPLE 5: Load from file + custom RenderConfig")
    print("=" * 60)

    # Try to find recipe.yaml in parent directory
    recipe_path = Path("..") / "recipe.yaml"

    if recipe_path.exists():
        print(f"✅ Found recipe at: {recipe_path.resolve()}")

        # Read and parse
        recipe_text = recipe_path.read_text()
        file_recipe = Recipe.from_yaml(recipe_text)

        print(f"\nRecipe type: {type(file_recipe).__name__}")

        # Create custom render config
        render_config = RenderConfig(
            target_platform="linux-64", build_platform="linux-64", host_platform="linux-64", experimental=False
        )

        # You can also set context variables
        render_config.set_context("custom_var", "custom_value")
        print(f"\nRenderConfig platforms: target={render_config.target_platform}")
        print(f"Custom context: {render_config.get_all_context()}")

        # Render with the config
        variant_cfg = VariantConfig()
        rendered = render_recipe(file_recipe, variant_cfg, render_config)

        print(f"\n✨ Rendered {len(rendered)} variant(s) from file")

        # Show first variant
        if rendered:
            first = rendered[0].recipe()
            print(f"\nFirst variant package: {first.package.name} v{first.package.version}")
            print(f"Build number: {first.build.number}")
            if hasattr(first, "about") and first.about:
                about_dict = first.about.to_dict()
                print(f"License: {about_dict.get('license', 'N/A')}")
    else:
        print(f"❌ Recipe file not found at: {recipe_path.resolve()}")
        print("   Skipping file loading example.")
    return


@app.cell
def _(py_rendered):
    # Example 6: Deep dive into Stage1 recipe structure
    print("=" * 60)
    print("EXAMPLE 6: Exploring Stage1 Recipe Properties")
    print("=" * 60)
    if py_rendered:
        # Use one of the rendered recipes from earlier
        _stage1 = py_rendered[0].recipe()
        print(f"\n🔍 Stage1 Recipe: {_stage1.package.name}")
        print(f"   Type: {type(_stage1)}")
        print("\n📦 Package:")
        print(f"   name: {_stage1.package.name}")
        print(f"   version: {_stage1.package.version}")
        print("\n🔨 Build:")  # Package info
        _build_dict = _stage1.build.to_dict()
        print(f"   number: {_stage1.build.number}")
        print(f"   string: {_stage1.build.string}")
        print(f"   noarch: {_stage1.build.noarch}")
        print(f"   script: {_stage1.build.script}")  # Build info
        print("\n📋 Requirements:")
        req_dict = _stage1.requirements.to_dict()
        print(f"   build: {req_dict.get('build', [])}")
        print(f"   host: {req_dict.get('host', [])}")
        print(f"   run: {req_dict.get('run', [])}")
        if hasattr(_stage1, "about") and _stage1.about:
            print("\n📄 About:")
            about = _stage1.about  # Requirements
            print(f"   homepage: {about.homepage}")
            print(f"   license: {about.license}")
            print(f"   summary: {about.summary}")
            print(f"   description: {about.description}")
        print("\n🌐 Context (evaluated):")
        ctx = _stage1.context
        if ctx:  # About
            for key, value in sorted(ctx.items())[:10]:
                print(f"   {key}: {value}")
        print("\n🎯 Used variant:")
        used_variant = _stage1.used_variant
        if used_variant:
            for key, value in sorted(used_variant.items()):
                print(f"   {key}: {value}")
        print(f"\n📦 Sources: {len(_stage1.sources)}")
        print(f"📦 Staging caches: {len(_stage1.staging_caches)}")  # Context (evaluated context variables)
        if hasattr(_stage1, "inherits_from"):
            print(f"📦 Inherits from: {_stage1.inherits_from}")
        print("\n" + "=" * 60)
        print("✅ Exploration complete!")  # Show first 10
    else:
        print(
            "No rendered variants available from previous examples."
        )  # Used variant  # Sources and staging caches  # Inherits from (for multi-output)
    return


@app.cell
def _():
    import marimo as mo

    return (mo,)


if __name__ == "__main__":
    app.run()
