# Multi-Output Recipes

## Overview

A multi-output recipe produces multiple packages from a single `recipe.yaml` file.
Multi-output recipes are used when a single source tree produces multiple packages.
Common examples include splitting a C library into runtime, development, and language binding packages.

In a multi-output recipe each output is a self-contained recipe with its own `package`, `build`, `requirements`, `tests`, and `about` sections.

## Basic Structure

A minimal multi-output recipe with two package outputs:

```yaml
recipe:
  name: my-project
  version: 1.0.0

source:
  - url: https://example.com/my-project-1.0.0.tar.gz
    sha256: abcdef...

outputs:
  - package:
      name: my-lib
    build:
      script:
        - mkdir -p $PREFIX/lib
        - cp libfoo.so $PREFIX/lib/
    requirements:
      run:
        - some-runtime-dep
    tests:
      - script:
          - test -f $PREFIX/lib/libfoo.so

  - package:
      name: my-tool
    requirements:
      run:
        - ${{ pin_subpackage('my-lib', upper_bound='x.x') }}
    build:
      script:
        - mkdir -p $PREFIX/bin
        - cp my-tool $PREFIX/bin/
    tests:
      - script:
          - my-tool --version
```

### Connecting outputs with `pin_subpackage`

Use `pin_subpackage()` to create a dependency from one output to another within
the same recipe:

```yaml
requirements:
  run:
    - ${{ pin_subpackage('my-lib', upper_bound='x.x') }}
```

With `exact=True`, the dependent package is injected as a variant. This means if
`my-lib` has two variant builds (e.g., against `openssl 1` and `openssl 3`),
`my-tool` will also be built twice — once for each `my-lib` variant:

```yaml
outputs:
  - package:
      name: my-lib
    requirements:
      host:
        - openssl
  - package:
      name: my-tool
    requirements:
      run:
        - ${{ pin_subpackage('my-lib', exact=True) }}
```

### Topological sorting


Outputs are topologically sorted based on their dependency relationships.
If one output depends on another (for example via `pin_subpackage` or by
referencing its name in requirements), the dependent output will be built
after its dependency.

The order of entries in `outputs:` does not affect build order.


## Inheritance Behavior

Top-level sections are inherited by all outputs unless the output overrides them:

| Section   | Inherited? | Notes                                      |
|-----------|------------|---------------------------------------------|
| `source`  | Yes        | Available to all outputs                    |
| `build`   | Yes        | `number`, `script`, etc. can be overridden  |
| `about`   | Yes        | Per-output `about` overrides top-level      |
| `tests`   | Yes        | Top-level tests are prepended to output tests |
| `extra`   | Yes        | Merged into each output                     |
| `context` | Yes        | Variables are available during rendering of all outputs    |

The `package` and `requirements` keys are **forbidden** at the top level in
multi-output recipes. Each output must define its own `package:` and
`requirements:` sections.

```yaml
recipe:
  name: my-project
  version: 2.0.0

# These are inherited by all outputs:
about:
  license: MIT
  homepage: https://example.com

build:
  number: 0

outputs:
  - package:
      name: output-a
      # version defaults to 2.0.0 from recipe.version
    # inherits about.license, about.homepage, build.number

  - package:
      name: output-b
      version: 3.0.0  # overrides recipe.version
    about:
      license: Apache-2.0  # overrides inherited license
```


## Staging Outputs (Experimental)

!!! warning
    Staging outputs require the `--experimental` flag:
    `rattler-build build --experimental -r recipe.yaml`

A staging output compiles code once and caches the result. Package outputs then
inherit from the staging cache and select subsets of the built files. This avoids
rebuilding the same source for each package.

### Basic staging example

```yaml
recipe:
  name: mylib
  version: 2.0.0

source:
  - url: https://example.com/mylib-2.0.0.tar.gz
    sha256: abcdef...

outputs:
  # Staging: builds the library once
  - staging:
      name: mylib-build
    requirements:
      build:
        - ${{ compiler('c') }}
        - cmake
        - ninja
    build:
      script:
        - cmake -GNinja -DCMAKE_INSTALL_PREFIX=$PREFIX .
        - ninja install

  # Package: runtime library
  - package:
      name: libmylib
    inherit: mylib-build
    build:
      files:
        - lib/*
    requirements:
      run_exports:
        - ${{ pin_subpackage('libmylib') }}

  # Package: development headers
  - package:
      name: mylib-headers
    inherit: mylib-build
    build:
      files:
        - include/*
    requirements:
      run:
        - ${{ pin_subpackage('libmylib', exact=True) }}
```

### The `inherit` key

The `inherit:` key specifies which staging cache a package output inherits from.

**Short form** — inherit all files and run exports:

```yaml
inherit: mylib-build
```

**Structured form** — control run export inheritance:

```yaml
inherit:
  from: mylib-build
  run_exports: false  # do not inherit run exports from staging
```

### Run exports from staging

Run exports from the staging output's build/host dependencies are propagated to
inheriting package outputs by default. Use `run_exports: false` in the structured
`inherit:` form to suppress this.

If the staging output has `ignore_run_exports`, those filters apply at the
staging level. If an inheriting output also ignores run exports, both filters
apply.

### Source handling with staging

The top-level `source:` is available to the staging output and all package
outputs. Each output restores the (dirty) source from the staging directory, so
outputs can continue from where the staging build left off (e.g., running
`cmake install` after staging already ran `cmake build`).

Outputs can add additional sources on top of the staging source:

```yaml
outputs:
  - package:
      name: py-mylib
    inherit: mylib-build
    source:
      - path: ../README.md
        file_name: extra_file.md
```


## File Selection

In multi-output recipes, use `build: files:` to select which files from the
prefix end up in each package. This is especially important with staging outputs
to avoid packaging the same files in multiple packages.

```yaml
build:
  files:
    - lib/*.so
    - lib/*.so.*
```

For more advanced selection with include/exclude patterns, see the
[build options documentation](../build_options.md#include-only-certain-files-in-the-package).
