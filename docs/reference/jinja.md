# Jinja functions that can be used in the recipe

`rattler-build` comes with a couple of useful helpers that can be used in the recipe.

## Functions

### The compiler function

The compiler function can be used to put together a compiler that works for the current platform and the compilation "`target_platform`".
The syntax looks like: `${{ compiler('c') }}` where `'c'` signifies the programming language that is used.

This function evaluates to `<compiler>_<target_platform> <compiler_version>`.
For example, when compiling _on_ `linux` and _to_ `linux-64`, this function evaluates to `gcc_linux-64`.

The values can be influenced by the `variant_configuration`.
The `<lang>_compiler` and `<lang>_compiler_version` variables are the keys with influence. See below for an example:

#### Usage in a recipe

```yaml title="recipe.yaml"
requirements:
  build:
    - ${{ compiler('c') }}
```

With a corresponding variant_configuration:

```yaml title="variant_configuration.yaml"
c_compiler:
- clang
c_compiler_version:
- 9.0
```

The variables shown above would select the `clang` compiler in version `9.0`. Note that the final output will still contain the `target_platform`, so that the full compiler will read `clang_linux-64 9.0` when compiling with `--target-platform linux-64`.

`rattler-build` defines some default compilers for the following languages (inherited from `conda-build`):

- `c`: `gcc` on Linux, `clang` on `osx` and `vs2017` on Windows
- `cxx`: `gxx` on Linux, `clangxx` on `osx` and `vs2017` on Windows
- `fortran`: `gfortran` on Linux, `gfortran` on `osx` and `vs2017` on Windows
- `rust`: `rust`

### The `stdlib` function

The `stdlib` function closely mirrors the compiler function. It can be used to put together a standard library that works for the current platform and the compilation "`target_platform`".

Usage: `${{ stdlib('c') }}`

Results in `<stdlib>_<target_platform> <stdlib_version>`. And uses the variant variables `<lang>_stdlib` and `<lang>_stdlib_version` to influence the output.

#### Usage in a recipe:

```yaml title="recipe.yaml"
requirements:
  build:
    # these are usually paired!
    - ${{ compiler('c') }}
    - ${{ stdlib('c') }}
```

With a corresponding variant_configuration:

```yaml title="variant_configuration.yaml"
# these are the values `conda-forge` uses in their pinning file
# found at https://github.com/conda-forge/conda-forge-pinning-feedstock/blob/main/recipe/conda_build_config.yaml
c_stdlib:
- sysroot
c_stdlib_version:
- 2.17
```

### The `pin` functions

A pin is created based on the version input (from a subpackage or a package resolution).

The pin functions take the following five arguments:

- `min_pin` (default: `"x.x.x.x.x.x"`): The minimum pin to be used. When set to `None`, no lower bound is set.
- `max_pin` (default: `"x"`): The maximum pin to be used. When set to `None`, no upper bound is set.

These "pins" are applied to the version input to create the lower and upper bounds. For example, if the version is `3.10.5` with `min_pin="x.x", max_pin="x.x.x"`, the lower bound will be `3.10` and the upper bound will be `3.10.6.0a0`. The `max_pin` will increment the last selected segment of the version by `1`, and append `.0a0` to the end to prevent any alpha versions from being selected.

If the last segment of the version contains a letter (e.g. `9e` or `1.1.1j`), then incrementing the version will set that letter to `a`, e.g. `9e` will become `10a`, and `1.1.1j` will become `1.1.2a`. In this case, also no `0a0` is appended to the end.

Sometimes you want to strongly connect your outputs. This can be achieved with the following input:

- `exact=True` (default: `False`): This will pin the version exactly to the version of the output, incl. the build string.

To override the lower or upper bound with a hard-coded value, you can use the following input:

- `lower_bound` (default: `None`): This will override the lower bound with the given value.
- `upper_bound` (default: `None`): This will override the upper bound with the given value.

Both `lower_bound` and `upper_bound` expect a valid version string (e.g. `1.2.3`).

#### The `pin_subpackage` function

- `${{ pin_subpackage("mypkg", min_pin="x.x", max_pin="x.x") }}` creates a pin to another output in the recipe. With an input of `3.1.5`, this would create a pin of `mypkg >=3.1,<3.2.0a0`.
- `${{ pin_subpackage("other_output", exact=True) }}` creates a pin to another output in the recipe with an exact version.
- `${{ pin_subpackage("other_output", lower_bound="1.2.3", upper_bound="1.2.4") }}` creates a pin to another output in the recipe with a lower bound of `1.2.3` and an upper bound of `1.2.4`. This is equivalent to writing `other_output >=1.2.3,<1.2.4`.

#### The `pin_compatible` function

The pin compatible function works exactly as the `pin_subpackage` function, but it pins the package in the run requirements based on the resolved package of the `host` or `build` section.

- `pin_compatible` pins a package in the run requirements based on the resolved package of the `host` or `build` section.

### The `cdt` function

- `${{ cdt("mypkg") }}` creates a cross-dependency to another output in the recipe.

This function helps add Core Dependency Tree packages as dependencies by converting packages as required according to hard-coded logic. See below for an example of how this function can be used:

```yaml
# on x86_64 system
cdt('package-name') # outputs: package-name-cos6-x86_64
# on aarch64 system
cdt('package-name') # outputs: package-name-cos6-aarch64
```

### The `hash` variable

- `${{ hash }}` is the variant hash and is useful in the build string computation.

### The `version_to_buildstring` function

- `${{ python | version_to_buildstring }}` converts a version from the variant to a build string (it removes the `.` character and takes only the first two elements of the version).

### The `env` object

You can use the `env` object to retrieve environment variables and forward them to your build script. There are two ways to do this:

- `${{ env.get("MY_ENV_VAR") }}` will return the value of the environment variable `MY_ENV_VAR` or throw an error if it is not set.
- `${{ env.get_default("MY_ENV_VAR", "default_value") }}` will return the value of the environment variable `MY_ENV_VAR` or `"default_value"` if it is not set.

You can also check for the existence of an environment variable:

- `${{ env.exists("MY_ENV_VAR") }}` will return `true` if the environment variable `MY_ENV_VAR` is set and `false` otherwise.

## Default Jinja filters

The following Jinja filters are available: `lower`, `upper`, indexing into characters (e.g. `https://myurl.com/{{ name[0] }}/{{ name | lower }}_${{ version }}.tar.gz`).

Navigate to the [Minijinja documentation](https://docs.rs/minijinja/latest/minijinja/filters/index.html#built-in-filters) for a list of all available built-in filters.
