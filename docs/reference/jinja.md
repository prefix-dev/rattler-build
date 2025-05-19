# Jinja

`rattler-build` comes with a couple of useful [Jinja](https://jinja.palletsprojects.com)
functions and filters that can be used in the recipe.

## Functions

### The compiler function

The compiler function can be used to put together a compiler that works for the
current platform and the compilation "`target_platform`". The syntax looks like:
`${{ compiler('c') }}` where `'c'` signifies the programming language that is
used.

This function evaluates to `<compiler>_<target_platform> <compiler_version>`.
For example, when compiling _on_ `linux` and _to_ `linux-64`, this function
evaluates to `gcc_linux-64`.

The values can be influenced by the `variant_configuration`. The
`<lang>_compiler` and `<lang>_compiler_version` variables are the keys with
influence. See below for an example:

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

The variables shown above would select the `clang` compiler in version `9.0`.
Note that the final output will still contain the `target_platform`, so that the
full compiler will read `clang_linux-64 9.0` when compiling with
`--target-platform linux-64`.

`rattler-build` defines some default compilers for the following languages
(inherited from `conda-build`):

- `c`: `gcc` on Linux, `clang` on `osx` and `vs2017` on Windows
- `cxx`: `gxx` on Linux, `clangxx` on `osx` and `vs2017` on Windows
- `fortran`: `gfortran` on Linux, `gfortran` on `osx` and `vs2017` on Windows
- `rust`: `rust`

### The `stdlib` function

The `stdlib` function closely mirrors the compiler function. It can be used to
put together a standard library that works for the current platform and the
compilation "`target_platform`".

Usage: `${{ stdlib('c') }}`

Results in `<stdlib>_<target_platform> <stdlib_version>`. And uses the variant
variables `<lang>_stdlib` and `<lang>_stdlib_version` to influence the output.

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

A pin is created based on the version input (from a subpackage or a package
resolution).

The pin functions take the following three arguments:

- `lower_bound` (default: `"x.x.x.x.x.x"`): The lower bound pin expression to be
  used. When set to `None`, no lower bound is set.
- `upper_bound` (default: `"x"`): The maximum pin to be used. When set to
  `None`, no upper bound is set.

The lower bound and upper bound can either be a "pin expression" (only `x` and
`.` are allowed) or a hard-coded version string.

A "pin expression" is applied to the version input to create the lower and upper
bounds. For example, if the version is `3.10.5` with a  `lower_bound="x.x",
upper_bound="x.x.x"`, the lower bound will be `3.10` and the upper bound will be
`3.10.6.0a0`. A pin expression for the `upper_bound` will increment the last
selected segment of the version by `1`, and append `.0a0` to the end to prevent
any alpha versions from being selected.

If the last segment of the version contains a letter (e.g. `9e` or `1.1.1j`),
then incrementing the version will set that letter to `a`, e.g. `9e` will become
`10a`, and `1.1.1j` will become `1.1.2a`. In this case, also no `0a0` is
appended to the end.

Sometimes you want to strongly connect your outputs. This can be achieved with
the following input:

- `exact=True` (default: `False`): This will pin the version exactly to the
  version of the output, incl. the build string.

To override the lower or upper bound with a hard-coded value, you can use the
following input:

- `lower_bound` (default: `None`): This will override the lower bound with the
  given value.
- `upper_bound` (default: `None`): This will override the upper bound with the
  given value.

Both `lower_bound` and `upper_bound` expect a valid version string (e.g.
`1.2.3`).

To add an build-string matching expression, you can use the `build` argument:

- `build` (default: `None`): This will add a build string matching expression to
  the pin. The build string matching expression is a string that is used to
  match the build string with the match spec. For example, if the build string is
  `py38_0`, the build string matching expression could be `py38*` or to match
  exactly `py38_0`. The `build` and `exact` options are mutually exclusive.

#### The `pin_subpackage` function

- `${{ pin_subpackage("mypkg", lower_bound="x.x", upper_bound="x.x") }}` creates a pin
  to another output in the recipe. With an input of `3.1.5`, this would create a
  pin of `mypkg >=3.1,<3.2.0a0`.
- `${{ pin_subpackage("other_output", exact=True) }}` creates a pin to another
  output in the recipe with an exact version.
- `${{ pin_subpackage("other_output", lower_bound="1.2.3", upper_bound="1.2.4")
  }}` creates a pin to another output in the recipe with a lower bound of
  `1.2.3` and an upper bound of `1.2.4`. This is equivalent to writing
  `other_output >=1.2.3,<1.2.4`.
- `${{ pin_subpackage("foo", build="py38*") }}` creates a matchspec like `foo >=3.1,<3.2.0a0 py38*`.

#### The `pin_compatible` function

The pin compatible function works exactly as the `pin_subpackage` function, but
it pins the package in the run requirements based on the resolved package of the
`host` or `build` section.

- `pin_compatible` pins a package in the run requirements based on the resolved
  package of the `host` or `build` section.

### The `cdt` function

- `${{ cdt("mypkg") }}` creates a cross-dependency to another output in the
  recipe.

This function helps add Core Dependency Tree packages as dependencies by
converting packages as required according to hard-coded logic. See below for an
example of how this function can be used:

```yaml
# on x86_64 system
cdt('package-name') # outputs: package-name-cos6-x86_64
# on aarch64 system
cdt('package-name') # outputs: package-name-cos6-aarch64
```

### The `hash` variable

- `${{ hash }}` is the variant hash and is useful in the build string
  computation.

### The `version_to_buildstring` function

- `${{ python | version_to_buildstring }}` converts a version from the variant
  to a build string (it removes the `.` character and takes only the first two
  elements of the version).

### The `env` object

You can use the `env` object to retrieve environment variables and forward them
to your build script. `${{ env.get("MY_ENV_VAR") }}` will return the value of
the environment variable `MY_ENV_VAR` or throw an error if it is not set.

To supply a default value when the environment variable is not set, you can use
`${{ env.get("MY_ENV_VAR", default="default_value") }}`. In this case, if
`MY_ENV_VAR` is not set, the value `default_value` will be returned (and no
error is thrown).

You can also check for the existence of an environment variable:

- `${{ env.exists("MY_ENV_VAR") }}` will return `true` if the environment
  variable `MY_ENV_VAR` is set and `false` otherwise.

## Tests

You can write tests using minijinja to check whether objects have certain properties.
The syntax for a filter is `{{ variable is test_name }}`.

- `undefined`: Check whether a variable is undefined.
- `defined`: Check whether a variable is defined.
- `none`: Check whether a variable is none.
- `safe`: Check whether a variable is safe.
- `escaped`: Check whether a variable is escaped. Same as `is safe`.
- `odd`: Check whether a number is odd.
- `even`: Check whether a number is even.
- `number`: Check whether a variable is a number.
- `integer`: Check whether a variable is an integer.
- `int`: Check whether a variable is an integer. Same as `is integer`.
- `float`: Check whether a variable is a float.
- `string`: Check whether a variable is a string.
- `sequence`: Check whether a variable is a sequence.
- `boolean`: Check whether a variable is a boolean.
- `startingwith`: Check whether a variable is starting with another string: `{{ python is startingwith('3.12') }}`
- `endingwith`: Check whether a variable is starting with another string: `{{ python is endingwith('.*') }}`

## Filters

A feature of `jinja` is called "filters". Filters are functions that can be
applied to variables in a template expression.

The syntax for a filter is `{{ variable | filter_name }}`. A filter can also
take arguments, such as `... | replace('foo', 'bar')`.

The following Jinja filters are available, taken from the upstream `minijinja`
library:

- `replace`: replace a string with another string (e.g. `"{{ 'foo' | replace('oo', 'aa') }}"` will return `"faa"`)
- `lower`: convert a string to lowercase (e.g. `"{{ 'FOO' | lower }}"` will return `"foo"`)
- `upper`: convert a string to uppercase (e.g. `"{{ 'foo' | upper }}"` will
return `"FOO"`) - `int`: convert a string to an integer (e.g. `"{{ '42' | int }}"` will return `42`)
- `abs`: return the absolute value of a number (e.g. `"{{ -42 | abs }}"` will return `42`)
- `bool`: convert a value to a boolean (e.g. `"{{ 'foo' | bool }}"` will return `true`)
- `default`: return a default value if the value is falsy (e.g. `"{{ '' | default('foo') }}"` will return `"foo"`)
- `first`: return the first element of a list (e.g. `"{{ [1, 2, 3] | first }}"`
will return `1`) - `last`: return the last element of a list (e.g. `"{{ [1, 2, 3] | last }}"` will return `3`)
- `length`: return the length of a list (e.g. `"{{ [1, 2, 3] | length }}"` will return `3`)
- `list`: convert a string to a list (e.g. `"{{ 'foo' | list }}"` will return `['f', 'o', 'o']`)
- `join`: join a list with a separator (e.g. `"{{ [1, 2, 3] | join('.') }}"` will return `"1.2.3"`)
- `min`: return the minimum value of a list (e.g. `"{{ [1, 2, 3] | min }}"` will return `1`)
- `max`: return the maximum value of a list (e.g. `"{{ [1, 2, 3] | max }}"` will return `3`)
- `reverse`: reverse a list (e.g. `"{{ [1, 2, 3] | reverse }}"` will return `[3, 2, 1]`)
- `sort`: sort a list (e.g. `"{{ [3, 1, 2] | sort }}"` will return `[1, 2, 3]`)
- `trim`: remove leading and trailing whitespace from a string (e.g. `"{{ ' foo ' | trim }}"` will return `"foo"`)
- `unique`: remove duplicates from a list (e.g. `"{{ [1, 2, 1, 3] | unique }}"` will return `[1, 2, 3]`)
- `split`: split a string into a list (e.g. `"{{ '1.2.3' | split('.') | list }}"` will return `['1', '2', '3']`). By default, splits on whitespace.

??? "Removed filters"

    The following filters are removed from the builtins:

    - `attr`
    - `indent`
    - `select`
    - `selectattr`
    - `dictsort`
    - `reject`
    - `rejectattr`
    - `round`
    - `map`
    - `title`
    - `capitalize`
    - `urlencode`
    - `escape`
    - `pprint`
    - `safe`
    - `items`
    - `float`
    - `tojson`

### Extra filters for recipes

#### The `version_to_buildstring` filter

- `${{ python | version_to_buildstring }}` converts a version from the variant
  to a build string (it removes the `.` character and takes only the first two
  elements of the version).

For example the following:

```yaml
context:
  cuda: "11.2.0"

build:
  string: ${{ hash }}_cuda${{ cuda_version | version_to_buildstring }}
```

Would evaluate to a `abc123_cuda112` (assuming the hash was `abc123`).

### Various remarks

#### Inline conditionals with Jinja

The new recipe format allows for inline conditionals with Jinja. If they are
falsey, and no `else` branch exists, they will render to an empty string (which
is, for example in a list or dictionary, equivalent to a YAML `null`).

When a recipe is rendered, all values that are `null` must be filtered from the
resulting YAML.

```yaml
requirements:
  host:
    - ${{ "numpy" if cuda == "yes" }}
```

If `cuda` is not equal to yes, the first item of the host requirements will be
empty (null) and thus filtered from the final list.

This must also work for dictionary values. For example:

```yaml
build:
  number: ${{ 100 if cuda == "yes" }}
  # or an `else` branch can be used, of course
  number: ${{ 100 if cuda == "yes" else 0 }}
```

#### Slicing lists

Lists can be spliced using the regular Python `[i:j]` syntax.  Note that when
lists are obtained through using filters such as `split`, the whole filter
expression needs to be parenthesized.

For example, to slice a version string from `x.y.z` to `x.y`:

```jinja
${{ (version | split('.'))[:2] | join('.') }}
```
