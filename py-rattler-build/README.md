# py-rattler-build

Python bindings for [rattler-build](https://github.com/prefix-dev/rattler-build), the fast conda package builder.

`py-rattler-build` lets you build, inspect, test, and upload conda packages
directly from Python — no subprocess calls needed.

## Quick start

### Build a package from a recipe

```python
from rattler_build import Stage0Recipe, VariantConfig

recipe = Stage0Recipe.from_file("recipe.yaml")
results = recipe.run_build()

for result in results:
    print(f"Built {result.name} {result.version} in {result.build_time:.1f}s")
    for pkg in result.packages:
        print(f"  {pkg}")
```

### Render and build with variants

```python
from rattler_build import Stage0Recipe, VariantConfig

recipe = Stage0Recipe.from_file("recipe.yaml")
rendered_variants = recipe.render(VariantConfig())

for variant in rendered_variants:
    result = variant.run_build()
```

### Inspect an already built package

```python
from rattler_build import Package

pkg = Package.from_file("mypackage-1.0.0-py312h0_0.conda")
print(f"{pkg.name} {pkg.version} ({pkg.platform})")
print(f"Dependencies: {pkg.depends}")
print(f"Files: {len(pkg.files)}")
```

### Test a package

```python
from rattler_build import Package

pkg = Package.from_file("mypackage-1.0.0-py312h0_0.conda")
results = pkg.run_tests()
```

### Generate a recipe from PyPI

```python
from rattler_build import generate_pypi_recipe

recipe_yaml = generate_pypi_recipe("requests", version="2.31.0")
print(recipe_yaml)
```

### Build with progress reporting

```python
from rattler_build import Stage0Recipe, VariantConfig
from rattler_build.progress import RichProgressCallback

recipe = Stage0Recipe.from_file("recipe.yaml")
rendered_variants = recipe.render(VariantConfig())

for variant in rendered_variants:
    with RichProgressCallback(show_logs=True) as callback:
        result = variant.run_build(progress_callback=callback)
```

## Documentation

For the full reference, check out our
[website](https://rattler-build.prefix.dev/latest/py-rattler-build/reference/).
