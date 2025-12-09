# Multi-Output Recipes and Staging Caches

This tutorial teaches you about advanced recipe structures:

1. Multi-output recipes - Build multiple packages from one source
2. Staging outputs - Create temporary build artifacts
3. Output inheritance - Reuse build configurations
4. Inspecting Stage1 staging caches
5. Variants with multi-output recipes
6. Complete build pipeline visualization

Let's get started!

```python exec="1" source="above" session="multi_output_and_staging"
import json
import shutil
import tempfile
from pathlib import Path

from rattler_build import (
    MultiOutputRecipe,
    PlatformConfig,
    RenderConfig,
    Stage0Recipe,
    VariantConfig,
)
```

## Example 1: Multi-Output with Inter-Output Dependencies

Multi-output recipes build multiple packages from one recipe. When one output needs another, you list it as a **host** (or build) **dependency**. The dependency package is installed into the build environment, and your build script can use its files.

In this example:
- `myproject-lib` creates a Python module
- `myproject-tools` depends on `myproject-lib` as a host dependency and reads that file during its build

<!--pytest-codeblocks:cont-->
```python exec="1" source="above" result="text" session="multi_output_and_staging"
# Multi-output recipe with inter-output dependency
multi_output_yaml = """
schema_version: 1

context:
  name: myproject
  version: "2.0.0"

recipe:
  version: ${{ version }}

outputs:
  # First output: The library
  - package:
      name: ${{ name }}-lib
    build:
      script:
        interpreter: python
        content: |
          import os
          from pathlib import Path

          prefix = Path(os.environ["PREFIX"])
          lib_dir = prefix / "lib" / "python"
          lib_dir.mkdir(parents=True, exist_ok=True)

          (lib_dir / "myproject_lib.py").write_text('VERSION = "2.0.0"')
          print(f"Created library at {lib_dir}")
    requirements:
      build:
        - python

  # Second output: Uses the library as a host dependency
  - package:
      name: ${{ name }}-tools
    build:
      script:
        interpreter: python
        content: |
          import os
          from pathlib import Path

          prefix = Path(os.environ["PREFIX"])

          # Read and print the lib file (installed as host dependency)
          lib_file = prefix / "lib" / "python" / "myproject_lib.py"
          print(f"Reading library from: {lib_file}")
          print(lib_file.read_text())
    requirements:
      build:
        - python
      host:
        - ${{ name }}-lib
"""

multi_recipe = Stage0Recipe.from_yaml(multi_output_yaml)

print("Multi-Output Recipe Loaded")
print("=" * 60)
print(f"Recipe type: {type(multi_recipe).__name__}")
print(f"Is multi-output: {isinstance(multi_recipe, MultiOutputRecipe)}")
print(f"Number of outputs: {len(multi_recipe.outputs)}")

print("\nOutputs:")
for idx, output in enumerate(multi_recipe.outputs, 1):
    print(f"  {idx}. {output.to_dict()['package']['name']}")

# Render the recipe
mo_variants = VariantConfig()
mo_render = RenderConfig()
mo_results = multi_recipe.render(mo_variants, mo_render)

print(f"\nRendered {len(mo_results)} package(s):")
print("=" * 60)

for rendered in mo_results:
    stage1 = rendered.recipe()
    print(f"\nPackage: {stage1.package.name} {stage1.package.version}")
    print(f"   Build script: {stage1.build.script}")
    print(f"   Run requirements: {stage1.requirements.run}")
```

<!--pytest-codeblocks:cont-->
```python exec="1" source="above" result="text" session="multi_output_and_staging"
# Create persistent temp directories (clean up from previous runs)
recipe_tmpdir = Path(tempfile.gettempdir()) / "rattler_build_multi_output"
output_tmpdir = Path(tempfile.gettempdir()) / "rattler_build_multi_output_output"

# Clean up from previous runs
if recipe_tmpdir.exists():
    shutil.rmtree(recipe_tmpdir)
if output_tmpdir.exists():
    shutil.rmtree(output_tmpdir)

# Create the directories
recipe_tmpdir.mkdir(parents=True)
output_tmpdir.mkdir(parents=True)

# Define dummy recipe path
recipe_path = recipe_tmpdir / "recipe.yaml"

# Build each variant
print("Building Example 1 packages...")
print("=" * 60)
print(f"Recipe directory: {recipe_tmpdir}")
print(f"Output directory: {output_tmpdir}")

for i, variant in enumerate(mo_results, 1):
    print(f"\nBuilding variant {i}/{len(mo_results)}")
    stage1_recipe = variant.recipe()
    package = stage1_recipe.package
    build = stage1_recipe.build

    print(f"  Package: {package.name}")
    print(f"  Version: {package.version}")
    print(f"  Build string: {build.string}")

    result = variant.run_build(
        output_dir=output_tmpdir,
        recipe_path=recipe_path,
    )

    # Display build result information
    print(f"  Build complete in {result.build_time:.2f}s!")
    print(f"  Package: {result.packages[0]}")
    if result.variant:
        print(f"  Variant: {result.variant}")

    # Display build log
    if result.log:
        print(f"  Build log: {len(result.log)} messages captured")

print("\n" + "=" * 60)
print("Example 1 builds completed successfully!")
print(f"\nBuilt packages are available in: {output_tmpdir}")
```

## Example 2: Staging Outputs - Shared Build Artifacts

Staging is different from regular dependencies (Example 1). A staging output runs its build script once, then **copies its files directly into each inheriting package's prefix**. Since these are "new" files in the prefix, they will be included in the final package.

Use the `files` field to select which subset of files to include from the staging prefix.

In this example:
- `shared-build` staging creates both `/lib/shared.py` AND `/bin/tool.py`
- `compiled-project-python` inherits and uses `files: [lib/**]` to only include lib files
- `compiled-project-cli` inherits and uses `files: [bin/**]` to only include bin files

<!--pytest-codeblocks:cont-->
```python exec="1" source="above" result="text" session="multi_output_and_staging"
# Recipe with staging output
staging_yaml = """
schema_version: 1

context:
  name: compiled-project
  version: "1.5.0"

recipe:
  version: ${{ version }}

outputs:
  # Staging output: Creates shared artifacts for multiple packages
  - staging:
      name: shared-build
    build:
      script:
        interpreter: python
        content: |
          import os
          from pathlib import Path

          prefix = Path(os.environ["PREFIX"])

          # Create lib files
          lib_dir = prefix / "lib"
          lib_dir.mkdir(parents=True, exist_ok=True)
          (lib_dir / "shared.py").write_text('SHARED_VERSION = "1.5.0"')
          print(f"Created shared library at {lib_dir}")

          # Create bin files
          bin_dir = prefix / "bin"
          bin_dir.mkdir(parents=True, exist_ok=True)
          (bin_dir / "tool.py").write_text('#!/usr/bin/env python\\nprint("CLI tool")')
          print(f"Created CLI tool at {bin_dir}")
    requirements:
      build:
        - python

  # Package output 1: Python bindings (inherits lib files from staging)
  - package:
      name: ${{ name }}-python
    inherit: shared-build
    build:
      files:
        - lib/**

  # Package output 2: CLI tool (inherits bin files from staging)
  - package:
      name: ${{ name }}-cli
    inherit: shared-build
    build:
      files:
        - bin/**
"""

staging_recipe = Stage0Recipe.from_yaml(staging_yaml)

print("Recipe with Staging Output")
print("=" * 60)
print(f"Total outputs defined: {len(staging_recipe.outputs)}")

print("\nOutput types:")
for idx, output in enumerate(staging_recipe.outputs, 1):
    output_dict = output.to_dict()
    if "staging" in output_dict:
        print(f"  {idx}. Staging: {output_dict['staging']['name']}")
    elif "package" in output_dict:
        pkg_name = output_dict["package"]["name"]
        inherits = output_dict.get("inherits_from", None)
        print(f"  {idx}. Package: {pkg_name}", end="")
        if inherits:
            print(f" (inherits from: {inherits})")
        else:
            print()

# Render the recipe
staging_variants = VariantConfig()
platform_config = PlatformConfig(experimental=True)  # Staging is still experimental
staging_render = RenderConfig(platform=platform_config)
staging_results = staging_recipe.render(staging_variants, staging_render)

print(f"\nRendered {len(staging_results)} package(s)")
print("(Staging outputs don't produce packages)")
print("=" * 60)

for rendered in staging_results:
    stage1 = rendered.recipe()
    print(f"\n{stage1.package.name} {stage1.package.version}")

    # Check for staging caches
    if stage1.staging_caches:
        print(f"   Uses {len(stage1.staging_caches)} staging cache(s):")
        for cache in stage1.staging_caches:
            print(f"     - {cache.name}")
            print(f"       Build script: {cache.build.script}")

    # Check inheritance
    if stage1.inherits_from:
        print(f"   Inherits from: {json.dumps(stage1.inherits_from, indent=6)}")
```

<!--pytest-codeblocks:cont-->
```python exec="1" source="above" result="text" session="multi_output_and_staging"
# Create persistent temp directories (clean up from previous runs)
staging_recipe_tmpdir = Path(tempfile.gettempdir()) / "rattler_build_staging"
staging_output_tmpdir = Path(tempfile.gettempdir()) / "rattler_build_staging_output"

# Clean up from previous runs
if staging_recipe_tmpdir.exists():
    shutil.rmtree(staging_recipe_tmpdir)
if staging_output_tmpdir.exists():
    shutil.rmtree(staging_output_tmpdir)

# Create the directories
staging_recipe_tmpdir.mkdir(parents=True)
staging_output_tmpdir.mkdir(parents=True)

# Define dummy recipe path
staging_recipe_path = staging_recipe_tmpdir / "recipe.yaml"

# Build each variant
print("Building Example 2 packages (with staging)...")
print("=" * 60)
print(f"Recipe directory: {staging_recipe_tmpdir}")
print(f"Output directory: {staging_output_tmpdir}")

for i, variant in enumerate(staging_results, 1):
    print(f"\nBuilding variant {i}/{len(staging_results)}")
    stage1_recipe = variant.recipe()
    package = stage1_recipe.package
    build = stage1_recipe.build

    print(f"  Package: {package.name}")
    print(f"  Version: {package.version}")
    print(f"  Build string: {build.string}")

    if stage1_recipe.staging_caches:
        print(f"  Staging caches: {[c.name for c in stage1_recipe.staging_caches]}")

    result = variant.run_build(
        output_dir=staging_output_tmpdir,
        recipe_path=staging_recipe_path,
    )

    # Display build result information
    print(f"  Build complete in {result.build_time:.2f}s!")
    print(f"  Package: {result.packages[0]}")
    if result.variant:
        print(f"  Variant: {result.variant}")

    # Display build log
    if result.log:
        print(f"  Build log: {len(result.log)} messages captured")

print("\n" + "=" * 60)
print("Example 2 builds completed successfully!")
print(f"\nBuilt packages are available in: {staging_output_tmpdir}")
```

## Summary

In this tutorial, you learned about multi-output recipes and staging:

- **Multi-Output Recipes**: Build multiple packages from one recipe using the `outputs` list
- **Staging Outputs**: Create temporary build artifacts with `staging:` that other packages can inherit
