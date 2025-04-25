# Packaging an R Package and executing a script

This guide shows how to create recipes for R packages with scripts to execute them with rattler-build.

## Generating R Package Recipes

The easiest way to get started with R packages is to use the built-in recipe generator:

```bash
rattler-build generate-recipe cran package_name
```

For example, to generate a recipe for the "dplyr" package:

```bash
rattler-build generate-recipe cran dplyr
```

This will create a recipe.yaml file with all the necessary dependencies and configuration for building the R package.

```yaml title="recipe.yaml"
package:
  name: r-specsverification
  version: 0.1.0

source:
  url:
    - https://cran.r-project.org/src/contrib/SpecsVerification_0.5-3.tar.gz
    - https://cran.r-project.org/src/contrib/Archive/SpecsVerification/SpecsVerification_0.5-3.tar.gz
  sha256: 630fd876b51cb5e22061fa64dbb447c09e88c14e81fb801001ae18e969a4e6ec

build:
  number: 0
  script:
    interpreter: r
    content: |
      # Note, to install the package via source, we need to set the SRC_DIR environment variable
      install.packages(Sys.getenv("SRC_DIR"), repos=NULL, type="source")

requirements:
  build:
    - if: unix
      then:
        - ${{ compiler('c') }}
        - ${{ compiler('cxx') }}
      else:
        - mingwpy
        - ucrt
        - m2-filesystem
        - m2-sed
        - m2-coreutils
        - m2-zip
    - m2-make
  host:
    - r-base
    - r-rcpp
    - r-rcpparmadillo
  run:
    - r-base
    - r-rcpp
    - r-rcpparmadillo

tests:
  - script:
      interpreter: r
      content: |
        # Ensure R can find the installed package
        .libPaths(c(file.path(Sys.getenv("PREFIX"), "lib", "R", "library"), .libPaths()))
        library("SpecsVerification")
        TRUE

about:
  homepage: https://CRAN.R-project.org/package=SpecsVerification
  license: GPL-2.0-or-later
  summary: A collection of forecast verification routines developed for the SPECS FP7 project. The emphasis is on comparative verification of ensemble forecasts of weather and climate.
```

## Basic Recipe

If however you want to start with working through the recipe by yourself, you can start with a basic set of requirements. An example would be:

```yaml
package:
  name: r-mypackage
  version: 1.0.0

requirements:
  host:
    - r-base
    - r-dependency  # You can add any R package dependencies here
  run:
    - r-base
    - r-dependency

tests:
  - script:
      interpreter: r
      content: | # You can add your R script here as well
        library(mypackage)
        TRUE
```

## From Source (Windows)

Sometimes, conda won't provide prebuilt binaries for Windows. When that happens, we need to compile from the source code, which is extremely easy and straightforward in rattler-build. We just need to add required build tools to our recipe file!

```yaml
requirements:
  build:
    - r-base
    - vs2019_win-64    # Visual Studio
    - m2-coreutils     # For 'basename'
    - m2-base          # MSYS2 utilities
```

## Testing

Test scripts should load the package and return `TRUE` for success:

```yaml
library(mypackage)
# Verify functionality if needed
TRUE
```

Although there will be packages that you will need to compile from source code, our suggestion is to always find and prefer pre-built conda packages over compiling from source when possible. Regardless, rattler-build will still function faster than it's alternatives even while compiling from source code!
