#!/usr/bin/env python3
"""
Example: Building a recipe with progress reporting using Rich.

This example demonstrates how to use rattler-build's progress reporting
capabilities with the Rich library for beautiful terminal output.

Usage:
    python build_with_progress.py recipe.yaml

Requirements:
    pip install rich
"""

import sys
from pathlib import Path
import tempfile

# Import rattler_build components
from rattler_build.stage0 import Recipe
from rattler_build.variant_config import VariantConfig
from rattler_build.render import RenderConfig, render_recipe
from rattler_build.progress import RichProgressCallback, SimpleProgressCallback


def build_recipe_with_rich_progress(recipe_path: Path):
    """Build a recipe with Rich progress display.

    Args:
        recipe_path: Path to the recipe YAML file
    """
    print(f"🔍 Loading recipe from {recipe_path}")

    # Load the recipe
    recipe = Recipe.from_file(str(recipe_path))
    print(f"✅ Loaded recipe: {recipe.package.name} {recipe.package.version}")

    # Configure variant rendering
    variant_config = VariantConfig()
    # Set recipe_path so the build can find license files, etc.
    render_config = RenderConfig(recipe_path=str(recipe_path.parent))

    print("\n📋 Rendering recipe variants...")
    rendered_variants = render_recipe(recipe, variant_config, render_config)
    print(f"✅ Rendered {len(rendered_variants)} variant(s)")

    # Build each variant with progress reporting
    for i, variant in enumerate(rendered_variants, 1):
        print(f"\n🔨 Building variant {i}/{len(rendered_variants)}")
        stage1_recipe = variant.recipe()
        package = stage1_recipe.package
        build = stage1_recipe.build
        print(f"   Package: {package.name}")
        print(f"   Version: {package.version}")
        print(f"   Build string: {build.string}")

        # Use Rich progress callback for beautiful output
        # Set show_logs=True to see all log messages (recommended!)
        with RichProgressCallback(show_logs=True) as callback:
            print("\n" + "=" * 60)
            print("Starting build with progress reporting...")
            print("=" * 60 + "\n")

            # Build with real progress callbacks!
            import tempfile

            with tempfile.TemporaryDirectory() as tmpdir:
                variant.run_build(
                    progress_callback=callback, keep_build=False, output_dir=Path(tmpdir), recipe_path=recipe_path
                )

    print("\n✅ Build complete!")


def build_recipe_with_simple_progress(recipe_path: Path):
    """Build a recipe with simple console progress display.

    Args:
        recipe_path: Path to the recipe YAML file
    """
    print(f"🔍 Loading recipe from {recipe_path}")

    # Load the recipe
    recipe = Recipe.from_file(str(recipe_path))
    print(f"✅ Loaded recipe: {recipe.package.name} {recipe.package.version}")

    # Configure and render
    variant_config = VariantConfig()
    # Set recipe_path so the build can find license files, etc.
    render_config = RenderConfig(recipe_path=str(recipe_path.parent))

    print("\n📋 Rendering recipe variants...")
    rendered_variants = render_recipe(recipe, variant_config, render_config)
    print(f"✅ Rendered {len(rendered_variants)} variant(s)")

    # Build with simple callback
    callback = SimpleProgressCallback()

    for i, variant in enumerate(rendered_variants, 1):
        print(f"\n🔨 Building variant {i}/{len(rendered_variants)}")

        # Build with real progress callbacks!
        with tempfile.TemporaryDirectory() as tmpdir:
            variant.run_build(
                progress_callback=callback, keep_build=False, output_dir=Path(tmpdir), recipe_dir=recipe_path.parent
            )

    print("\n✅ Build complete!")


def main():
    """Main entry point."""
    if len(sys.argv) < 2:
        print("Usage: python build_with_progress.py <recipe.yaml> [--simple]")
        print("\nOptions:")
        print("  --simple    Use simple console output instead of Rich")
        print("\nExamples:")
        print("  python build_with_progress.py recipe.yaml")
        print("  python build_with_progress.py recipe.yaml --simple")
        sys.exit(1)

    recipe_path = Path(sys.argv[1])
    if not recipe_path.exists():
        print(f"Error: Recipe file not found: {recipe_path}")
        sys.exit(1)

    use_simple = "--simple" in sys.argv

    try:
        if use_simple:
            build_recipe_with_simple_progress(recipe_path)
        else:
            try:
                build_recipe_with_rich_progress(recipe_path)
            except ImportError as e:
                print(f"Rich library not available: {e}")
                print("Falling back to simple progress...")
                build_recipe_with_simple_progress(recipe_path)

    except Exception as e:
        print(f"❌ Error: {e}")
        import traceback

        traceback.print_exc()
        sys.exit(1)


if __name__ == "__main__":
    main()
