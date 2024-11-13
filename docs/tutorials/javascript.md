# Packaging a Javascript (NPM/NodeJS) package

This tutorial will guide you though making a NodeJS package with
`rattler-build`. Please note that, while packaging executable applications is
possible, the conda ecosystem is not ideal for NPM libraries. NPM supports a
number of features that cannot easily be modeled in the conda ecosystem, such as
peer dependencies, optional dependencies, and the ability to install multiple
versions of the same package.

However, if you need to package a NodeJS application, `rattler-build` can help!

## Building a NodeJS Package

In this example, we will build a package for the NodeJS package `bibtex-tidy`.
We use `nodejs` in build and run requirements, and install the package using
`npm`. NPM comes as part of the NodeJS installation, so we do not need to
install it separately.

```yaml title="recipe.yaml"
context:
  version: "1.14.0"

package:
  name: bibtex-tidy
  version: ${{ version }}

source:
  url: https://registry.npmjs.org/bibtex-tidy/-/bibtex-tidy-${{ version }}.tgz
  sha256: 0a2c1bb73911a7cee36a30ce1fc86feffe39b2d39acd4c94d02aac6f84a00285
  # we do not extract the source code and install the tarball directly as that works better
  file_name: bibtex-tidy-${{ version }}.tgz

build:
  number: 0
  script:
    # we use NPM to globally install the bibtex-tidy package
    - npm install -g bibtex-tidy-${{ version }}.tgz --prefix ${{ PREFIX }}

requirements:
  build:
    - nodejs
  run:
    - nodejs

tests:
  - script:
    - bibtex-tidy --version
```
