# Recipe Rendering Basics

Welcome to the rattler-build Python bindings tutorial! This notebook will teach you how to:

1. Load recipes from YAML strings and Python dictionaries
2. Configure variants (different build configurations)
3. Render recipes to produce fully evaluated build specifications
4. Understand the difference between Stage0 (template) and Stage1 (evaluated) recipes

Let's get started!

```python exec="1" source="above" session="recipe_rendering_basics"
import json
import pprint
import tempfile
from pathlib import Path

import yaml

from rattler_build import (
    MultiOutputRecipe,
    PlatformConfig,
    RenderConfig,
    SingleOutputRecipe,
    Stage0Recipe,
    VariantConfig,
)
```

## Example 1: Loading a Simple Recipe from YAML

The most common way to define a recipe is using YAML format. Let's create a simple package recipe:

<!--pytest-codeblocks:cont-->
```python exec="1" source="above" result="text" session="recipe_rendering_basics"
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

# Parse the YAML into a Stage0Recipe object
simple_recipe = Stage0Recipe.from_yaml(simple_recipe_yaml)

print("Recipe loaded successfully!")
print(f"Type: {type(simple_recipe).__name__}")
print(f"Is single output: {isinstance(simple_recipe, SingleOutputRecipe)}")
print(f"Is multi output: {isinstance(simple_recipe, MultiOutputRecipe)}")
print("\nRecipe structure (as dict):")
print(json.dumps(simple_recipe.to_dict(), indent=2))
```

## Example 2: Creating a Recipe from a Python Dictionary

You can also create recipes from Python dictionaries. Let's verify that `Recipe.from_yaml()` and `Recipe.from_dict()` produce the same result when given the same data:

<!--pytest-codeblocks:cont-->
```python exec="1" source="above" result="text" session="recipe_rendering_basics"
# Parse the same YAML as a Python dictionary
recipe_dict = yaml.safe_load(simple_recipe_yaml)

# Create Stage0Recipe from dictionary
dict_recipe = Stage0Recipe.from_dict(recipe_dict)

print("Recipe created from dictionary!")

# Assert that both recipes are the same
yaml_dict = simple_recipe.to_dict()
dict_dict = dict_recipe.to_dict()
assert yaml_dict == dict_dict, "Recipes should be identical!"

print("\nBoth recipes are identical!")
```

## Example 3: Understanding VariantConfig with Zip Keys

Variants allow you to build the same package with different configurations (e.g., different Python versions, compilers, or dependencies). By default, VariantConfig creates all possible combinations (Cartesian product), but we can use `zip_keys` to pair specific variants together:

<!--pytest-codeblocks:cont-->
```python exec="1" source="above" result="text" session="recipe_rendering_basics"
# Create a VariantConfig from a dictionary
variant_dict = {
    "python": ["3.9", "3.10", "3.11"],
    "numpy": ["1.21", "1.22", "1.23"],
}
variant_config_without_zip = VariantConfig(variant_dict)

print("Variant Configuration")
print("=" * 60)
print(f"Variant keys: {variant_config_without_zip.keys()}")
print(f"Python versions: {variant_config_without_zip.get_values('python')}")
print(f"Numpy versions: {variant_config_without_zip.get_values('numpy')}")

print("\nWITHOUT zip_keys (Cartesian product):")
print(f"Total combinations: {len(variant_config_without_zip.combinations())} (3 x 3)")
print("\nAll possible combinations:")
pprint.pprint(variant_config_without_zip.combinations())

# Create a new VariantConfig with zip_keys (python and numpy zipped together by index)
variant_config = VariantConfig(variant_dict, zip_keys=[["python", "numpy"]])

print("\n" + "=" * 60)
print("WITH zip_keys (paired by index):")
print(f"Zip keys: {variant_config.zip_keys}")
print(f"Total combinations: {len(variant_config.combinations())} (paired)")
print("\nPaired combinations:")
pprint.pprint(variant_config.combinations())

print("\nVariant config as dict:")
print(json.dumps(variant_config.to_dict(), indent=2))
```

## Example 4: RenderConfig - Controlling the Build Environment

RenderConfig lets you specify the target platform and add custom context variables for recipe rendering:

<!--pytest-codeblocks:cont-->
```python exec="1" source="above" result="text" session="recipe_rendering_basics"
# Create a render config with custom settings
platform_config = PlatformConfig(
    target_platform="linux-64",
    build_platform="linux-64",
    host_platform="linux-64",
    experimental=False,
)
render_config = RenderConfig(
    platform=platform_config,
    extra_context={
        "custom_var": "custom_value",
        "build_timestamp": "2024-01-01",
        "my_number": 42,
    },
)

print("Render Configuration")
print("=" * 60)
print(f"Target platform: {render_config.target_platform}")
print(f"Build platform: {render_config.build_platform}")
print(f"Host platform: {render_config.host_platform}")
print(f"Experimental: {render_config.experimental}")
print("\nCustom context variables:")
print(json.dumps(render_config.get_all_context(), indent=2))
```

## Example 5: Rendering Recipe with Variants

Now let's put it all together! We'll use the recipe from Example 1, the variant config from Example 3, and the render config from Example 4 to render our recipe with multiple variants.

**Stage0** is the parsed recipe with Jinja templates still intact (e.g., `${{ python }}`).
**Stage1** is the fully evaluated recipe with all templates resolved to actual values.

<!--pytest-codeblocks:cont-->
```python exec="1" source="above" result="text" session="recipe_rendering_basics"
# Render the recipe with all the configurations we've created
rendered_variants = simple_recipe.render(variant_config, render_config)

print("STAGE 0 (Parsed, templates intact)")
print("=" * 60)
print(f"Package name (raw): {simple_recipe.package.name}")
print(f"Package version (raw): {simple_recipe.package.version}")
print(f"Host requirements (raw): {simple_recipe.requirements.host}")

print(f"\nRendered {len(rendered_variants)} variant(s)")
print("=" * 60)

for _i, _rendered_variant in enumerate(rendered_variants, 1):
    _variant_values = _rendered_variant.variant()
    _stage1_recipe = _rendered_variant.recipe()

    print(f"\nSTAGE 1 - Variant {_i} (Rendered, templates evaluated)")
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
print("Recipe rendering complete!")
```

## Example 6: Building the Package

Finally, let's actually build the package! We'll take the rendered variants and build them into conda packages:

<!--pytest-codeblocks:cont-->
```python exec="1" source="above" result="text" session="recipe_rendering_basics"
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
_recipe_tmpdir.mkdir(parents=True)
_output_tmpdir.mkdir(parents=True)

# Define dummy recipe path
_recipe_path = _recipe_tmpdir / "recipe.yaml"

# Build each variant
print("Building packages...")
print("=" * 60)
print(f"Recipe directory: {_recipe_tmpdir}")
print(f"Output directory: {_output_tmpdir}")

for _i, _variant in enumerate(rendered_variants, 1):
    print(f"\nBuilding variant {_i}/{len(rendered_variants)}")
    _stage1_recipe = _variant.recipe()
    _package = _stage1_recipe.package
    _build = _stage1_recipe.build

    print(f"  Package: {_package.name}")
    print(f"  Version: {_package.version}")
    print(f"  Build string: {_build.string}")

    _result = _variant.run_build(
        output_dir=_output_tmpdir,
        recipe_path=_recipe_path,
    )

    # Display build result information
    print(f"  Build complete in {_result.build_time:.2f}s!")
    print(f"  Package: {_result.packages[0]}")
    if _result.variant:
        print(f"  Variant: {_result.variant}")

    # Display build log
    if _result.log:
        print(f"  Build log: {len(_result.log)} messages captured")
        print("\n  Build log details:")
        for _log_entry in _result.log[:10]:  # Show first 10 log entries
            print(f"    {_log_entry}")
        if len(_result.log) > 10:
            print(f"    ... and {len(_result.log) - 10} more messages")

print("\n" + "=" * 60)
print("All builds completed successfully!")
print(f"\nBuilt packages are available in: {_output_tmpdir}")
```

## Summary

In this notebook, you learned:

- **Recipe Creation**: Load recipes from YAML strings (`Stage0Recipe.from_yaml()`) or Python dicts (`Stage0Recipe.from_dict()`)
- **VariantConfig**: Define build variants and use `zip_keys` to pair specific combinations
- **RenderConfig**: Configure target platforms and add custom context variables
- **Stage0 vs Stage1**: Understand the difference between parsed templates and evaluated recipes
- **Rendering**: Use `recipe.render()` to transform Stage0 -> Stage1 with variants
- **Building**: Use `variant.run_build()` to build conda packages, which returns a `BuildResult` with package paths, metadata, timing information, and captured build logs
