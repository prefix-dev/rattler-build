# Writing a Python package

Writing a Python package is fairly straightforward, especially for "Python-only" packages.
In the second example we will build a package for `numpy` which contains compiled code.

## Generating a starter recipe

Rattler-Build provides a command to generate a recipe for a package from PyPI.
The generated recipe can be used as a starting point for your recipe.
The recipe generator will fetch the metadata from PyPI and generate a recipe that will build the package from the `sdist` source distribution.

```bash
rattler-build generate-recipe pypi ipywidgets
# select an older version of the package
rattler-build generate-recipe pypi ipywidgets --version 8.0.0
```

## A Python-only package

The following recipe uses the `noarch: python` setting to build a `noarch` package that can be installed on any platform without modification.
This is very handy for packages that are pure Python and do not contain any compiled extensions.

Additionally, `noarch: python` packages work with a range of Python versions (contrary to packages with compiled extensions that are tied to a specific Python version).


```yaml title="recipe.yaml"
--8<-- "docs/snippets/recipes/ipywidgets.yaml"
```

1. The `noarch: python` line tells Rattler-Build that this package is pure
   Python and can be one-size-fits-all. `noarch` packages can be installed on any
   platform without modification which is very handy.
2. The `imports` section in the tests is used to check that the package is
   installed correctly and can be imported.

### Running the recipe

To build this recipe, simply run:

```bash
rattler-build build --recipe ./ipywidgets
```

## Multiple `noarch: python` variants for different Python versions

A `noarch: python` package is built only once and works with any Python
version. But sometimes a pure Python package needs *different requirements
depending on the Python version* it is installed with.

A good real-world example is `botocore`: the package supports Python 3.8 and
up, but requires `urllib3 <1.27` on Python 3.8 / 3.9 and allows `urllib3 <2.1`
on Python 3.10 and up. A single `noarch: python` package cannot express this,
but you can build **two variants of the same `noarch: python` package** — one
per Python version range.

To do so, define a custom variant key in the `variants.yaml` file next to your
recipe:

```yaml title="variants.yaml"
--8<-- "docs/snippets/recipes/variants/botocore.yaml"
```

Then use the variant key in [selectors](../selectors.md) to switch the
requirements between the two variants. The following recipe is based on
conda-forge's [`botocore` feedstock](https://github.com/conda-forge/botocore-feedstock/blob/0ee2e3cdb522285b93a9897affe668990832f0bf/recipe/meta.yaml):

```yaml title="recipe.yaml"
--8<-- "docs/snippets/recipes/botocore.yaml"
```

1. The two variants would normally only differ in their hash. Setting an
   explicit build string with a readable prefix makes it easy to see which
   variant of the package you are getting. The `hash` and `build_number`
   variables are provided by Rattler-Build.
2. Since `python_geq_310` is a boolean variant key, it can be used directly as
   a condition in selectors.
3. This is the reason for building two variants: on Python 3.10+, newer
   versions of `urllib3` are supported.

Building this recipe produces two `noarch` packages of the same name and
version:

```txt
Build variant: botocore-1.34.56-pyge38_c34f5b3_0

╭─────────────────┬──────────╮
│ Variant         ┆ Version  │
╞═════════════════╪══════════╡
│ python_geq_310  ┆ false    │
│ target_platform ┆ "noarch" │
╰─────────────────┴──────────╯

Build variant: botocore-1.34.56-pyge310_96c4f1e_0

╭─────────────────┬──────────╮
│ Variant         ┆ Version  │
╞═════════════════╪══════════╡
│ python_geq_310  ┆ true     │
│ target_platform ┆ "noarch" │
╰─────────────────┴──────────╯
```

In an environment with Python 3.8 or 3.9, only the `pyge38` variant is
installable. With Python 3.10 or newer, both variants can be installed — but
since the `pyge310` variant allows newer versions of `urllib3` (and the solver
maximizes dependency versions), it is usually the one that gets picked. If you
need explicit control over which variant the solver prefers, take a look at
[prioritizing variants](../variants.md#prioritizing-variants).

!!! tip "Variants based on other dependencies"
    The same technique works for any dependency, not just Python itself. For
    example, to ship one variant of your package for `pydantic` v1 and one for
    `pydantic` v2, you can use the variant values directly in a dependency:

    ```yaml title="variants.yaml"
    pydantic:
      - "1.*"
      - "2.*"
    ```

    ```yaml title="recipe.yaml (excerpt)"
    build:
      noarch: python
      string: pydantic${{ pydantic[0] }}_${{ hash }}_${{ build_number }}

    requirements:
      run:
        - python >=3.9
        - pydantic ${{ pydantic }}
    ```

    This builds `my-package-1.0.0-pydantic1_<hash>_0` and
    `my-package-1.0.0-pydantic2_<hash>_0`, and the solver picks the variant
    that is compatible with the `pydantic` version in your environment.

## A Python package with compiled extensions

We will build a package for `numpy` – which contains compiled code.
Since compiled code is `python` version-specific, we will need to specify the `python` version explicitly.

The best way to do this is with a "variants.yaml" file.
The variant config file allows us to easily compile the package against multiple Python versions.

```yaml title="variants.yaml"
python:
  - 3.11
  - 3.12
```

This will replace any `python` found in the recipe with the versions specified in the `variants.yaml` file.

```yaml title="recipe.yaml"
--8<-- "docs/snippets/recipes/numpy.yaml"
```

The build script for Unix:

```bash title="build.sh"
mkdir builddir

$PYTHON -m build -w -n -x \
    -Cbuilddir=builddir \
    -Csetup-args=-Dblas=blas \
    -Csetup-args=-Dlapack=lapack

$PYTHON -m pip install dist/numpy*.whl
```

The build script for Windows:

```bat title="build.bat"
mkdir builddir

%PYTHON% -m build -w -n -x ^
    -Cbuilddir=builddir ^
    -Csetup-args=-Dblas=blas ^
    -Csetup-args=-Dlapack=lapack
if %ERRORLEVEL% neq 0 exit 1

:: `pip install dist\numpy*.whl` does not work on windows,
:: so use a loop; there's only one wheel in dist/ anyway
for /f %%f in ('dir /b /S .\dist') do (
    pip install %%f
    if %ERRORLEVEL% neq 0 exit 1
)
```

### Running the recipe

Running this recipe with the variant config file will build a total of 2 `numpy` packages:

```bash
rattler-build build --recipe ./numpy
```

At the beginning of the build process, Rattler-Build will print the following message to show you the variants it found:

```txt
Found variants:

numpy-1.26.4-py311h5f8ada8_0
╭─────────────────┬───────────╮
│ Variant         ┆ Version   │
╞═════════════════╪═══════════╡
│ python          ┆ 3.11      │
│ target_platform ┆ osx-arm64 │
╰─────────────────┴───────────╯

numpy-1.26.4-py312h440f24a_0
╭─────────────────┬───────────╮
│ Variant         ┆ Version   │
╞═════════════════╪═══════════╡
│ python          ┆ 3.12      │
│ target_platform ┆ osx-arm64 │
╰─────────────────┴───────────╯
```

## An ABI3-compatible package

Certain packages contain compiled code that is compatible with multiple Python versions.
This is the case e.g. for a lot of Rust / PyO3 based Python extensions.

In this case, you can use the special `abi3` settings to build a package that is specific to a certain operating system and architecture, but compatible with multiple Python versions.

Note: this feature relies on the `python-abi3` package which exists in the `conda-forge` channel.
The full recipe can be found on [`conda-forge/py-rattler-feedstock`](https://github.com/conda-forge/py-rattler-feedstock)

```yaml title="recipe.yaml"
--8<-- "docs/snippets/recipes/py-rattler.yaml"
```

1. The `python-abi3` package is a special package that ensures that the   run dependencies
   are compatible with the ABI3 standard.
2. The `python_version` setting is used to test against the oldest compatible Python version.

## Testing Python packages

Testing Python packages is done using the `tests` section of the recipe.
We can either use a special "python" test or a regular script test to test the package.

All tests will have the current package and all it's run dependencies installed in an isolated environment.

```yaml title="recipe.yaml"
# contents of the recipe.yaml file
tests:
  - python:
      # The Python test type will simply import packages as a sanity check.
      imports:
        - rattler
        - rattler.version.Version
      pip_check: true # (4)!
      # You can select different Python versions to test against.
      python_version: ["${{ python_min ~ '.*' }}", "3.12.*"]  # (1)!

  # You can run a script test to run arbitrary code.
  - script:
      - pytest ./tests
    requirements:  # (2)!
      run:
         - pytest
    files:  # (3)!
      source:
        - tests/
  # You can also directly execute a Python script and run some tests from it.
  # The script is searched in the `recipe` directory.
  - script: mytest.py
```

1. The `python_version` setting is used to test against different Python versions. It is useful to test against the minimum version of Python that the package supports.
2. We can add additional requirements for the test run. such as pytest, pytest-cov, ... – you can also specify a `python` version here by adding e.g. `python 3.12.*` to the run requirements.
3. This will copy over the tests from the source directory into the package. Note that this makes the package larger, so you might want to use a different approach for larger packages.
4. The `pip_check` will run `pip check` in the environment to make sure that all dependencies are installed correctly. By default, this is set to `true`.
