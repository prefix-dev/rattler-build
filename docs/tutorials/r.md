# Packaging a R (CRAN) package

Packaging a R package is similar to packaging a Python package!

## Generating a starting point

You can use rattler-build to generate a starting point for your recipe from the metadata on CRAN.

```bash
rattler-build generate-recipe cran r-knitr
```

## Building a R Package

```yaml title="recipe.yaml"
--8<-- "docs/snippets/recipes/r-knitr.yaml"
```

1. The `script` section is where you specify the build commands to run. In this case, we are using `R CMD INSTALL --build .` to build the package.
2. The `r-base` package is required to run R and is specified in the `host` requirements.
3. The `script` key automatically detects the language based on the file extension. In the case of `.R`, it will execute the R script with `rscript`.
4. The `noarch: generic` directive indicates that the package is architecture-independent. This is useful for R packages that do not contain compiled code and can run on any architecture. It allows the package to be installed on any platform without needing to rebuild it for each architecture.
