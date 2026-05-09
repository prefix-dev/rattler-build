# Minimal Env Vars: Candidates for Removal

A review of the environment variables that `rattler-build` injects into build
scripts, with suggestions for what could be dropped (or made opt-in) in a more
minimal env setup. Triggered by the discussion in
[#2487](https://github.com/prefix-dev/rattler-build/issues/2487) about
`CMAKE_GENERATOR=Unix Makefiles`.

## TL;DR

The cleanest first cut for a "minimal" mode would drop these vars, all of which
are inherited from `conda-build` and either point at outdated systems, duplicate
information already available elsewhere, or target tools that have been
deprecated:

- `BUILD` (Linux/macOS/Windows triples)
- `CMAKE_GENERATOR`
- `NPY_DISTUTILS_APPEND_FLAGS`
- `PY3K`
- `CYGWIN_PREFIX`
- `DEJAGNU`, `QEMU_LD_PREFIX`, `QEMU_UNAME`, `DISPLAY` (Linux)

## Strong candidates (legacy / outdated / dead defaults)

### `BUILD` triple

Set in `crates/rattler_build_core/src/{linux,macos,windows}/env.rs`.

| Platform | Current value |
| --- | --- |
| Linux | `{arch}-conda_cos6-linux-gnu` or `cos7` |
| macOS x86 | `x86_64-apple-darwin13.4.0` |
| macOS arm64 | `arm64-apple-darwin20.0.0` |
| macOS 32-bit | `i386-apple-darwin13.4.0` |
| Windows | `{arch}-pc-windows-19.0.0` |

These are autotools-style triples from Anaconda's old build infrastructure.
CentOS 6 went EOL in 2020 and CentOS 7 in 2024. Darwin 13 corresponds to OS X
10.9 (2013). Most projects that genuinely need a build triple will run
`config.guess` and produce an accurate value; the hardcoded one is misleading.

### `CMAKE_GENERATOR=Unix Makefiles`

Set in `crates/rattler_build_core/src/unix/env.rs:10`. The subject of #2487:
forces Make even when Ninja is available, conflicting with `scikit-build-core`
defaults and noticeably slowing down builds.

### `NPY_DISTUTILS_APPEND_FLAGS=1`

Set in `crates/rattler_build_core/src/env_vars.rs`. `numpy.distutils` was
removed in NumPy 1.26 (2023). Dead weight for any modern Python build.

### `PY3K`

Python 2 has been EOL since 2020. `PY3K` was a Py2/Py3 transition flag and is
no longer meaningful.

### `CYGWIN_PREFIX`

Set in `crates/rattler_build_core/src/windows/env.rs:143`. Almost no modern
conda recipes use Cygwin paths.

## Medium candidates (niche / rarely used)

### `DEJAGNU` (Linux)

Only relevant to the test suites of GCC, binutils, gdb, and glibc. Forwarding
it when nothing in the host environment sets it is a no-op, but it does not
belong in the default list of "things every recipe gets".

### `QEMU_LD_PREFIX` / `QEMU_UNAME` (Linux)

Only relevant when building under QEMU emulation. A natural fit for an opt-in
"cross-compilation extras" group rather than the default env.

### `DISPLAY` (Linux)

Leaks the host's X11 display into the build. Most builds should not need it;
the few test suites that do (Selenium, some GUI packages) should request it
explicitly.

### `R_USER`

Conda-build legacy that points R's user library at the build prefix. Modern R
packaging via `R CMD INSTALL --library=…` does not need it.

### `CONDA_DEFAULT_ENV`

Duplicates `PREFIX`.

### `OSX_ARCH` (macOS)

Duplicates `ARCH`. Kept for compatibility with some older recipes that
explicitly grep for it.

## Worth discussing but defensible

These are inherited from `conda-build` too, but they have functional
consequences beyond just the env dictionary, so removing them is a behavior
change rather than a cleanup:

- `PIP_NO_BUILD_ISOLATION=False`
- `PIP_NO_DEPENDENCIES=True`
- `PIP_IGNORE_INSTALLED=True`
- `PIP_NO_INDEX=True`

They are load-bearing for the conda-build pip workflow (we supply the deps;
pip should not reach out to PyPI). I would keep these.

`CONDA_BUILD_STATE` only matters for recipes that branch on `BUILD` vs `TEST`.
It is useful but conda-specific. Worth keeping for compatibility.

The strict-mode synthetic values (`USER=rattler`, `SHELL=/bin/bash`,
`EDITOR=/bin/false`, `TERM=xterm-256color`) are deliberate hermeticity choices,
not legacy — keep.

## Should keep (fundamental)

For reference, these are the vars that should stay in any "minimal" mode
because they are fundamental to how recipes work:

- `PREFIX`, `BUILD_PREFIX`, `SRC_DIR`, `RECIPE_DIR`, `BUILD_DIR`
- `PKG_NAME`, `PKG_VERSION`, `PKG_BUILDNUM`, `PKG_BUILD_STRING`, `PKG_HASH`
- `target_platform`, `host_platform`, `build_platform`, `SUBDIR`, `ARCH`
- `PYTHON`, `PY_VER`, `SP_DIR`, `STDLIB_DIR` (when Python is a dep)
- `R`, `R_VER` (when R is a dep)
- `PATH` / `Path`
- `LIBRARY_PREFIX`, `LIBRARY_BIN`, `LIBRARY_LIB`, `LIBRARY_INC` (Windows)
- `PKG_CONFIG_PATH`
- `LD_RUN_PATH` (Linux — important for rpath)
- `MACOSX_DEPLOYMENT_TARGET`
- `SOURCE_DATE_EPOCH` (reproducibility)
- `CONDA_BUILD_CROSS_COMPILATION`
- `PYTHONNOUSERSITE`, `PYTHONDONTWRITEBYTECODE` (noarch)
- `SHLIB_EXT`

## Possible rollout

Two reasonable shapes for this:

1. **Just delete the obviously-dead ones** (`BUILD`, `NPY_DISTUTILS_APPEND_FLAGS`,
   `PY3K`, `CYGWIN_PREFIX`). Low risk, no flag needed.
2. **Add a `minimal_env: true` build option** that gates the medium-confidence
   set as well, leaving the current defaults in place for existing recipes.

Option 1 could ship now; option 2 covers anything we are unsure about without
breaking existing recipes.
