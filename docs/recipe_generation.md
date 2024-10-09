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

The generated recipe for `jinja2` will look something like:

```yaml title="recipe.yaml"
package:
  name: jinja2
  version: 3.1.4

source:
- url: https://files.pythonhosted.org/packages/ed/55/39036716d19cab0747a5020fc7e907f362fbf48c984b14e62127f7e68e5d/jinja2-3.1.4.tar.gz
  sha256: 4a3aee7acbbe7303aede8e9648d13b8bf88a429282aa6122a993f0ac800cb369

build:
  script: python -m pip install .

requirements:
  host:
  - flit_core <4
  - python >=3.7
  - pip
  run:
  - python >=3.7
  - markupsafe >=2.0
  # - babel >=2.7  # extra == 'i18n'

tests: []

about:
  summary: A very fast and expressive template engine.
  documentation: https://jinja.palletsprojects.com/
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
package:
  name: r-dplyr
  version: 1.1.4

source:
- url: https://cran.r-project.org/src/contrib/dplyr_1.1.4.tar.gz
  md5: e3066ea859b26e0d3b992c476ea3af2e

build:
  script: R CMD INSTALL --build .
  python: {}

requirements:
  host:
  - r-base >=3.5.0
  run:
  - r-cli >=3.4.0
  - r-generics
  - r-glue >=1.3.2
  - r-lifecycle >=1.0.3
  - r-magrittr >=1.5
  - r-methods
  - r-pillar >=1.9.0
  - r-r6
  - r-rlang >=1.1.0
  - r-tibble >=3.2.0
  - r-tidyselect >=1.2.0
  - r-utils
  - r-vctrs >=0.6.4
  # -  r-bench  # suggested
  # -  r-broom  # suggested
  # -  r-callr  # suggested
  # -  r-covr  # suggested
  # -  r-dbi  # suggested
  # -  r-dbplyr >=2.2.1  # suggested
  # -  r-ggplot2  # suggested
  # -  r-knitr  # suggested
  # -  r-lahman  # suggested
  # -  r-lobstr  # suggested
  # -  r-microbenchmark  # suggested
  # -  r-nycflights13  # suggested
  # -  r-purrr  # suggested
  # -  r-rmarkdown  # suggested
  # -  r-rmysql  # suggested
  # -  r-rpostgresql  # suggested
  # -  r-rsqlite  # suggested
  # -  r-stringi >=1.7.6  # suggested
  # -  r-testthat >=3.1.5  # suggested
  # -  r-tidyr >=1.3.0  # suggested
  # -  r-withr  # suggested

about:
  homepage: https://dplyr.tidyverse.org, https://github.com/tidyverse/dplyr
  summary: A Grammar of Data Manipulation
  description: |-
    A fast, consistent tool for working with data frame like
    objects, both in memory and out of memory.
  license: MIT
  license_file: LICENSE
  repository: https://github.com/cran/dplyr
```

!!!tip

    You can use the generated recipes to build your own "forge" with `rattler-build`. Read more about it in the [Building your own forge](./tips_and_tricks.md#building-your-own-forge) section.
