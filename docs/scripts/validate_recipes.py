"""Validate all recipe snippets render correctly with py-rattler-build."""

import sys
from pathlib import Path

from rattler_build import Stage0Recipe, VariantConfig

recipes_dir = Path("docs/snippets/recipes")
# recipes that need a variant configuration have a file with the same name in
# the `variants` subdirectory
variants_dir = recipes_dir / "variants"
failed = []

for recipe_path in sorted(recipes_dir.glob("*.yaml")):
    try:
        recipe = Stage0Recipe.from_file(recipe_path)
        variants_path = variants_dir / recipe_path.name
        variant_config = VariantConfig.from_file(variants_path) if variants_path.exists() else None
        recipe.render(variant_config)
        print(f"  OK:   {recipe_path}")
    except Exception as e:
        failed.append((recipe_path, str(e)))
        print(f"  FAIL: {recipe_path}")

if failed:
    print(f"\n{len(failed)} recipe(s) failed validation:")
    for recipe_path, err in failed:
        print(f"\n--- {recipe_path} ---")
        print(err)
    sys.exit(1)

print(f"\nAll {len(list(recipes_dir.glob('*.yaml')))} recipes validated successfully.")
