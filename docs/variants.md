# Variant configuration

`rattler-build` can automatically build multiple _variants_ of a given package.
For example, a Python package might need multiple variants per Python version
(especially if it is a binary package such as `numpy`).

For this use case, one can specify _variant_ configuration files. A variant
configuration file has 2 special entries and a list of packages with variants.
For example:

```yaml title="variants.yaml"
# special entry #1, the zip keys
zip_keys:
- [python, numpy]

# special entry #2, the pin_run_as_build key
pin_run_as_build:
  numpy:
    max_pin: 'x.x'

# entries per package version that users are interested in
python:
# Note that versions are _strings_ (not numbers)
- "3.8"
- "3.9"
- "3.10"

numpy:
- "1.12"
- "1.12"
- "1.20"
```

If we have a recipe, that has a `build`, `host` or `run` dependency on `python`
we will build multiple variants of this package, one for each configured
`python` version ("3.8", "3.9" and "3.10").

For example:

```yaml
# ...
requirements:
  host:
  - python
```

... will be rendered as (for the first variant):

```yaml
# ...
requirements:
  host:
- python 3.8*
```

Note that variants are _only_ applied if the requirement doesn't specify any
constraints. If the requirement would be `python >3.8,<3.10` then the variant entry
would be ignored.

## Automatic Discovery

`rattler-build` automatically discovers and includes variant configurations from
either:

- `variants.yaml` file located next to the recipe
- `conda_build_config.yaml` file located next to the recipe

To disable automatic discovery, use the `--ignore-recipe-variants` flag.
If you pass variant configuration files explicitly using `--variant-config / -m
<file>`, the passed variants are loaded with higher priority.

### Custom Configuration Files

To specify variant configurations from other locations or include multiple
files, use the `--variant-config` or `-m` option:

```sh
rattler-build build --variant-config ~/user_variants.yaml --variant-config /opt/rattler-build/global_variants.yaml --recipe myrecipe.yaml
```

### Merging of multiple variant configuration files

When multiple variant configuration files are merged, the following rules apply:

- A key from a higher priority file will completely override a key from a lower priority file.
- Zip key lengths must still match.

### `conda-build` Compatibility

Since version 0.35.0, rattler-build supports conda_build_config.yaml files,
parsing a subset of conda-build's configuration syntax. The filename must match
exactly to be recognized as a conda-build config file.

## Package hash from variant

You might have wondered what the role of the build string is. The build string is (if not explicitly set) computed from the variant configuration.
It serves as a mechanism to discern different build configurations that produce a package with the same name and version.

The hash is computed by dumping all of the variant configuration values that are used by a
given recipe into a JSON file, and then hashing that JSON file.

For example, in our `python` example, we would get a variant configuration file that looks something like:

```json
{
    "python": "3.8"
}
```

This JSON string is then hashed with the MD5 hash algorithm, and produces the hash.
For certain packages (such as Python packages) special rules exists, and the `py<Major.Minor>` version is prepended to the hash, so that the final hash
would look something like `py38h123123`.

### Zip keys

Zip keys modify how variants are combined. Usually, each variant key that has multiple
entries is expanded to a build matrix. For example, if we have:

```yaml
python: ["3.8", "3.9"]
numpy: ["1.12", "1.14"]
```

...then we obtain 4 variants for a recipe that uses both `numpy` and `python`:

```
- python 3.8, numpy 1.12
- python 3.8, numpy 1.14
- python 3.9, numpy 1.12
- python 3.9, numpy 1.14
```

However, if we use the `zip_keys` and specify:

```yaml
zip_keys: ["python", "numpy"]
python: ["3.8", "3.9"]
numpy: ["1.12", "1.14"]
```

...then the versions are "zipped up" and we only get 2 variants. Note that
both `python` and `numpy` need to specify the exact same number of versions
to make this work.

The resulting variants with the zip applied are:

```
- python 3.8, numpy 1.12
- python 3.9, numpy 1.14
```

### Pin run as build

The `pin_run_as_build` key allows the user to inject additional pins. Usually, the `run_exports` mechanism is used to
specify constraints for runtime dependencies from _build_ time dependencies, but `pin_run_as_build` offers a mechanism
to override that if the package does not contain a run exports file.

For example:

```yaml
pin_run_as_build:
  libcurl:
    min_pin: 'x'
    max_pin: 'x'
```

If we now have a recipe that uses `libcurl` in the `host` and `run` dependencies like:

```yaml

requirements:
  host:
  - libcurl
  run:
  - libcurl
```

During resolution, `libcurl` might be evaluated to `libcurl 8.0.1 h13284`. Our new runtime dependency then
looks like:

```yaml
requirements:
  host:
  - libcurl 8.0.1 h13284
  run:
  - libcurl >=8,<9
```

### Channel sources

You can specify the channels when building by adjusting `channel_sources` in your variant file:

```yaml
channel_sources: conda-forge/label/rust_dev,conda-forge
```

## Prioritizing variants

You might produce multiple variants for a package, but want to define a _priority_ for a given variant.
The variant with the highest priority would be the default package that is selected by the resolver.

There are two mechanisms to make this possible: `mutex` packages and the `down_prioritize_variant` option in the recipe.

### The `down_prioritize_variant` option

!!! note
    It is not always necessary to use the `down_prioritize_variant` option - only if the solver has no other way to
    prefer a given variant. For example, if you have a package that has multiple variants for different Python versions,
    the solver will automatically prefer the variant with the highest Python version.

The `down_prioritize_variant` option allows you to specify a variant that should be _down-prioritized_. For example:

```yaml title="recipe.yaml" hl_lines="7"
build:
  variant:
    use_keys:
      # use cuda from the variant config, e.g. to build multiple CUDA variants
      - cuda
    # this will down-prioritize the cuda variant versus other variants of the package
    down_prioritize_variant: ${{ 1 if cuda else 0 }}
```

### Mutex packages

Another way to make sure the right variants are selected are "mutex" packages. A mutex package is a package that is
mutually exclusive. We use the fact that only one package of a given name can be installed at a time (the solver has to choose).

A mutex package might be useful to make sure that all packages that depend on BLAS are compiled against the same BLAS implementation.
The mutex package will serve the purpose that "`openblas`" and "`mkl`" can never be installed at the same time.

We could define a BLAS mutex package like this:

```yaml title="variant_config.yaml"
blas_variant:
  - "openblas"
  - "mkl"
```

And then the `recipe.yaml` for the `mutex` package could look like this:

```yaml title="recipe.yaml" hl_lines="6 9"
package:
  name: blas_mutex
  version: 1.0

build:
  string: ${{ blas_variant }}${{ hash }}_${{ build_number }}
  variant:
    # make sure that `openblas` is preferred over `mkl`
    down_prioritize_variant: ${{ 1 if blas_variant == "mkl" else 0 }}
```

This will create two package: `blas_mutex-1.0-openblas` and `blas_mutex-1.0-mkl`.
Only one of these packages can be installed at a time because they share the same name.
The solver will then only select one of these two packages.

The `blas` package in turn should have a `run_export` for the `blas_mutex` package, so that any package
that links against `blas` also has a dependency on the correct `blas_mutex` package:

```yaml title="recipe.yaml" hl_lines="2 8"
package:
  name: openblas
  version: 1.0

requirements:
  # any package depending on openblas should also depend on the correct blas_mutex package
  run_export:
    # Add a run export on _any_ version of the blas_mutex package whose build string starts with "openblas"
    - blas_mutex * openblas*
```

Then the recipe of a package that wants to build two variants, one for `openblas` and
one for `mkl` could look like this:

```yaml title="recipe.yaml" hl_lines="8"
package:
  name: fastnumerics
  version: 1.0

requirements:
  host:
    # build against both openblas and mkl
    - ${{ blas_variant }}
  run:
    # implicitly adds the correct blas_mutex package through run exports
    # - blas_mutex * ${{ blas_variant }}*
```
