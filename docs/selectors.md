# Selectors in recipes

Recipe and variant configuration files can utilize selectors to conditionally
add, remove, or modify dependencies, configuration options, or even skip recipe
execution based on specific conditions.

Selectors are implemented using a simple `if / then / else` map, which is a
valid YAML dictionary. The condition is evaluated using `minijinja` and follows
the same syntax as a Python expression.

During rendering, several variables are set based on the platform and variant
being built. For example, the unix variable is true for macOS and Linux, while
win is true for Windows. Consider the following recipe executed on Linux:


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

The following variables are available during the initial rendering and
afterward:

| Variable                      | Description                                                                                      |
| ----------------------------- | ------------------------------------------------------------------------------------------------ |
| `target_platform`             | the configured target_platform for the build                                                     |
| `build_platform`              | the build platform                                                                               |
| `linux`                       | true if target_platform is Linux                                                                 |
| `osx`                         | true if target_platform is OSX / macOS                                                           |
| `win`                         | true if target_platform is Windows                                                               |
| `unix`                        | true if target_platform is a Unix (macOS or Linux)                                               |
| `x86_64`, `x86`, `arm64`, ... | The architecture ("x86_64" for 64 bit, "x86" for 32 bit, otherwise arm64, aarch64, ppc64le, ...) |

After the initial phase, when the variant configuration is selected, the variant
values are also available in selectors. For example, if the build uses `python:
3.8` as variant, we can use `if: python == "3.8"` to enable a dependency only
when the Python version is 3.8.

### The `cmp` function

Inside selectors, one can use a special `cmp` function to test if the selected
variant version has a matching version. For example, if we have again a `python:
3.8` variant, we could use the following tests:

```yaml
- if: cmp(python, "3.8")    # true
  then: mydep
- if: cmp(python, ">=3.8")  # true
  then: mydep
- if: cmp(python, "<3.8")   # false
  then: mydep
```

This function eliminates the need to implement any python-special conda-build
selectors (such as `py3k`, `py38`, etc.) or the `py` and `npy` integers.

Please note that during the _initial_ phase of rendering we do not know the
variant, and thus the `cmp` condition always evaluates to true.
