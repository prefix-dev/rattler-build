# Recipe Generation

Generate conda recipes from various package ecosystems.

These functions fetch package metadata from upstream repositories and generate
ready-to-use `Stage0Recipe` objects.

You can import the generation functions from `rattler_build`:

```python
from rattler_build import (
    generate_pypi_recipe,
    generate_cran_recipe,
    generate_cpan_recipe,
    generate_luarocks_recipe,
)
```

## `generate_pypi_recipe`

::: rattler_build.generate_pypi_recipe

## `generate_cran_recipe`

::: rattler_build.generate_cran_recipe

## `generate_cpan_recipe`

::: rattler_build.generate_cpan_recipe

## `generate_luarocks_recipe`

::: rattler_build.generate_luarocks_recipe
