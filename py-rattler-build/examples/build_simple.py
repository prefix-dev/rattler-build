#!/usr/bin/env python3
"""
Example: Simple recipe building without progress bars.

This example demonstrates the basic workflow for building a recipe
using rattler-build without any progress reporting.

Usage:
    python build_simple.py recipe.yaml
"""

import sys
import tempfile
from pathlib import Path

from rattler_build.render import RenderConfig, render_recipe
from rattler_build.stage0 import Recipe
from rattler_build.variant_config import VariantConfig


def build_recipe(recipe_path: Path):
    """Build a recipe with simple console output.

    Args:
        recipe_path: Path to the recipe YAML file
    """
    print(f"Loading recipe from {recipe_path}")

    # Load the recipe
    recipe = Recipe.from_file(str(recipe_path))
    print(f"Loaded recipe: {recipe.package.name} {recipe.package.version}")

    # Configure variant rendering
    variant_config = VariantConfig()
    render_config = RenderConfig(recipe_path=str(recipe_path.parent))

    print("\nRendering recipe variants...")
    rendered_variants = render_recipe(recipe, variant_config, render_config)
    print(f"Rendered {len(rendered_variants)} variant(s)")

    # Build each variant
    for i, variant in enumerate(rendered_variants, 1):
        print(f"\nBuilding variant {i}/{len(rendered_variants)}")
        stage1_recipe = variant.recipe()
        package = stage1_recipe.package
        build = stage1_recipe.build
        print(f"  Package: {package.name}")
        print(f"  Version: {package.version}")
        print(f"  Build string: {build.string}")

        # Build the package
        with tempfile.TemporaryDirectory() as tmpdir:
            variant.run_build(
                progress_callback=None,
                keep_build=False,
                output_dir=Path(tmpdir),
                recipe_path=recipe_path,
            )

    print("\nBuild complete!")


def main():
    """Main entry point."""
    if len(sys.argv) < 2:
        print("Usage: python build_simple.py <recipe.yaml>")
        print("\nExample:")
        print("  python build_simple.py recipe.yaml")
        sys.exit(1)

    recipe_path = Path(sys.argv[1])
    if not recipe_path.exists():
        print(f"Error: Recipe file not found: {recipe_path}")
        sys.exit(1)

    try:
        build_recipe(recipe_path)
    except Exception as e:
        print(f"Error: {e}")
        import traceback

        traceback.print_exc()
        sys.exit(1)


if __name__ == "__main__":
    main()
