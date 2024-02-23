# Rebuilding a package

The `rebuild` command allows you to rebuild a package from an _existing
package_. The main use case is to examine if a package can be rebuilt in a
reproducible manner. You can read more about [reproducible builds
here](https://reproducible-builds.org/).

## Usage

```bash
rattler-build rebuild ./mypkg-0.1.0-h60d57d3_0.tar.bz2
```

### How it works

The recipe is "rendered" and stored into the package. The way the recipe is
rendered is subject to change. For the moment, the rendered recipe is stored as
`info/recipe/rendered_recipe.yaml`. It includes the exact package versions that
were used at build time. When rebuilding, we use the package resolutions from
the rendered recipe, and execute the same build script as the original package.

We also take great care to sort files in a deterministic manner as well as
erasing any time stamps. The `SOURCE_DATE_EPOCH` environment variable is set to
the same timestamp as the original build for additional determinism (some build
tools use this variable to set timestamps).

## How to check the reproducibility of a package

There is an excellent tool called [`diffoscope`](https://diffoscope.org/) that
allows you to compare two packages and see the differences. You can install it
with `pixi`:

```bash
pixi global install diffoscope
```

To compare two packages, you can use the following command:

```bash
rattler-build rebuild ./build0.tar.bz2
diffoscope ./build0.tar.bz2 ./mypkg-0.1.0-h60d57d3_0.tar.bz2
```
