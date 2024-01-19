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

We can pass a variant configuration file to `rattler-build` using a command line
like this:

```sh
rattler-build build --variant-config ./variants.yaml --recipe myrecipe.yaml
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

will be rendered as (for the first variant)

```yaml
# ...
requirements:
  host:
- python 3.8*
```

Note that variants are _only_ applied if the requirement doesn't specify any
constraints. If the requirement would be `python >3.8,<3.10` the variant entry
would be ignored.

## Package hash from variant

You might have wondered what the role of the build string is. The build string is (if not explicitly set) computed from the variant configuration.
It serves as a mechanism to discern different build configurations that produce a package with the same name and version.

The hash is computed by dumping all the variant configuration values that are used by a given recipe into a JSON file, and then hashing that JSON file.
For example, in our `python` example, we would get a variant configuration file that looks something like:

```json
{
    "python": "3.8"
}
```

This JSON string is then hashed with the MD5 hash algorithm, and produces the hash.
For certain packages (such as Python packages) special rules exists, and the `py<Major.Minor>` version is prepended to the hash, so that the final hash
would look something like `py38h123123`.

### Zip Keys

Zip keys modify how variants are combined. Usually, each variant key that has multiple
entries is expanded to a build matrix, for example if we have:

```yaml
python: ["3.8", "3.9"]
numpy: ["1.12", "1.14"]
```

We obtain 4 variants for a recipe that uses both `numpy` and `python`:

```
- python 3.8, numpy 1.12
- python 3.8, numpy 1.14
- python 3.9, numpy 1.12
- python 3.9, numpy 1.14
```

However, if we use the `zip_keys` and specify

```yaml
zip_keys: ["python", "numpy"]
python: ["3.8", "3.9"]
numpy: ["1.12", "1.14"]
```

Then the versions are "zipped up" and we only get two variants. Note that both, `python` and `numpy` need to specify the exact same number of
versions to make this work.
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

### Compilers

As part of the key value fields of the variant config file, they follow identical syntax.
But provide control over compiler related packages/libraries rather than traditional packages.

```yaml
c_compiler:
  - gcc
```

This when mixed with If conditional allows for robustly switching compilers based on platform and targets.

```yaml
c_compiler:
  - if: win
    then: m2w64-gcc # mingw compilers
    else: gcc
```

Some of the compilers supported are,

```yaml
rust_compiler: ~
cxx_compiler: ~
fortran_compiler: ~ 
```