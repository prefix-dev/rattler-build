# Selectors in recipes

Recipe and variant configuration files can utilize selectors to conditionally
add, remove, or modify dependencies, configuration options, or even skip recipe
execution based on specific conditions.

Selectors are implemented using an `if / then / else` map, which is a
valid YAML dictionary. The condition is evaluated using [`minijinja`][minijinja] and
follows
the same syntax as a Python expression.

During rendering, several variables are set based on the platform and variant
being built. For example, the `unix` variable is true for macOS and Linux, while
`win` is true for Windows. Consider the following recipe executed on Linux:

```yaml
requirements:
  host:
    - if: unix
      then: unix-tool
    - if: win
      then: win-tool
```

This will be evaluated as:

```yaml
requirements:
  host:
    - unix-tool
```

The line containing the Windows-specific configuration is removed. Multiple
items can also be selected, such as:

```yaml
host:
  - if: linux
    then:
      - linux-tool-1
      - linux-tool-2
      - linux-tool-3
```

For Linux, this will result in:

```yaml
host:
  - linux-tool-1
  - linux-tool-2
  - linux-tool-3
```

Other examples often found in the wild:

```yaml
if: build_platform != target_platform ... # true if cross-platform build
if: osx and arm64 ... # true for apple silicon (osx-arm64)
if: linux and (aarch64 or ppc64le)) ... # true for linux ppc64le or linux-aarch64
```

### Available variables

The following variables are available during rendering of the recipe:

| Variable             | Description                                                            |
|----------------------|------------------------------------------------------------------------|
| `target_platform`    | the configured `target_platform` for the build                         |
| `build_platform`     | the configured `build_platform` for the build                          |
| `linux`              | "true" if `target_platform` is Linux                                   |
| `osx`                | "true" if `target_platform` is OSX / macOS                             |
| `win`                | "true" if `target_platform` is Windows                                 |
| `unix`               | "true" if `target_platform` is a Unix (macOS or Linux)                 |
| `x86`, `x86_64`      | x86 32/64-bit Architecture                                             |
| `aarch64`, `arm64`   | 64-bit Arm (these are the same but are both supported for legacy)      |
| `armV6l`, `armV7l`   | 32-bit Arm                                                             |
| `ppc64`, `s390x`,    | Big endian                                                             |
| `ppc64le`            | Little endian                                                          |
| `riscv32`, `riscv64` | The [RISC-V](https://wikipedia.org/wiki/RISC-V) Architecture           |
| `wasm32`             | The [WebAssembly](https://wikipedia.org/wiki/WebAssembly) Architecture |

### Variant selectors

To select based on [variant configuration](variants.md) you can use the names in the selectors as well.
For example, if the build uses `python: 3.8` as a variant, we can use `if: python == "3.8"` to enable a dependency for
only
when the Python version is 3.8.

!!! note "String comparison"
The comparison is a string comparison done by [`minijinja`][minijinja], so it is important to use the correct string
representation of the variant.
Use the `match` function to compare versions.

```yaml title="variants.yaml"
python:
  - 3.8
  - 3.9
```

```yaml title="recipe.yaml"
requirements:
  host:
    - if: python == "3.8" # (1)!
      then: mydep
      else: otherdep
```

1. This will only add `mydep` when the Python version is 3.8. This comparison is a string comparison, so it is important
   to
   use the correct string representation of the variant.

### The `match` function

!!!  note "Rename from `cmp` to `match`"
    The `cmp` function has been renamed to `match` to better reflect its purpose.

Inside selectors, one can use a special `match` function to test if the selected variant version has a matching version.
For example, having the following variants file, we could use the these tests:

```yaml title="variants.yaml"
python:
  - 3.8
  - 3.9
```

```yaml title="recipe.yaml"
- if: match(python, "3.8")    # true, false
  then: mydep
- if: match(python, ">=3.8")  # true, true
  then: mydep
- if: match(python, "<3.8")   # false, false (1)
  then: mydep
```

1. `else: ` would also have worked here.

This function eliminates the need to implement any Python-specific `conda-build`
selectors (such as `py3k`, `py38`, etc.) or the `py` and `npy` integers.

Please note that during the _initial_ phase of rendering we do not know the
variant, and thus the `match` condition always evaluates to `true`.

### Selector evaluation

Except for the rattler-build specific selectors, the selectors are evaluated using the `minijinja` engine. This means
that the selectors are evaluated by [`minijinja`][minijinja] thus Python like expressions.
Some notable options are:

```yaml
- if: python == "3.8" # equal
- if: python != "3.8" # not equal
- if: python and linux # true if python variant is set and the target_platform is linux
- if: python and not linux # true if python variant is set and the target_platform is not linux
- if: python and (linux or osx) # true if python variant is set and the target_platform is linux or osx
```

[minijinja]: https://github.com/mitsuhiko/minijinja

### Alternatives for scalar fields

Some fields accept scalars rather than lists, and selectors cannot be used. Alternatives include:

```yaml
string: ${{ "foobar" if USE_OPENMP else "bla" }}
```

```yaml
string: |
  {% if USE_OPENMP %}
     blablabla
  {% else %}
     blabla
  {% endif %}
```
