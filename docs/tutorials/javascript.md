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
--8<-- "docs/snippets/recipes/bibtex-tidy.yaml"
```
