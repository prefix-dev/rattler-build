# Generating recipes for different ecosystems

Rattler-build has some builtin functionality to generate recipes for different (existing) ecosystems.

Currently we support the following ecosystems:

- `pypi` (Python) - generates a recipe for a Python package
- `cran` (R) - generates a recipe for an R package

To generate a recipe for a Python package, you can use the following command:

```sh
rattler-build generate-recipe pypi jinja2
```

This will generate a recipe for the `jinja2` package from PyPI and print it to the console. To turn it into a recipe, you can either pipe the stdout to a file or use the `-w` flag. The `-w` flag will create a new folder with the recipe in it.

The PyPI recipe generation supports additional flags:

- `-w/--write` write the recipe to a folder
- `-m/--use-mapping` use the conda-forge PyPI name mapping (defaults to true)
- `-t/--tree` generate recipes for all dependencies
- `--pypi-index-url` specify one or more PyPI index URLs to use for recipe generation (comma-separated)

The `--pypi-index-url` option allows you to use alternative PyPI mirrors or private PyPI repositories. You can specify multiple URLs, and the system will try each in order until one succeeds. This is especially useful for organizations with private packages or in environments with limited internet access. You can also set the `RATTLER_BUILD_PYPI_INDEX_URL` environment variable.

```sh
# Use a custom PyPI index
rattler-build generate-recipe pypi --pypi-index-url https://my-custom-pypi.example.com/pypi my-package

# Use multiple PyPI indexes (will try each in order)
rattler-build generate-recipe pypi --pypi-index-url https://my-custom-pypi.example.com/pypi,https://pypi.org/pypi my-package
```

The generated recipe for `jinja2` will look something like:

```yaml title="recipe.yaml"
--8<-- "docs/snippets/recipes/jinja2-generated.yaml"
```

## Generating recipes for R packages

To generate a recipe for an R package, you can use the following command:

```sh
rattler-build generate-recipe cran dplyr
```

The `R` recipe generation supports some additional flags:

- `-u/--universe` select an R universe to use (e.g. `bioconductor`)
- `-t/--tree` generate multiple recipes, for every dependency as well

R packages will be prefixed with `r-` to avoid name conflicts with Python packages. The generated recipe for `dplyr` will look something like:

```yaml title="recipe.yaml"
--8<-- "docs/snippets/recipes/r-dplyr-generated.yaml"
```

!!!tip

    You can use the generated recipes to build your own "forge" with `rattler-build`. Read more about it in the [Building your own forge](./tips_and_tricks.md#building-your-own-forge) section.
