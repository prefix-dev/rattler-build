# Design: Subpackages (melange-style file splitting)

Status: **Draft / in progress**
Author: (initial draft)
Tracking branch: `claude/laughing-albattani-xlwgak`

## Motivation

When building a package, especially for compiled C/C++ libraries, a single build
produces many different *kinds* of files that conventionally belong in separate
packages:

- the runtime shared libraries (`lib/*.so.*`) — the "main" package,
- development files: headers (`include/**`), the linker symlink (`lib/*.so`),
  CMake/pkg-config files (`lib/cmake/**`, `lib/pkgconfig/**`) — a `-dev` package,
- documentation and man pages (`share/man/**`, `share/doc/**`) — a `-doc` package,
- static libraries (`lib/*.a`) — a `-static` package.

Today, producing these split packages in rattler-build requires writing a full
multi-output recipe with a staging cache, repeating the source/build wiring, and
manually carving the file sets apart with `build.files` include/exclude globs —
including hand-maintaining the "everything that is left over" set for the main
package. This is verbose and error-prone.

Linux distributions (and, more recently, Chainguard's
[melange](https://github.com/chainguard-dev/melange)) make this ergonomic: you
declare *subpackages*, each one grabbing a set of paths, and the main package
implicitly keeps the remainder. We want the same ergonomics in rattler-build.

## Goals

- Add a `subpackages` key under an output (both single-output recipes and the
  outputs of a multi-output recipe).
- A subpackage **splits off** a set of files (selected by globs) from the build.
- The **owning output keeps the remainder** — every built file not claimed by a
  subpackage stays in the parent package.
- A subpackage can declare its own `requirements` (notably `run`/`run_constraints`),
  `about`, and `tests`, independent of the parent.
- Subpackages and the parent can refer to each other with `pin_subpackage(...)`,
  including the common `pin_subpackage('<parent>', exact=true)` from a `-dev`
  package.
- The build runs **once** for the parent + all its subpackages (no rebuild per
  subpackage).

## Non-goals (for now)

- **Templating / pipelines.** melange ships reusable "split" pipelines (e.g.
  `split/dev`, `split/static`). We explicitly defer a templating mechanism for
  common C/C++/Python layouts to a later iteration. The data model below is
  designed so templates can later expand *into* `subpackages` entries.
- **Cross-recipe subpackages.** Subpackages always belong to exactly one output
  within one recipe.
- **Conditional subpackage *lists*** (`if/then` choosing whole subpackages).
  Conditionals *inside* a subpackage's fields (e.g. platform-specific `files`)
  are supported via the existing `ConditionalList`, which is enough for v1.

## User-facing design

### Single-output recipe

```yaml
package:
  name: mylib
  version: 1.2.3

source:
  url: https://example.com/mylib-1.2.3.tar.gz
  sha256: ...

build:
  script: |
    cmake --install build --prefix $PREFIX

requirements:
  build:
    - ${{ compiler('cxx') }}
  run:
    - libstdcxx

subpackages:
  - package:
      name: mylib-dev
    files:
      - include/**
      - lib/**/*.so          # the dev symlink
      - lib/cmake/**
      - lib/pkgconfig/**
    requirements:
      run:
        - ${{ pin_subpackage('mylib', exact=true) }}
    about:
      summary: Development files for mylib

  - package:
      name: mylib-doc
    files:
      - share/man/**
      - share/doc/**
```

`mylib` (the parent) is packaged with **everything except** the files claimed by
`mylib-dev` and `mylib-doc`.

### Inside a multi-output recipe

```yaml
recipe:
  name: mylib
  version: 1.2.3

outputs:
  - package:
      name: mylib
    requirements:
      run:
        - libstdcxx
    subpackages:
      - package:
          name: mylib-dev
        files:
          - include/**
          - lib/**/*.so
        requirements:
          run:
            - ${{ pin_subpackage('mylib', exact=true) }}
```

### Semantics

1. **File claiming order.** Subpackages are evaluated in declaration order. A
   built file is assigned to the *first* subpackage whose `files` globs match it.
   Files matched by no subpackage stay with the parent. (First-match-wins mirrors
   melange and makes overlapping globs predictable.)
2. **`version`** defaults to the parent's version; it may be overridden per
   subpackage (rare, but some distros do this).
3. **`build.number`/`build string`** default to the parent's. The variant hash is
   shared (same build, same variant), so subpackages get the same hash component
   of the build string by default.
4. **`requirements`**: a subpackage has no `build`/`host` of its own (it does not
   build); it contributes `run`, `run_constraints`, `run_exports`, and
   `ignore_run_exports`. Run-exports applied to the parent's build environment
   still apply to the whole build; per-subpackage `run` deps are independent.
5. **`about`** defaults to the parent's `about`, with provided fields overriding.
6. **`pin_subpackage`** resolves against every package produced by the recipe,
   parent and subpackages alike, exactly like cross-output pins do today.
7. **Empty subpackages.** If a subpackage's globs match nothing, we emit a
   warning (likely a typo) but still produce an empty package, matching how an
   empty output behaves today. (Configurable to "error" later.)

## Implementation strategy

rattler-build already has all the heavy machinery we need:

- A **staging cache** primitive: a build that runs once and whose result is
  reused by multiple package outputs (`crates/rattler_build_recipe` stage1
  `StagingCache` + `InheritsFrom`).
- **Per-output file selection** via `build.files` include/exclude
  (`crates/rattler_build_types` `GlobVec`, applied in
  `crates/rattler_build_core/src/packaging/file_finder.rs`).
- **`pin_subpackage`** resolution through the global subpackages registry
  (`BuildConfiguration.subpackages`, populated in `src/lib.rs`) and topological
  sorting of outputs (`variant_render::topological_sort_variants`).

There are two ways to map `subpackages` onto this machinery.

### Option A — Desugar into staging + sibling outputs (recipe transform)

Transform an output with `subpackages` into:

- a staging cache `__<name>_build` carrying the `source` + `build` (script,
  build/host requirements),
- the parent package output, inheriting from that cache, with
  `build.files.exclude = ⋃ subpackage include-globs`,
- one package output per subpackage, inheriting from the same cache, with
  `build.files = <subpackage globs>`.

**Pros:** essentially zero new code downstream — rendering, `pin_subpackage`,
topo-sort, dependency resolution, and packaging all work unchanged.

**Cons:** the parent's "remainder" is computed by *exclude globs*, not by
concrete file assignment. This is wrong when a subpackage uses an internal
`exclude` (`include: [lib/**], exclude: [lib/*.a]`): those `*.a` files should
fall through to the parent, but a naive `exclude: [lib/**]` on the parent drops
them entirely. It also re-installs the staging cache and re-walks the prefix once
per output (build still runs once, but file detection is repeated).

### Option B — Single build, split at packaging time (chosen)

Keep the parent as a single build/output. Surface subpackages as **first-class
rendered package descriptors that share the parent's build**, then split the
parent's built files in a single packaging pass:

1. **Render**: subpackages are evaluated alongside the parent so each has a
   concrete name, version, build string, resolved `run` requirements, `about`,
   and `tests`. Each is registered in `BuildConfiguration.subpackages` and takes
   part in the topological sort so `pin_subpackage` resolves correctly.
2. **Build**: runs once, in the parent's host prefix (unchanged).
3. **Package**: after collecting the parent's `new_files`
   (`Files::from_prefix`), partition them by the subpackage globs
   (first-match-wins). For each subpackage produce a conda archive from its
   claimed files with its own metadata; the parent is packaged from the
   remainder. This yields **correct remainder semantics** (concrete files, not
   globs) and a single prefix walk.

**Implemented: Option A (desugaring), behind `--experimental`.** Building once
and splitting files end-to-end is delivered by desugaring `subpackages` into the
existing, well-tested staging machinery, which gives `pin_subpackage`, per-output
`run` dependencies, run-exports inheritance, metadata, and tests for free. The
remainder is expressed as exclude globs (the union of the subpackages' includes),
which is exact for the common include-only case.

Option B (a bespoke packaging-time split with concrete remainder semantics)
remains the longer-term target for precise remainder handling and avoiding the
per-output prefix re-walk; it can replace the desugaring internally without any
recipe-syntax change.

### Where the desugaring hooks in

| Concern | Location |
| --- | --- |
| Schema (stage0) | `crates/rattler_build_recipe/src/stage0/subpackage.rs`, referenced from `SingleOutputRecipe` and `PackageOutput` in `stage0/output.rs` |
| Parsing | `crates/rattler_build_recipe/src/stage0/parser/output_parser.rs` (`parse_subpackages`), wired into `parse_single_output_recipe_with_config` and `parse_package_output`, plus `validate_keys` |
| Desugaring transform | `crates/rattler_build_recipe/src/stage0/desugar.rs` (`recipe_has_subpackages`, `desugar_subpackages`) — pure stage0→stage0 rewrite into a staging-backed multi-output recipe |
| Experimental gate + wiring | `src/lib.rs` `find_variants` — error unless `--experimental`, then desugar before rendering |
| Everything downstream | unchanged: staging evaluation, `pin_subpackage`, topo sort, dependency resolution, packaging (`build.files`/`GlobVec`) |

## Data model

### Stage 0 (`Subpackage`)

```rust
pub struct Subpackage {
    /// Name (required); version optional, inherits the parent output's version.
    pub package: PackageMetadata,
    /// Globs selecting which built files this subpackage claims.
    pub files: IncludeExclude,
    /// Run / run_constraints / run_exports / ignore_run_exports for this
    /// subpackage. build/host are rejected at parse time (no separate build).
    pub requirements: Requirements,
    /// About metadata; unset fields inherit from the parent output.
    pub about: About,
    /// Tests for this subpackage.
    pub tests: ConditionalList<TestType>,
}
```

Added to both `SingleOutputRecipe` and `PackageOutput`:

```rust
#[serde(default, skip_serializing_if = "Vec::is_empty")]
pub subpackages: Vec<Subpackage>,
```

(`Vec`, not `ConditionalList`, for v1 — conditionals live inside each field.)

### Stage 1

A parallel evaluated `Subpackage` (concrete `PackageName`, `VersionWithSource`,
`GlobVec`, evaluated `Requirements`/`About`/tests, plus the resolved build
string) hangs off the stage1 `Recipe`. The renderer registers each subpackage in
the subpackages map and inserts pin edges into the topo sort.

## Validation rules

- Subpackage names must be valid `PackageName`s and unique across the recipe
  (including vs. the parent and other outputs).
- `requirements.build` / `requirements.host` are rejected inside a subpackage.
- `files` is effectively required (a subpackage with no globs claims nothing).
- Overlapping globs are allowed (first-match-wins); we may add an opt-in
  "warn on overlap" later.

## Phased plan

- **Phase 1 (done): schema + parser + tests.** Stage0 `Subpackage` type, parsing
  under single-output and package outputs, field validation, and `used_variables`
  plumbing.
- **Phase 2 (done): desugaring + experimental gate + end-to-end.** Pure
  stage0→stage0 transform into a staging-backed multi-output recipe, gated behind
  `--experimental`, wired into `find_variants`. Reuses all downstream machinery,
  so `pin_subpackage`, per-subpackage `run` deps, about inheritance, run-exports,
  and tests work end-to-end. Covered by unit tests (desugar transform) and
  Python e2e tests (`test/end-to-end/test_subpackages.py`).
- **Phase 3 (later): bespoke packaging-time split (Option B).** Replace the
  desugaring internally with a single-prefix file partition for precise remainder
  semantics (handling subpackage internal `exclude`) and to avoid re-walking the
  prefix per output. No recipe-syntax change.
- **Phase 4 (later): templates/pipelines** for common C/C++/Python layouts that
  expand into `subpackages` entries.

## Open questions

- Should an empty subpackage (no matched files) warn or error by default?
- How should per-subpackage `run_exports` interact with the parent's build
  environment run-exports? (Leaning: subpackage `run_exports` only affect
  *consumers* of that subpackage.)
- Build string customization per subpackage — needed in v1, or always inherit?
