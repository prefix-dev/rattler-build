# Subpackages (experimental)

!!! warning
    Subpackages are an **experimental** feature and may change. Enable them with
    the `--experimental` flag (or `RATTLER_BUILD_EXPERIMENTAL=true`).

Subpackages let you build a package once and *split off* a set of files into
separate packages — for example separating a C/C++ library's headers and
development files into a `-dev` package, or its man pages into a `-doc` package.
This is similar to subpackages in Linux distributions and to
[melange](https://github.com/chainguard-dev/melange)'s subpackages.

The owning output performs a single build, and then:

- each subpackage **claims** the files matching its `files` globs, and
- the owning output keeps the **remainder** — every built file not claimed by a
  subpackage.

## Quick example

```yaml
package:
  name: mylib
  version: 1.2.3

source:
  url: https://example.com/mylib-1.2.3.tar.gz
  sha256: "..."

build:
  script: cmake --install build --prefix $PREFIX

requirements:
  build:
    - ${{ compiler('cxx') }}
  run:
    - libstdcxx

subpackages:
  # Development files: headers, the .so symlink, CMake/pkg-config files.
  - package:
      name: mylib-dev
    files:
      - include/**
      - lib/**/*.so
      - lib/cmake/**
      - lib/pkgconfig/**
    requirements:
      run:
        - ${{ pin_subpackage('mylib', exact=true) }}
    about:
      summary: Development files for mylib

  # Documentation / man pages.
  - package:
      name: mylib-doc
    files:
      - share/man/**
      - share/doc/**
```

This builds **once** and produces three packages:

- `mylib` — everything except the files claimed below (the shared libraries),
- `mylib-dev` — the headers, the linker symlink and the CMake/pkg-config files,
- `mylib-doc` — the man pages and documentation.

Subpackages also work inside the outputs of a multi-output recipe — add a
`subpackages` key under any `package` output.

## Fields

Each entry under `subpackages` accepts:

| Field          | Description                                                                                                  |
| -------------- | ------------------------------------------------------------------------------------------------------------ |
| `package`      | `name` (required) and an optional `version` (defaults to the parent output's version).                        |
| `files`        | Glob patterns selecting which built files this subpackage claims. A list, or an `include`/`exclude` mapping. |
| `requirements` | `run`, `run_constraints`, `run_exports`, `ignore_run_exports`. `build`/`host` are not allowed (see below).    |
| `about`        | About metadata. Unset fields are inherited from the parent output's `about`.                                  |
| `tests`        | Tests for this subpackage.                                                                                    |

## Semantics

- **First match wins.** Files are claimed in subpackage declaration order. A file
  matched by an earlier subpackage is not re-claimed by a later one. Anything
  unclaimed stays with the parent output. Claiming is computed on the concrete
  built files, so overlapping globs are predictable.
- **Internal excludes fall through.** If a subpackage's `files` uses an
  `include`/`exclude` mapping, files it excludes are *not* claimed and remain
  available for a later subpackage or the parent. For example
  `include: [lib/**], exclude: [lib/*.a]` leaves the static libraries for
  another subpackage or the parent.
- **No separate build.** Subpackages share the parent's single build (the build
  runs once), so they cannot declare `build`/`host` requirements — those belong
  to the build and are declared on the owning output. They *can* declare
  independent `run` requirements.
- **`pin_subpackage`.** Subpackages and the parent can reference each other with
  `pin_subpackage(...)` — the common case being `pin_subpackage('<parent>',
  exact=true)` from a `-dev` package so it always installs the matching build of
  the runtime package.
- **Run-exports.** Subpackages inherit the run-exports contributed by the build/host
  environment, just like the parent package.
- **About inheritance.** A subpackage inherits the parent output's `about`
  section; any field set on the subpackage overrides it.

## Under the hood

Subpackages are a first-class, single-build mechanism: the output builds **once**,
then the resulting file set is partitioned between the parent and its subpackages
and one conda package is written per subpackage (plus the parent remainder). They
are *not* turned into separate outputs or a staging cache, so a recipe with
subpackages still renders as a single output (with the subpackages attached),
which keeps the build logs and rendered recipe easy to reason about.

## Limitations

- Reusable subpackage *templates* (e.g. an automatic C/C++ `-dev` split) are not
  available yet; they are planned as a follow-up.
