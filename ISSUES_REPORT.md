# rattler-build Open Issues Analysis Report

**Date:** 2026-02-15
**Total Open Issues:** ~167 (includes issues + some PRs)

---

## Executive Summary

The rattler-build repository has approximately 167 open issues spanning bugs, feature requests, documentation gaps, and architectural proposals. This report classifies them, identifies the most urgent issues requiring immediate attention, and highlights easy wins that can be resolved quickly.

---

## 1. Issue Classification

### Category Breakdown

| Category | Count | Description |
|----------|-------|-------------|
| **Bugs (Active Regressions)** | ~15 | Broken functionality, regressions from recent releases |
| **Bugs (Long-standing)** | ~10 | Known issues that have persisted across versions |
| **Feature Requests** | ~45 | New functionality proposals |
| **Enhancements** | ~30 | Improvements to existing functionality |
| **Recipe Generation** | ~8 | Issues with `generate-recipe` command |
| **Documentation** | ~10 | Missing docs, tutorials, schema files |
| **Architecture/Design** | ~15 | Proposals for format changes, CEP amendments |
| **Platform-Specific** | ~12 | Windows/macOS/cross-compilation issues |
| **CI/DevOps** | ~8 | GitHub Actions, output formatting, CI workflows |
| **Support/Questions** | ~5 | User questions and migration help |

---

## 2. URGENT Issues (Recommend Immediate Action)

These are active bugs causing build failures or incorrect outputs for users:

### P0 - Critical (Build Failures / Incorrect Packages)

| # | Title | Why Urgent |
|---|-------|-----------|
| **#2137** | `file_name` no longer works with `patches` | **Regression** - Worked in 0.30.x/0.41, broken in 0.5x. Combining `file_name` + `patches` in source causes `Is a directory (os error 21)`. Blocks users who relied on this pattern. |
| **#2147** | Prefix not substituted for Python 3.11 | Prefix replacement fails specifically for Python 3.11 due to mtime preservation issue. Python 3.10/3.12 unaffected. Blocks multi-version Python builds (e.g., deepspeed). Related to #1865. |
| **#2133** | String prefix replacement broken in Rust binaries | Placeholder paths remain in Rust binaries after install. Null-termination algorithm doesn't handle Rust string representations. Affects nushell and other Rust packages. Labeled `:bug:`. |
| **#1955** | Duplicate RPATHs on macOS >= 15.4 | Duplicate `LC_RPATH` entries cause library load failures. Breaks complex packages like OpenUSD on modern macOS. Straightforward fix: deduplicate RPATHs after processing. |
| **#2110** | Top-level skip not working anymore | **Regression** - Global `build.skip` rules silently ignored when outputs define their own skips. Causes unintended builds (e.g., Windows builds running when skipped globally). Labeled `:bug:`. |

### P1 - High Priority (Significant User Impact)

| # | Title | Why Important |
|---|-------|--------------|
| **#1684** | Dynamic linking checks broken with multi-output cache | Overdepending warnings that don't occur without cache. Incorrect dependency analysis. |
| **#1784** | `CONDA_BUILD_CROSS_COMPILATION` always set to 1 for noarch:python | Incompatibility with conda-build behavior, breaks recipes that check this variable. |
| **#1774** | `MACOSX_DEPLOYMENT_TARGET` / `c_stdlib_version` not working in staged-recipes | Blocks macOS recipes in conda-forge staged-recipes workflow. |
| **#1797** | Build variants create inscrutable solver errors | Users get cryptic errors when variant configs have issues, very hard to debug. |
| **#1638** | Too many open files / `RATTLER_IO_CONCURRENCY_LIMIT` ineffective | Resource exhaustion on macOS with many dependencies. Env var workaround doesn't help. |
| **#2111** | Cryptic error when v0 artifact is passed | Poor error message when wrong artifact format is used. Should be a clear message. |
| **#1984** | Context variable cannot be an empty string | Valid empty string `""` rejected by parser. Blocks recipe migration from conda-build. |

---

## 3. EASY FIXES (Low-Hanging Fruit)

These issues have clear scope, minimal risk, and can likely be resolved quickly:

### Quick Wins (Estimated: Small PRs)

| # | Title | Why Easy | Effort |
|---|-------|----------|--------|
| **#2138** | Absolute symlink should trigger a warning | Add a warning log when absolute symlinks are detected during packaging. Pure additive change. | Trivial |
| **#1955** | Duplicate RPATHs on macOS >= 15.4 | Deduplicate RPATH list after processing. Well-understood fix, localized change. | Small |
| **#243** | Channel names truncated in output | Increase display width or remove truncation for channel names. UI-only change. | Trivial |
| **#715** | Make GitHub error messages prettier | Formatting improvement for error output. | Trivial |
| **#1953** | Report patches already fully applied | Add check + warning when a patch has no effect. | Small |
| **#2103** | Add `--no-build-id` automatically on CI | Detect CI environment and auto-set flag. | Trivial |
| **#1984** | Context variable cannot be empty string | Fix scalar validation to accept `""` as valid. | Small |
| **#837** | Noarch text files should have unix line endings | Normalize line endings for noarch packages. | Small |
| **#1543** | `generate-recipe` produces bad license identifier | Fix license text -> SPDX identifier mapping in recipe generator. | Small |
| **#1544** | `generate-recipe` produces bad test imports | Fix hyphen-to-dot conversion logic for Python imports. | Small |
| **#1988** | `generate-recipe` produces invalid SPDX format for BSD-3-Clause | Same category as #1543 - fix license normalization. | Trivial |
| **#784** | More information on excluded reasons | Add log messages explaining why variants were excluded. | Small |
| **#816** | Cannot add `$ORIGIN` to `rpath_allowlist` | Allow `$ORIGIN` in allowlist pattern matching. | Small |

### Medium-Effort Improvements

| # | Title | Why Worthwhile | Effort |
|---|-------|----------------|--------|
| **#1533** | Extra conda package output directory | Add `--output-dir` flag for final packages. Well-scoped CLI addition. | Medium |
| **#1512** | Grouping of files in package output | UI improvement for file listing. | Medium |
| **#295** | Add JSON schema for variant files | Enables IDE autocompletion and validation. High user value. | Medium |
| **#1228** | Allow excluding test files from package | Add exclude pattern support for test files. | Medium |
| **#2112** | Write GitHub summary to separate file | Add `--summary-file` option alongside `GITHUB_STEP_SUMMARY`. | Medium |

---

## 4. Recipe Generation Issues (Cluster)

Multiple issues affect `generate-recipe`, suggesting a focused improvement sprint:

| # | Title | Type |
|---|-------|------|
| **#1543** | Bad license identifier (full text instead of SPDX) | Bug |
| **#1544** | Bad test imports (hyphen conversion) | Bug |
| **#1988** | Invalid SPDX format for BSD-3-Clause | Bug |
| **#1049** | Apache 2.0 vs Apache-2.0 | Bug |
| **#1071** | Support private PyPI index | Feature |

**Recommendation:** Address #1543, #1988, and #1049 together as they all relate to license identifier normalization. Issue #1544 is independent but similarly scoped.

---

## 5. Prefix Replacement Issues (Cluster)

A recurring theme across multiple issues:

| # | Title | Platform |
|---|-------|----------|
| **#2133** | Broken in Rust binaries | All |
| **#2147** | Broken for Python 3.11 (mtime) | All |
| **#101** | `install_name_tool` only removes first rpath | macOS |
| **#1955** | Duplicate RPATHs on macOS >= 15.4 | macOS |

**Recommendation:** These share a root cause area (prefix/rpath handling). A focused effort on the relinking and prefix replacement subsystem could resolve multiple issues simultaneously.

---

## 6. Platform-Specific Issues

### Windows
| # | Title |
|---|-------|
| **#1635** | MinGW lib existence tests missing `.dll.a` |
| **#1772** | Overlinking warnings against Python and system32 DLLs |
| **#2039** | CEP-28 customizable linking checks |
| **#1514** | Migration help for scikit-build |

### macOS
| # | Title |
|---|-------|
| **#1955** | Duplicate RPATHs on macOS >= 15.4 |
| **#1774** | `MACOSX_DEPLOYMENT_TARGET` not working in staged-recipes |
| **#101** | `install_name_tool` repeated invocation needed |

### Cross-compilation
| # | Title |
|---|-------|
| **#716** | Error using `--target-platform linux-64` on osx-arm64 |
| **#463** | Better support for cross-running tests |
| **#1784** | `CONDA_BUILD_CROSS_COMPILATION` always set to 1 |

---

## 7. Architecture & Design Discussions

These are longer-term items requiring design decisions:

| # | Title | Theme |
|---|-------|-------|
| **#2139** | CEP amendments (recipe format) | Recipe spec evolution |
| **#1959** | List of changes for recipe v2 | Recipe spec evolution |
| **#1604** | Matrix-style variants (no `zip_keys`) | Variant config redesign |
| **#203** | Reproducibility: control tools | Reproducible builds |
| **#261** | Compute and store content hash | Package integrity |
| **#1053** | Adding telemetry | Project telemetry |
| **#2132** | Sandboxing the file system | Build isolation |

---

## 8. Recommended Action Plan

### Immediate (This Sprint)
1. **Fix #2137** - `file_name` + `patches` regression (fix already referenced in issue)
2. **Fix #2110** - Top-level skip merging regression (test PR #2144 exists)
3. **Fix #1955** - Deduplicate RPATHs (straightforward fix)
4. **Fix #1984** - Allow empty string context variables

### Short-Term (Next 2 Sprints)
5. **Fix #2133** - Rust binary prefix replacement
6. **Fix #2147** - Python 3.11 prefix / mtime issue
7. **Fix recipe-gen cluster** - #1543, #1544, #1988, #1049 (license + import fixes)
8. **Add #2138** - Absolute symlink warning

### Medium-Term
9. Address prefix replacement subsystem holistically
10. Improve error messages (#1797, #2111, #774)
11. Add JSON schema for variant files (#295)
12. Tackle Windows platform issues cluster

---

## 9. Statistics

- **Oldest open issue:** #90 (2023-04-13) - `pin_run_as_build` variant file support
- **Newest issues:** #2147 (2026-02-13) and later
- **Issues with bug label:** 5 formally labeled (many more are de facto bugs)
- **Issues with enhancement label:** ~5 formally labeled
- **Issues in recipe-generation label:** ~4

**Note:** Many issues lack labels. A labeling pass would improve triage and discoverability.
