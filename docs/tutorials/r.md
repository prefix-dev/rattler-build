# Packaging an R Package and executing a script

This guide shows how to create recipes for R packages with scripts to execute them with rattler-build.

## Basic Recipe

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
