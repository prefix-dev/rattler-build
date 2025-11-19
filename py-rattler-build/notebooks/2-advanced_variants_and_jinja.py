"""
Educational notebook: Advanced Variants and Jinja Templating

This notebook covers advanced features of rattler-build:
- Complex variant configurations with zip_keys
- JinjaConfig for platform-specific builds
- Conditional requirements using platform selectors
- Custom context variables in templates
- Inspecting variant usage in rendered recipes
"""

import marimo

__generated_with = "0.17.6"
app = marimo.App(width="medium")


@app.cell(hide_code=True)
def _(mo):
    mo.md(r"""
    # 🔧 Advanced Variants and Jinja Templating

    This notebook explores advanced features for recipe rendering:

    1. Complex variant synchronization with `zip_keys`
    2. JinjaConfig for platform-specific rendering
    3. Conditional requirements based on platform
    4. Custom context variables in Jinja templates
    5. Multiple zipped variant groups
    6. Inspecting which variants were actually used

    Let's dive in!
    """)
    return


@app.cell
def _():
    import json
    import pprint

    import marimo as mo

    from rattler_build import JinjaConfig
    from rattler_build.render import RenderConfig
    from rattler_build.stage0 import Recipe
    from rattler_build.tool_config import PlatformConfig
    from rattler_build.variant_config import VariantConfig

    return (
        JinjaConfig,
        PlatformConfig,
        Recipe,
        RenderConfig,
        VariantConfig,
        json,
        mo,
        pprint,
    )


@app.cell(hide_code=True)
def _(mo):
    mo.md(r"""
    ## Example 1: Zip Keys - Synchronizing Multiple Variants

    When building packages, you often need to pair specific versions together. For example, certain Python versions might need to be paired with specific NumPy versions for compatibility.

    Without `zip_keys`: 3 python × 3 numpy = 9 combinations
    With `zip_keys`: 3 paired combinations
    """)
    return


@app.cell
def _(VariantConfig, pprint):
    # Create variants that should be paired
    synced_variants_without_zip = VariantConfig({"python": ["3.9", "3.10", "3.11"], "numpy": ["1.21", "1.23", "1.24"]})

    print("❌ WITHOUT zip_keys (Cartesian product)")
    print("=" * 60)
    combinations_before = synced_variants_without_zip.combinations()
    print(f"Total combinations: {len(combinations_before)}")
    pprint.pprint(combinations_before[:6])  # Show first 6
    print("...")

    # Now synchronize python and numpy
    synced_variants = VariantConfig(
        {"python": ["3.9", "3.10", "3.11"], "numpy": ["1.21", "1.23", "1.24"]}, zip_keys=[["python", "numpy"]]
    )

    print("\n✅ WITH zip_keys (synchronized)")
    print("=" * 60)
    combinations_after = synced_variants.combinations()
    print(f"Total combinations: {len(combinations_after)}")
    pprint.pprint(combinations_after)

    print("\n🔍 Explanation:")
    print("  python[0]=3.9  pairs with numpy[0]=1.21")
    print("  python[1]=3.10 pairs with numpy[1]=1.23")
    print("  python[2]=3.11 pairs with numpy[2]=1.24")
    return


@app.cell(hide_code=True)
def _(mo):
    mo.md(r"""
    ## Example 2: Multiple Zip Groups

    You can have multiple independent zip groups. For example, pair Python with NumPy, AND pair C/C++ compilers:
    """)
    return


@app.cell
def _(VariantConfig, pprint):
    # Create variants with two independent pairing groups
    multi_zip = VariantConfig(
        {
            "python": ["3.9", "3.10", "3.11"],
            "numpy": ["1.21", "1.23", "1.24"],
            "c_compiler": ["gcc", "clang", "msvc"],
            "cxx_compiler": ["g++", "clang++", "msvc"],
        },
        zip_keys=[["python", "numpy"], ["c_compiler", "cxx_compiler"]],
    )

    print("🔗 Multiple Zip Groups")
    print("=" * 60)
    print("Group 1: python ↔ numpy")
    print("Group 2: c_compiler ↔ cxx_compiler")
    print(f"\nTotal combinations: {len(multi_zip.combinations())}")
    print("(3 python/numpy pairs × 3 compiler pairs = 9 total)")
    print("\nAll combinations:")
    pprint.pprint(multi_zip.combinations())
    return


@app.cell(hide_code=True)
def _(mo):
    mo.md(r"""
    ## Example 3: JinjaConfig - Platform-Specific Rendering

    JinjaConfig allows you to specify the target platform for conditional rendering. This is useful for platform-specific dependencies and build configurations.
    """)
    return


@app.cell
def _(JinjaConfig, PlatformConfig, Recipe, RenderConfig, VariantConfig):
    # Recipe with platform selectors
    platform_recipe_yaml = """
    context:
      name: cross-platform-pkg
      version: "1.0.0"

    package:
      name: ${{ name }}
      version: ${{ version }}

    build:
      number: 0

    requirements:
      build:
        - ${{ compiler('c') }}
      host:
        - python
        # Platform-specific dependencies
        - if: unix
          then:
            - readline
        - if: win
          then:
            - m2w64-toolchain
        - if: linux
          then:
            - libffi
      run:
        - python
        - if: osx
          then:
            - libomp
    """

    platform_recipe = Recipe.from_yaml(platform_recipe_yaml)
    variant_cfg = VariantConfig()

    # Render for Linux
    print("🐧 Rendering for LINUX")
    print("=" * 60)
    linux_platform_config = PlatformConfig("linux-64")
    linux_render = RenderConfig(platform=linux_platform_config)
    linux_result = platform_recipe.render(variant_cfg, linux_render)
    linux_stage1 = linux_result[0].recipe()

    print(f"Host requirements: {linux_stage1.requirements.host}")
    print(f"Run requirements: {linux_stage1.requirements.run}")
    print("Note: Includes 'readline' (unix) and 'libffi' (linux)")

    # Render for macOS
    print("\n🍎 Rendering for macOS")
    print("=" * 60)
    macos_platform_config = PlatformConfig("osx-arm64")
    macos_render = RenderConfig(platform=macos_platform_config)
    macos_result = platform_recipe.render(variant_cfg, macos_render)
    macos_stage1 = macos_result[0].recipe()

    print(f"Host requirements: {macos_stage1.requirements.host}")
    print(f"Run requirements: {macos_stage1.requirements.run}")
    print("Note: Includes 'readline' (unix) and 'libomp' (osx)")

    # Render for Windows
    print("\n🪟 Rendering for WINDOWS")
    print("=" * 60)
    windows_platform_config = PlatformConfig("win-64")
    windows_render = RenderConfig(platform=windows_platform_config)
    windows_result = platform_recipe.render(variant_cfg, windows_render)
    windows_stage1 = windows_result[0].recipe()

    print(f"Host requirements: {windows_stage1.requirements.host}")
    print(f"Run requirements: {windows_stage1.requirements.run}")
    print("Note: Includes 'm2w64-toolchain' (win)")
    return


@app.cell(hide_code=True)
def _(mo):
    mo.md(r"""
    ## Example 4: Custom Context Variables in Templates

    RenderConfig allows you to add custom context variables that can be used in Jinja templates. This is powerful for parameterizing recipes:
    """)
    return


@app.cell
def _(Recipe, RenderConfig, VariantConfig, json):
    # Recipe using custom context variables
    custom_context_yaml = """
    context:
      name: custom-pkg
      version: "1.0.0"

    package:
      name: ${{ name }}-${{ custom_suffix }}
      version: ${{ version }}

    build:
      number: ${{ build_number }}
      string: ${{ build_string }}

    requirements:
      host:
        - python
      run:
        - python

    about:
      summary: Built on ${{ build_date }} by ${{ builder_name }}
      description: |
        Build configuration: ${{ build_config }}
        Debug mode: ${{ debug_mode }}
    """

    custom_recipe = Recipe.from_yaml(custom_context_yaml)
    custom_variants = VariantConfig()

    # Create render config with custom variables
    custom_render = RenderConfig(
        extra_context={
            "custom_suffix": "special",
            "build_number": 42,
            "build_string": "custom_abc123",
            "build_date": "2024-01-15",
            "builder_name": "CI Pipeline",
            "build_config": "release",
            "debug_mode": False,
        }
    )

    print("🎨 Custom Context Variables")
    print("=" * 60)
    print("Context variables set:")
    print(json.dumps(custom_render.get_all_context(), indent=2))

    custom_result = custom_recipe.render(custom_variants, custom_render)
    custom_stage1 = custom_result[0].recipe()

    print("\n📦 Rendered Recipe")
    print("=" * 60)
    print(f"Package name: {custom_stage1.package.name}")
    print(f"Build number: {custom_stage1.build.number}")
    print(f"Build string: {custom_stage1.build.string}")
    print(f"Summary: {custom_stage1.about.summary}")
    print(f"Description: {custom_stage1.about.description}")
    return


@app.cell(hide_code=True)
def _(mo):
    mo.md(r"""
    ## Example 5: Complex Variant Scenario - Real World Example

    Let's build a more realistic scenario: building a Python package that needs to work with different Python versions, NumPy versions, and platforms:
    """)
    return


@app.cell
def _(PlatformConfig, Recipe, RenderConfig, VariantConfig):
    # Realistic recipe with multiple variants
    realistic_yaml = """
    context:
      name: scipy-like
      version: "1.10.0"

    package:
      name: ${{ name }}
      version: ${{ version }}

    build:
      number: 0
      skip:
        - if: python_impl == "pypy"
          then: True

    requirements:
      build:
        - ${{ compiler('c') }}
        - ${{ compiler('cxx') }}
      host:
        - python ${{ python }}.*
        - numpy ${{ numpy }}.*
        - pip
        - if: unix
          then:
            - libblas
            - liblapack
      run:
        - python
        - numpy >=${{ numpy }}

    about:
      homepage: https://example.com/scipy-like
      license: BSD-3-Clause
      summary: A realistic scientific Python package
    """

    realistic_recipe = Recipe.from_yaml(realistic_yaml)

    # Create variant config with synced python/numpy
    realistic_variants = VariantConfig(
        {"python": ["3.9", "3.10", "3.11"], "numpy": ["1.21", "1.23", "1.24"], "python_impl": ["cpython"]},
        zip_keys=[["python", "numpy"]],
    )

    # Render for Linux
    realistic_platform_config = PlatformConfig("linux-64")
    realistic_render = RenderConfig(platform=realistic_platform_config)
    realistic_results = realistic_recipe.render(realistic_variants, realistic_render)

    print(f"🔬 Realistic Package Build: {len(realistic_results)} variant(s)")
    print("=" * 60)

    for _idx, _result in enumerate(realistic_results, 1):
        _variant = _result.variant()
        _stage1 = _result.recipe()

        print(f"\n📦 Variant {_idx}:")
        print(f"  Python: {_variant['python']}")
        print(f"  NumPy: {_variant['numpy']}")
        print(f"  Package: {_stage1.package.name} {_stage1.package.version}")
        print(f"  Host requirements: {_stage1.requirements.host}")
        print(f"  Run requirements: {_stage1.requirements.run}")

    print("\n✅ All variants rendered successfully!")
    return


@app.cell(hide_code=True)
def _(mo):
    mo.md(r"""
    ## Example 6: Inspecting Variant Usage in Stage1

    After rendering, Stage1 recipes contain information about which variant values were actually used during the rendering process:
    """)
    return


@app.cell
def _(Recipe, RenderConfig, VariantConfig, json):
    # Recipe that uses variants in multiple places
    inspect_yaml = """
    context:
      name: variant-inspector
      version: "1.0.0"

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

    inspect_recipe = Recipe.from_yaml(inspect_yaml)

    inspect_variants = VariantConfig(
        {"python": ["3.10", "3.11"], "numpy": ["1.23", "1.24"], "build_number": ["0"]}, zip_keys=[["python", "numpy"]]
    )

    inspect_render = RenderConfig()
    inspect_results = inspect_recipe.render(inspect_variants, inspect_render)

    print("🔍 Variant Usage Inspector")
    print("=" * 60)

    for _i, _rendered in enumerate(inspect_results, 1):
        # Get the variant dict
        _variant_used = _rendered.variant()

        # Get Stage1 recipe
        _stage1 = _rendered.recipe()

        print(f"\n📋 Variant {_i}:")
        print("  Variant values used during rendering:")
        print(json.dumps(_variant_used, indent=4))

        print("\n  Results of rendering:")
        print(f"    Package name: {_stage1.package.name}")
        print(f"    Build string: {_stage1.build.string}")
        print(f"    Host deps: {_stage1.requirements.host}")

        print("\n  Stage1 used_variant field:")
        print(json.dumps(_stage1.used_variant, indent=4))

        print("\n  Context variables:")
        print(json.dumps(_stage1.context, indent=4))
    return


@app.cell(hide_code=True)
def _(mo):
    mo.md(r"""
    ## Summary

    In this notebook, you learned advanced variant and Jinja features:

    - **Zip Keys**: Synchronize variants using `zip_keys` to pair specific combinations
    - **Multiple Zip Groups**: Create independent pairing groups (e.g., python↔numpy, c_compiler↔cxx_compiler)
    - **JinjaConfig**: Control platform-specific rendering with conditional selectors
    - **Custom Context**: Add variables via `RenderConfig(extra_context={...})` for use in templates
    - **Complex Variants**: Build realistic multi-variant packages with proper synchronization
    - **Variant Inspection**: Examine which variants were used via `variant.variant()` and `stage1.used_variant`

    Next steps:
    - Explore multi-output recipes and staging caches in the next notebook
    - Try building your own recipes with complex variant configurations
    """)
    return


if __name__ == "__main__":
    app.run()
