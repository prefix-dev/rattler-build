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

from rattler_build.progress import RichProgressCallback
from rattler_build.render import RenderConfig

# Import rattler_build components
from rattler_build.stage0 import Recipe
from rattler_build.tool_config import PlatformConfig
from rattler_build.variant_config import VariantConfig


def build_recipe_with_rich_progress(recipe_path: Path) -> None:
    """Build a recipe with Rich progress display.

    Args:
        recipe_path: Path to the recipe YAML file
    """
    print(f"🔍 Loading recipe from {recipe_path}")

    # Load the recipe
    recipe = Recipe.from_file(recipe_path)

    # Configure variant rendering
    variant_config = VariantConfig()
    # Set recipe_path so the build can find license files, etc.
    # target_platform defaults to current platform if not specified
    platform_config = PlatformConfig(recipe_path=str(recipe_path))
    render_config = RenderConfig(platform=platform_config)

    print("\n📋 Rendering recipe variants...")
    rendered_variants = recipe.render(variant_config, render_config)
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
                result = variant.run_build(
                    progress_callback=callback, keep_build=False, output_dir=Path(tmpdir), recipe_path=recipe_path
                )

                # Display build result information
                print("\n" + "=" * 60)
                print("Build Result:")
                print("=" * 60)
                print(f"   Package: {result.name} {result.version}")
                print(f"   Build string: {result.build_string}")
                print(f"   Platform: {result.platform}")
                print(f"   Build time: {result.build_time:.2f}s")
                print("   Package files:")
                for pkg in result.packages:
                    print(f"     - {pkg}")
                if result.variant:
                    print(f"   Variant: {result.variant}")

    print("\n✅ Build complete!")


def main() -> None:
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

    try:
        build_recipe_with_rich_progress(recipe_path)

    except Exception as e:
        print(f"❌ Error: {e}")
        import traceback

        traceback.print_exc()
        sys.exit(1)


if __name__ == "__main__":
    main()
