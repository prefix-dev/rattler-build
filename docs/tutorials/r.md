# Packaging a R (CRAN) package

Packaging a R package is similar to packaging a Python package!

## Generating a starting point

You can use rattler-build to generate a starting point for your recipe from the metadata on CRAN.

```bash
rattler-build generate-recipe cran r-knitr
```

## Building a R Package

```yaml title="recipe.yaml"
context:
  version: "1.47"

package:
  name: r-knitr
  version: ${{ version }}
  noarch: generic  # (4)!

source:
- url: https://cran.r-project.org/src/contrib/Archive/knitr/knitr_${{ version }}.tar.gz
  sha256: fadd849bf94a4e02520088a6626577c3c636227fe11c5cd7e8fcc5d51a7aa6cf

build:
  script: R CMD INSTALL --build .  # (1)!

requirements:
  host:
  - r-base  # (2)!
  - r-evaluate >=0.15
  - r-highr >=0.11
  - r-xfun >=0.44
  - r-yaml >=2.1.19
  run:
  - r-base
  - r-evaluate >=0.15
  - r-highr >=0.11
  - r-xfun >=0.44
  - r-yaml >=2.1.19

tests:
# This is a shorthand test for R packages to ensure that the library loads correctly.
- r:
    libraries:
      - knitr
# You can also run arbitrary R code in the test section.
- script: test_package.R  # (3)!

about:
  homepage: https://yihui.org/knitr/
  summary: A General-Purpose Package for Dynamic Report Generation in R
  description: |-
    Provides a general-purpose tool for dynamic report
    generation in R using Literate Programming techniques.
  license: GPL-2.0
  repository: https://github.com/cran/knitr
```

1. The `script` section is where you specify the build commands to run. In this case, we are using `R CMD INSTALL --build .` to build the package.
2. The `r-base` package is required to run R and is specified in the `host` requirements.
3. The `script` key automatically detects the language based on the file extension. In the case of `.R`, it will execute the R script with `rscript`.
4. The `noarch: generic` directive indicates that the package is architecture-independent. This is useful for R packages that do not contain compiled code and can run on any architecture. It allows the package to be installed on any platform without needing to rebuild it for each architecture.
