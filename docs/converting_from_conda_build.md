# Converting a recipe from conda-build

The recipe format of `rattler-build` differs in some aspects from `conda-build`.
This document aims to help you convert a recipe from `conda-build` to
`rattler-build`.

## Automatic conversion

To convert a recipe from `meta.yaml` to `recipe.yaml` you can use the automatic
conversion utility.

To install `conda-recipe-manager`, run

```bash
pixi global install conda-recipe-manager
# or
conda install -c conda-forge conda-recipe-manager
```

Then, run the conversion utility:

```bash
conda-recipe-manager convert my-recipe/meta.yaml
```

This will print the converted recipe to the console. You can save it to a file
by redirecting the output:

```bash
conda-recipe-manager convert my-recipe/meta.yaml > recipe.yaml
```

To learn more about the tool, or contribute, find the [repository
here](https://github.com/conda-incubator/conda-recipe-manager/).

## Converting Jinja and selectors

To use `jinja` in the new recipes, you need to keep in mind two conversions. The
`{% set version = "1.2.3" %}` syntax is replaced by the `context` section in the new
recipe format.

```
{% set version = "1.2.3" %}
```

becomes

```yaml
context:
  version: "1.2.3"
```

To use the values or other Jinja expressions (e.g. from the variant config) you
can use the `${{ version }}` syntax. Note the `$` sign before the curly braces - it
makes Jinja fully compatible with the YAML format.

```yaml title="meta.yaml"
# instead of
package:
  version: "{{ version }}"
source:
  url: https://example.com/foo-{{ version }}.tar.gz
```

becomes

```yaml title="recipe.yaml"
package:
  version: ${{ version }}
source:
  url: https://example.com/foo-${{ version }}.tar.gz
```

## Converting selectors

`conda-build` has a line based "selector" system, to e.g. disable certain fields
on Windows vs. Unix.

In rattler-build we use two different syntaxes: an `if/else/then` map or a
inline jinja expression.

A typical selector in `conda-build` looks something like this:

```yaml title="meta.yaml"
requirements:
  host:
    - pywin32  # [win]
```

To convert this to `rattler-build` syntax, you can use one of the following two
syntaxes:

```yaml title="recipe.yaml"
requirements:
  host:
    - ${{ "pywin32" if win }}  # empty strings are automatically filtered
    # or
    - if: win
      then:
        - pywin32  # this list extends the outer list
```

## Converting the recipe script

We still support the `build.sh` script, but the `bld.bat` script was renamed to `build.bat`
in order to be more consistent with the `build.sh` script.

You can also choose a different name for your script:

```yaml
build:
  # note: if there is no extension, we will try to find .sh on unix and .bat on windows
  script: my_build_script
```

There are also new ways of writing scripts, for [example with `nushell` or `python`](build_script.md)

!!!danger "Variant keys in build scripts"
    `conda-build` tries to analyze the build scripts for any usage of variant keys. We do _not_ attempt that.
    If you want to use variant keys in your build script that are not used anywhere else you need to manually
    add them to your script environment, e.g.

    ```yaml title="recipe.yaml"
    build:
      script:
        content: echo $MY_VARIANT
        env:
          MY_VARIANT: ${{ my_variant }}
    ```

## Converting the recipe structure

There are a few differences in the recipe structure. However, the schema will
tell you quite easily what is expected and you should see red squiggly lines in
your editor (e.g. VSCode) if you make a mistake.

Here are a few differences:

- `build.run_exports` is now `requirements.run_exports`
- `requirements.run_constrained` is now `requirements.run_constraints`
- `build.ignore_run_exports` is now `requirements.ignore_run_exports.by_name`
- `build.ignore_run_exports_from` is now
  `requirements.ignore_run_exports.from_package`
- A `git` source now uses `git`, `tag`, ... and not `git_url` and `git_rev`, e.g.
  ```yaml
  git: https://github.com/foo/bar.git
  tag: 1.2.3
  ```

## Converting the test section

The `test` section is renamed to `tests` and is a list of independent tests.
Each test runs in its own environment.

Let's have a look at converting an existing test section:

```yaml title="meta.yaml"
test:
  imports:
    - mypackage
  commands:
    - mypackage --version
```

This would now be split into two tests:

```yaml title="recipe.yaml"
tests:
  - script:
      - mypackage --version
  - python:
      imports:
        - mypackage
      # by default we perform a `pip check` in the python test but
      # it can be disabled by setting this to false
      pip_check: false
```

The `script` tests also take a `requirements` section with `run` and `build`
requirements. The `build` requirements can be used to install emulators and
similar tools that need to run to execute tests in a cross-compilation
environment.

# Automatic feedstock conversion

Use the tool [`feedrattler`](https://github.com/hadim/feedrattler) by [hadim](https://github.com/hadim) to go directly from an existing conda-forge v0 recipe feedstock to the new v1 recipe used by rattler-build.

You can install and use it directly by running `pixi exec`:
```
pixi exec feedrattler my-awesome-feedstock
```

It uses the `conda-recipe-manager` for the generation of the recipe and `gh` or a `GITHUB_TOKEN` for creating the conversion PR in your name.

Alternative installation:
```
# Globally install the tool
pixi global install feedrattler
# or in a workspace
pixi add feedrattler
# or using conda/mamba
conda install -c conda-forge feedrattler
```
