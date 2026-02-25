## [unreleased]

### 🚀 Features

- Improve cache and error message on hash mismatch (#2146)
- Skip symlink check for noarch packages with __unix dependency (#2167)
- Add configurable git submodule support to source cache (#2158)
- Implement CEP-28 customizable system DLL linkage checks for Windows (#2157)
- Warn when `target_platform` is set in variant config files (#2156)
- Powershell support  (#2149)
- Improve GitHub Actions output, add --markdown-summary and --log-style simple (#2155)
- *(py)* Infer recipe path output (#2166)
- Base platform selectors on host_platform instead of target_platform (#2174)
- Update all rattler crates to latest versions (#2189)
- Implement sigstore publisher verification of URL sources (#2148)
- Can use custom pypi indexes (#1492)
- Merge debug and debug-shell into unified debug command (#2171)

### 🐛 Bug Fixes

- Resolve 'is a directory' error when using file_name with patches (#2141)
- Direct compilation for `crates/rattler_build_recipe_generator` testing (#2076)
- Issue with native-tls on main (#2150)
- Do not always log entry points creation (#2152)
- Glob with `./` or `.` (#2151)
- Git lfs on windows (#2160)
- Include a build number in generated recipes (#2077)
- Duplicate rpath macos (#2161)
- Make cargo-fmt lefthook job detect formatting issues (#2169)
- Extract license from license_expression and classifiers in PyPI generator (#2153)
- Running of v0 tests and allow running from extracted directory (#2143)
- Source extraction issue with content-disposition (#2172)
- Accept YAML 1.1 boolean variants (True/False) (#2176)
- Move timestamp generation outside build output loop (#2187)
- Aws-lc-rs compilation on windows (#2185)
- Parse values as MatchSpec before extracting package name (#2175)
- Overdepending noise reduction (#2181)
- Prevent post-link scripts from leaking files into downstream packages (#2188)
- Improve Windows PE file detection and system library allowlisting (CEP 28) (#2173)
- Overlinking detection for host-only dependencies (#2204)
- Win pre-release build (aws-lc-sys compilation) (#2209)

### 🚜 Refactor

- Organize Rust code into crates and add new Python bindings (#2057)
- Expose miette errors for render Rust API (#2168)

### 📚 Documentation

- Adds cpu/cuda example for variants priority (#2145)
- Update CLI docs (#2180)
- Pin `click` dependency (#2195)
- Clarify `file_name` behavior and archive extraction in recipe docs (#2194)

### ⚙️ CI

- Publish pre-release conda packages (#2179)
- Cache release job with sccache (#2184)

### ⚙️ Miscellaneous Tasks

- *(ci)* Bump the github-actions group with 3 updates (#2134)
- Add test for toplevel skip merging (#2144)
- Switch submodule fork (#2170)
- *(ci)* Bump taiki-e/install-action from 2.67.26 to 2.67.30 in the github-actions group (#2164)
- *(ci)* Bump taiki-e/install-action from 2.67.30 to 2.68.8 in the github-actions group (#2202)
- Add regression test for conditional dependencies with variant variables (#2203)
- Update all dependencies (#2206)
## [0.57.2] - 2026-02-06

### 🐛 Bug Fixes

- Release 0.57.2 to build sigstore again (#2130)
## [0.57.1] - 2026-02-06

### 🐛 Bug Fixes

- Fixes to `publish` subcommand and make sigstore a feature (#2120)
- Lockfile not up to date for 0.57.1 (#2129)

### 📚 Documentation

- Add documentation for rattler-index (#2122)

### ⚙️ Miscellaneous Tasks

- Release 0.57.1 (#2121)
## [0.57.0] - 2026-02-05

### 🚀 Features

- Absolute license_file paths (#1947)
- Always compile in sigstore-sign (#2101)

### 🐛 Bug Fixes

- Move retry middleware to the front (#2091)
- Use bash -e for running the script manually (#2044)
- Markdown links in README.md (#2092)
- Include recipe extra section in `about.json` (#2106)
- Rebuild confirm panic in non-tty (#2115)
- Ensure we actually pass the concurrency limit in tool config (#2109)
- Create new files from patch (#2108)

### 💼 Other

- Ensure no rustls is used when using native-tls feature (#2113)
- [docs] Correct note on variants interacting with `run` dependencies (#2117)

### 📚 Documentation

- Clarify target_platform for aarch64 and arm64 (#2086)
- Change `build/run_exports` to `requirements/` (#2098)
- Delete Admonition about downstream tests not being implemented (#2105)
- Explicit doc for env vars (#2118)

### ⚙️ Miscellaneous Tasks

- *(ci)* Bump taiki-e/install-action from 2.65.6 to 2.65.13 in the github-actions group (#2079)
- Use rattler_index native/rustls-tls features (#2090)
- Use lefthook no_auto_install (#2054)
- Update to latest rattler (#2102)
- *(ci)* Bump taiki-e/install-action from 2.66.1 to 2.66.7 in the github-actions group (#2096)
- *(ci)* Bump the github-actions group across 1 directory with 4 updates (#2114)
- Use the same script everywhere for checking native-tls (#2116)
- Prepare release 0.57.0 (#2119)
## [0.55.1] - 2026-01-06

### 🐛 Bug Fixes

- Cpan recipe generation (#2081)

### 📚 Documentation

- Fix Jinja syntax in context section (#2069)
- Add sigstore attestation documentation (#2073)

### ⚙️ Miscellaneous Tasks

- *(ci)* Bump taiki-e/install-action from 2.64.0 to 2.65.1 in the github-actions group (#2068)
- Use flickzeug (diffy fork) and prepare release 0.55.1 (#2080)
## [0.55.0] - 2025-12-18

### 🚀 Features

- Speed up ci and move to `lzma-rust2` (#2040)
- Add tbd file parsing to find all allowed link paths on macOS (#2062)
- Remove macos-13 x64 build in favor of the universal wheel (#2066)

### 🐛 Bug Fixes

- Patch file not found should error (#2060)

### 📚 Documentation

- Add imprint and privacy policy (#2051)
- Move paxton to the sidebar, fix a few URLs (#2052)

### 🧪 Testing

- Add test to show windows script test errorlevel injection (#2065)

### ⚙️ Miscellaneous Tasks

- Fix CI for release, and use macos-latest (#2043)
- *(ci)* Bump the github-actions group with 3 updates (#2037)
- Update README banner image and link (#2049)
- Improve banner image in README (#2050)
- *(ci)* Bump the github-actions group with 4 updates (#2053)
- Prepare release 0.55.0 (#2067)
## [0.54.0] - 2025-12-08

### 🚀 Features

- Improve docs and render files that contain prefix with `[prefix:text]` or `[prefix:bin]` (#2031)
- Add deterministic per-package colors for span logs (#2032)

### 🐛 Bug Fixes

- *(docs)* Always download moranga (#2025)
- *(docs)* Social tag generation (#2026)

### 📚 Documentation

- *(build_script)* Use `%LIBRARY_BIN%` (#2022)
- Update documentation style and improve docs (#2013)
- Improve social preview for documentation (#2024)
- Add new docs page for build logs and env var json (#2030)
- Improve winget docs (#2034)
- More style improvements (#2036)

### ⚡ Performance

- Add custom memory allocator for improved performance (#2021)

### ⚙️ Miscellaneous Tasks

- *(ci)* Bump the github-actions group with 3 updates (#2023)
- Loosen `tracing-subscriber` bounds (#2033)
- Release 0.54.0 (#2038)
## [0.53.0] - 2025-11-27

### 🚀 Features

- Reduce noise in relinking (#2015)
- Better rendering for dependencies (#2016)
- Add `bump-recipe` subcommand (#2010)
- Add enhanced file listing with path checks and warnings (#1781)

### 🐛 Bug Fixes

- Clean up patch skipping (#2014)

### ⚙️ Miscellaneous Tasks

- Update rattler, prepare release 0.53.0 (#2017)
## [0.52.0] - 2025-11-26

### 🚀 Features

- Enable subcommand suggestions for mistyped commands (#2005)
- Add `package inspect` subcommand (#2004)
- Add `package extract` subcommand (#2006)

### 🐛 Bug Fixes

- Topological output sort (#2008)
- Create- vs generate attestation in `publish` (#2007)
- Clean on-disk cache after publishing package (#2009)

### ⚙️ Miscellaneous Tasks

- *(ci)* Bump the github-actions group across 1 directory with 4 updates (#2002)
- Organize build summary log (#2003)
- Update rattler dependencies (#2001)
## [0.51.0] - 2025-11-21

### 🚀 Features

- Add `publish` subcommand (#1981)

### 🐛 Bug Fixes

- Revert filtering `.git` dirs for path source (#1996)
- Reduce verbosity of rpath conversion logging (#1997)
- Reindex output directory after moving package to broken folder (#1998)

### 📚 Documentation

- Add debugging docs (#1993)
- Fix up `ignore_binary_files` default (double negation) (#1995)

### ⚙️ Miscellaneous Tasks

- Update rattler (#1992)
- Release 0.51.0 (#1999)
## [0.50.0] - 2025-11-17

### 🚀 Features

- Add `debug-shell` subcommand and improve `create-patch` (#1990)
- Add `expected_commit` verification to git sources, update docs (#1978)

### 🐛 Bug Fixes

- Don't set referer header when downloading sources (#1964)
- Variant key in context should be ignored in output (#1963)
- Allow `null`/empty values for `noarch` field (#1973)
- .git* files not included (#1983)
- Avoid using corrupted git cache dirs (#1987)
- Allow all libraries in /usr/lib for macOS overlinking checks (#1977)
- Add helpful error messages for invalid root-level field names (#1974)

### 📚 Documentation

- Add documentation for the `match()` Jinja function (#1976)

### ⚙️ Miscellaneous Tasks

- *(ci)* Bump taiki-e/install-action from 2.62.38 to 2.62.45 in the github-actions group (#1969)
- Bump to 0.50.0 (#1991)
## [0.49.0] - 2025-10-28

### 🚀 Features

- Add cargo-deny (#1929)
- Make s3 support optional (#1950)

### 🐛 Bug Fixes

- Flaky test on windows (#1936)
- Diffy patch application for pure renames, timestamp rendering and release 0.49.0 (#1962)

### 🚜 Refactor

- Split metadata.rs into multiple files under `types/` (#1932)

### ⚙️ Miscellaneous Tasks

- Use rattler-sandbox as an external tool (#1921)
- Remove `UrlWithTrailingSlash` (#1930)
- *(ci)* Bump the github-actions group with 2 updates (#1935)
- *(ci)* Bump the github-actions group with 2 updates (#1948)
- *(ci)* Bump the github-actions group with 3 updates (#1958)
## [0.48.1] - 2025-10-08

### 🚀 Features

- Use `pixi-build` to build the Rust - Python bindings (#1925)

### ⚙️ Miscellaneous Tasks

- Release 0.48.1 (#1927)
## [0.48.0] - 2025-10-07

### 🚀 Features

- Add tracing logs for generating recipe file backup and write operations (#1888)
- Amend run exports from `run_exports.json` and other sources (#1795)
- *(bindings)* Add recipe parsing in python bindings (#1872)

### 🐛 Bug Fixes

- Disable run_exports extraction in render-only solves (#1883)
- Simplify `load_conda_build_config` (#1901)
- Return proper error instead of unwrapping (#1904)
- Improve error message when pin cannot be applied (#1906)
- Change `--force-path-style` to `--addressing-style` in test and docs (#1913)
- Skip uninterpretable PE files during relinking (#1908)
- Ordering of middleware (#1911)
- Fix copying .git folders by using overrides (#1838)

### 🚜 Refactor

- Move recipe generation into it's own crate (#1912)

### 📚 Documentation

- Fix testing docs (#1885)
- Update testing docs with env and file (#1886)
- Improve variants docs and fix warnings (#1899)

### ⚙️ Miscellaneous Tasks

- Don't autofix typos (#1878)
- *(ci)* Bump the github-actions group with 5 updates (#1881)
- Upgrade zip, crossterm and rattler dependencies to latest versions (#1890)
- *(ci)* Bump the github-actions group with 2 updates (#1889)
- *(ci)* Bump the github-actions group with 2 updates (#1893)
- *(ci)* Bump the github-actions group with 4 updates (#1907)
- Update rattler (#1915)
- *(ci)* Bump the github-actions group with 3 updates (#1914)
- Prepare release 0.48.0 (#1919)
## [0.47.1] - 2025-09-04

### 🐛 Bug Fixes

- Enhance `zip_keys` parsing to detect flat lists (#1874)
- Extract both arguments from `match` function (#1875)

### 📚 Documentation

- Add flag arg to rebuild.md (#1873)

### ⚙️ Miscellaneous Tasks

- Release 0.47.1 (#1876)
## [0.47.0] - 2025-09-03

### 🚀 Features

- Print environment info when externally managed instead of early return (#1829)
- Add variant override CLI and test (#1841)
- Enhance create_patch to skip existing identical patches and apply incremental changes (#1743)
- Variant config with `--variant` cli flags, docs, python bindings (#1846)
- Add python bindings recipe generators for CPAN, CRAN, and LuaRocks (#1862)
- Improvements for the rebuild subcommand (#1863)

### 🐛 Bug Fixes

- Pixi build warnings (#1842)
- Include symlink even outside (#1853)
- Patch application with `.orig` / `.bak` files (#1866)
- Run clippy with all features (#1869)

### 🚜 Refactor

- Replace upload module with 'rattler_upload' crate (#1782)

### ⚙️ CI

- Reduce ci times by not running all builds everytime (#1843)

### ⚙️ Miscellaneous Tasks

- *(ci)* Bump the github-actions group with 3 updates (#1850)
- Stage fixes from cargo-fmt (#1859)
- Add original license to error (#1855)
- Update dependencies and release 0.47.0 (#1867)
## [0.46.0] - 2025-08-20

### 🚀 Features

- Add `use-sharded` and `use-jlap` flags (#1831)
- Parallelize post-processing with rayon (#1826)
- Automatic error_level check for cmd.exe when using a list of strings as script (#1835)

### 🐛 Bug Fixes

- Use relative path for binary relocation glob match (#1815)
- Allow preserving the working directory (#1813)
- Sanitize hyphenated package names in python test imports (#1814)
- Allow clippy lint for windows readonly permission (#1817)
- Skip malformed PE files during relinking (#1818)
- Search for forward slash path on Windows, even for noarch packages (#1836)

### 📚 Documentation

- Add the link to build script page from recipe page. (#1834)

### ⚙️ Miscellaneous Tasks

- *(ci)* Bump taiki-e/install-action from 2.57.1 to 2.57.6 in the github-actions group (#1811)
- *(ci)* Bump the github-actions group with 2 updates (#1821)
- *(ci)* Bump the github-actions group with 4 updates (#1832)
- Update all dependencies and rattler (#1828)
- Prepare release 0.46.0 (#1839)
## [0.45.0] - 2025-08-04

### 🚀 Features

- Improve patch creation logging and ensure colored diff output (#1744)
- Add support for patch-test-extra feature in tests and configuration (#1746)
- Enhance patch creation error handling and logging (#1747)
- Delimit case-insensitive file matches with newline-space-hyphen instead of comma-space (#1757)
- Add `exclude-newer` parameter (#1759)
- Use actual filename from content-disposition when downloading source code (#1745)
- Support for `ruby` and `nodejs` as interpreters (#1767)
- Bump rattler to latest (#1769)
- *(extract)* Extract 7z files (#1779)
- Add option to mark environments as externally managed (#1790)
- Bump rattler to latest (#1805)

### 🐛 Bug Fixes

- Add more unit tests and simplify normalize path logic (#1760)
- Add missing parameters to Python bindings (#1766)
- Ignore object files when relinking (#1763)
- Ensure package name is defined in the pixi.toml (#1802)

### 📚 Documentation

- *(README)* Remove outdated requirement on `patch` command (#1777)
- Fix note syntax (#1808)

### 🧪 Testing

- Add unit tests for environment variable handling and checksum validation (#1729)

### ⚙️ Miscellaneous Tasks

- *(ci)* Bump the github-actions group with 4 updates (#1761)
- Switch to `pixi-build-rust` (#1762)
- *(ci)* Bump the github-actions group across 1 directory with 2 updates (#1776)
- *(ci)* Bump the github-actions group with 2 updates (#1787)
- Switch to lefthook from pre-commit (#1793)
- *(ci)* Bump the github-actions group with 2 updates (#1803)
- Release 0.45.0 (#1806)
## [0.44.0] - 2025-06-24

### 🚀 Features

- Add validation for missing license files and glob patterns in recipes (#1727)
- Add support  for `pixi build` (#1716)
- Patch application without git (#1676)
- Basic create-patch functionality using `diffy` (#1728)

### 🐛 Bug Fixes

- Symlinked directories (#1737)
- Use `rattler_config` crate instead of `pixi_config` and update rattler and other dependencies (#1731)
- Use `fs_err` for invalid source path (#1741)

### 💼 Other

- Recipe generators for `CPAN` and `luarocks` (#1726)

Co-authored-by: Bas Zalmstra <bas@prefix.dev>

### 🚜 Refactor

- Remove `license_url` from package specification and related structures (#1732)

### 📚 Documentation

- Alternatives to selectors for scalar fields (#1735)

### ⚡ Performance

- Improve build option parsing (#1719)

### ⚙️ Miscellaneous Tasks

- *(ci)* Bump the github-actions group with 2 updates (#1718)
- *(ci)* Bump the github-actions group across 1 directory with 5 updates (#1734)
- Bump diffy (#1738)
## [0.43.1] - 2025-06-05

### 🐛 Bug Fixes

- Win git rev (#1711)

### ⚙️ Miscellaneous Tasks

- Bump to 0.43.1 (#1712)
## [0.43.0] - 2025-06-05

### 🚀 Features

- Disable .ignore files in source copying (#1696)
- Add support for .rattlerbuildignore files in source copying (#1697)
- Add strict mode for package content tests to enforce file matching (#1677)
- Log git errors (#1691)
- Serialize `extra_meta` when it is not none so that can rebuild perfectly (#1707)
- Add support for `exists` and `not_exists` synonyms in glob vector parsing and enhance tests (#1669)
- Implement binary prefix detection behavior in packaging (#1658)

### 🐛 Bug Fixes

- Fix up error message (#1698)
- Respect case sensitivity of filesystem when collecting new files (#1699)

### 📚 Documentation

- Add security policy (#1692)

### ⚙️ Miscellaneous Tasks

- *(ci)* Bump taiki-e/install-action from 2.51.2 to 2.52.1 in the github-actions group (#1695)
- *(ci)* Bump taiki-e/install-action from 2.52.1 to 2.52.4 in the github-actions group (#1703)
- Bump version to 0.43.0 (#1709)
## [0.42.1] - 2025-05-20

### 🚀 Features

- Add startingwith to jinja whitelist (#1679)

### 🐛 Bug Fixes

- Set file permissions after copying directories (#1682)
- `git apply` inside a subdirectory of a git repository (#1675)

### 📚 Documentation

- Need git to checkout repo (#1683)
- Fix typo in recipe spec (#1687)

### ⚙️ CI

- Build for aarch64-pc-windows-msvc (#1674)

### ⚙️ Miscellaneous Tasks

- *(ci)* Bump the github-actions group with 2 updates (#1678)
- Release 0.42.1 (#1686)
## [0.42.0] - 2025-05-16

### 🚀 Features

- Add `rscript` interpreter and R test (#1586)
- Support for loading files relative to recipe path (#1607)
- Add test for building recipes with relative Git source paths (#1620)
- Add `unicode` normalization for path handling and detect case-insensitive filename collisions (#1621)
- Add interpreter detection for various script types (#1618)
- Add `--continue-on-failure` flag (#1626)
- Add support for building packages with spaces in paths in windows (#1654)
- Improve `debug` mode and add it to `test` subcommand (#1640)

### 🐛 Bug Fixes

- Add a default constructor for the baseclient (#1603)
- Move generate-cli-docs to `docs/generator` (#1609)
- Ignore python more for abi3 recipes (#1610)
- Make conda recognize rattler-build environments (#1614)
- Test for merging build and host and make sure `BUILD_PREFIX == HOST_PREFIX` (#1629)
- Mixed line ending (CRLF vs LF) issues with patches (#1627)
- Patch parsing fixes (#1641)
- Make `relink` on linux recognize `${ORIGIN}` (#1647)
- Pre-commit with pixi (#1662)
- Reject unnormalized package name and normalize index name (#1660)
- Tests and remove nushell (#1663)
- Do not leak credentials in about.json and rendered_recipe.yaml (#1637)
- Try to make test on Windows less flaky (#1665)
- Do not error when `--recipe-dir` does not contain any recipes (#1666)
- Copy over creation time metadata (#1661)

### 🚜 Refactor

- Change `PackageInfo` fields to use `Option` types for optional binaries and pkgdocs (#1625)

### 📚 Documentation

- Add an R tutorial and update Python tutorial with abi3 (#1642)
- More docs for testing and repackaging existing software (#1645)
- Mention `feedrattler` (#1644)
- Do not need tar on host system (#1659)

### ⚙️ Miscellaneous Tasks

- *(ci)* Bump the github-actions group with 2 updates (#1619)
- Update python bindings & parameters (#1630)
- *(ci)* Bump taiki-e/install-action from 2.50.7 to 2.50.10 in the github-actions group (#1650)
- Release 0.42.0 without bumping rattler (#1668)
## [0.41.0] - 2025-04-30

### 🚀 Features

- Add support for insecure SSL connections in reqwest client (#1528)
- Validate python_version specs in recipe parser (#1564)
- Better patch parser (#1565)
- Introduce debug mode, custom outputs (#1566)
- Add Windows support for DLL linking and validation (#1559)
- Add CarriageReturnToNewline wrapper for async reading (#1575)
- Optimize CarriageReturnToNewline for better performance with buffer processing (#1579)
- Read channels and package format from pixi config (#1563)
- Implement retry logic for directory deletion on Windows (#1589)
- Add s3 and mirror configuration from pixi config (#1593)
- Add log messages for patches applied (#1599)
- Read `channel_sources` from variant file (#1597)

### 🐛 Bug Fixes

- Add test and fix for bad package versions with hatch-vcs (#1570)
- Update lockfiles (#1573)
- Add `PWD` to env vars to fix some weird issues with conda-bash (#1578)
- More `VersionWithSource` in additional places (#1572)
- Properly link check on Windows (#1598)

### 📚 Documentation

- Correct indentation for C++ recipe in multi_output_cache.md (#1567)
- Fix s3 docs for force-path-style (#1596)
- Update python test docs (#1601)

### ⚙️ Miscellaneous Tasks

- *(ci)* Bump the github-actions group with 3 updates (#1574)
- Remove outdated URL from SSL test recipe (#1577)
- Bump rust to 1.86.0 (#1582)
- Update to Rust Edition 2024 (#1583)
- *(ci)* Bump the github-actions group with 3 updates (#1590)
- Update dependencies and release 0.41.0 (#1602)
## [0.40.0] - 2025-04-16

### 🚀 Features

- Add output name to finalized run dependencies (#1487)
- Skip upload existing package to prefix (#1501)
- Add `io_concurrency_limit` support to prevent resource exhaustion (#1489)
- Add `source.filter` to `PathSource` (#1545)
- Add error handling for hyphen in context variables (#1557)
- Emscripten build outputs will be produced with .js and .wasm extensions (#1558)

### 🐛 Bug Fixes

- Glob folders from simple name (#1067)
- *(docs)* Recipe.yaml uses {upper, lower}_bound instead of {max, min}_pin (#1526)
- Bump rattler remove zip fork (#1532)
- *(parser)* Enhance package name extraction logic in TryConvertNode (#1529)
- Filter path source files (#1549)
- Simplify lifetimes (#1550)
- Remove test folder (#1552)

### 🚜 Refactor

- Improve stack handling by spawning a dedicated thread (#1551)

### 📚 Documentation

- Document prefix patching options (#1484)

### ⚙️ CI

- Pin github actions (#1495)

### ⚙️ Miscellaneous Tasks

- *(ci)* Bump taiki-e/install-action from 2.49.28 to 2.49.29 (#1497)
- *(ci)* Bump taiki-e/install-action from 2.49.29 to 2.49.30 (#1500)
- *(ci)* Bump actions/download-artifact from 4.1.9 to 4.2.0 (#1499)
- *(ci)* Bump actions/download-artifact from 4.2.0 to 4.2.1 (#1502)
- *(ci)* Bump actions/upload-artifact from 4.6.1 to 4.6.2 (#1503)
- *(ci)* Bump Swatinem/rust-cache from 2.7.7 to 2.7.8 (#1504)
- *(ci)* Bump PyO3/maturin-action from 1.47.2 to 1.47.3 (#1505)
- *(ci)* Bump taiki-e/install-action from 2.49.30 to 2.49.34 (#1508)
- *(ci)* Bump taiki-e/install-action from 2.49.34 to 2.49.35 (#1511)
- *(ci)* Bump actions/setup-python from 5.4.0 to 5.5.0 (#1510)
- *(ci)* Bump taiki-e/install-action from 2.49.35 to 2.49.40 (#1523)
- *(ci)* Bump prefix-dev/setup-pixi from 0.8.3 to 0.8.4 (#1522)
- *(ci)* Bump taiki-e/install-action from 2.49.40 to 2.49.44 (#1527)
- *(ci)* Bump zgosalvez/github-actions-ensure-sha-pinned-actions from 3.0.22 to 3.0.23 (#1520)
- *(ci)* Bump PyO3/maturin-action from 1.47.3 to 1.48.1 (#1531)
- *(ci)* Bump taiki-e/install-action from 2.49.44 to 2.49.45 (#1530)
- *(ci)* Bump taiki-e/install-action from 2.49.45 to 2.49.47 (#1541)
- *(ci)* Bump PyO3/maturin-action from 1.48.1 to 1.49.1 (#1537)
- *(ci)* Bump prefix-dev/setup-pixi from 0.8.4 to 0.8.5 (#1536)
- Group dependabot updates and run only weekly (#1547)
- Bump rattler (#1553)
- *(ci)* Bump taiki-e/install-action from 2.49.47 to 2.49.49 in the github-actions group (#1556)
- Release 0.40.0 and bump all dependencies (#1560)
## [0.39.0] - 2025-03-12

### 🐛 Bug Fixes

- Properly raise error if macOS `codesign` - ing fails (#1479)
- MacOS relink 'libfoo.dylib' as '@rpath/libfoo.dylib' (#1477)

### 🚜 Refactor

- Align CLI for S3 with rattler-index (#1482)

### ⚙️ Miscellaneous Tasks

- Update all dependencies, prepare 0.39.0 (#1483)
## [0.38.0] - 2025-03-05

### 🐛 Bug Fixes

- Preserve macOS entitlements and requirements in codesign (#1461)
- Run exports parsing as a list (#1469)

### ⚙️ Miscellaneous Tasks

- *(ci)* Bump uraimo/run-on-arch-action from 2 to 3 (#1462)
- Bump to 0.38.0 (#1470)
## [0.37.0] - 2025-02-28

### 🚀 Features

- Add S3 upload (#1299)
- Support lists in `context` (#1289) (#1402)
- Menuinst schema check, rattler update (#1453)

### 📚 Documentation

- Fix typo (#1450)
- Fix package_contents tests in xtensor example (#1452)
- Fix docs on overlinking and overdepending (#1448)

### ⚙️ Miscellaneous Tasks

- *(ci)* Bump prefix-dev/setup-pixi from 0.8.2 to 0.8.3 (#1442)
- Release 0.37.0 (#1456)
## [0.36.0] - 2025-02-18

### 🚀 Features

- Jinja / recipe variable types (#1385)
- Right align size in solve table (#1399)
- Attach file name to source code in error message (#1404)
- Use `multipart/form` and allow uploading attestations to prefix.dev (#1370)
- Check if pypi is noarch (#1413)

### 🐛 Bug Fixes

- Update lock file of python bindings (#1419)
- Prepend path for build (#1429)
- Strict typing for variants (#1415)

### 📚 Documentation

- Improve python binding docs (#1397)
- Add parameter table for each function (#1403)
- Update CLI reference (#1411)
- Remove schema link (#1408)
- Update mkdocs dependencies (#1428)

### ⚙️ Miscellaneous Tasks

- Update all dependencies (#1401)
- Update all the dependencies (#1406)
- Update dependencies of rattler-build and py-rattler-build (#1420)
- *(ci)* Bump prefix-dev/setup-pixi from 0.8.1 to 0.8.2 (#1426)
- Introduce stricter lints (#1421)
- Release 0.36.0 (#1433)
## [0.35.9] - 2025-01-29

### 💼 Other

- Move back to Python 3.8 (#1395)
## [0.35.8] - 2025-01-29

### 🐛 Bug Fixes

- Prepare for patch release (#1384)

### 💼 Other

- Specify p39 for abi (#1387)
## [0.35.7] - 2025-01-28

### 🚀 Features

- Simplify code and improve error message when parsing spec from variant (#1365)
- Add missing `is boolean` test to Jinja (#1366)
- Add more py-rattler-build options (#1359)
- Expose more functions in rattler-build-python (#1369)
- Expose upload commands for Python bindings (#1371)
- Write package to temporary file, then persist to final name (#1372)

### 🐛 Bug Fixes

- Pypi url formatting (#1373)

### 📚 Documentation

- Python bindings (#1380)

### ⚙️ CI

- Add tbump in order to automate version bumping (#1368)

### ⚙️ Miscellaneous Tasks

- Add `rust-src` dependency (#1374)
- Bump to 0.35.7 (#1381)
## [0.35.6] - 2025-01-20

### 🐛 Bug Fixes

- Upload with retry (#1360)
- Removed jinja variable in `build.script.content` (#1353)
- Use python key, and improve skip output (#1361)
- Use stable URL for PyPI packages if possible (#1362)

### 🚜 Refactor

- Make source generic (#1352)

### 📚 Documentation

- Hide deprecated `--no-test` (#1358)
- Fix typo (#1357)
## [0.35.5] - 2025-01-18

### 🐛 Bug Fixes

- Empty cbc (#1351)
## [0.35.4] - 2025-01-18

### 🐛 Bug Fixes

- Keep tempdir to make recipe rendering from stdin work again (#1350)
## [0.35.3] - 2025-01-17

### 🚀 Features

- Add retry for run export (#1349)
## [0.35.2] - 2025-01-17

### 💼 Other

- Release 0.35.2 (#1347)
## [0.35.1] - 2025-01-17

### 🚀 Features

- Improve docs, variant discovery logic, and regex replacement test (#1344)

### ⚙️ Miscellaneous Tasks

- Release 0.35.1 (#1345)
- Update py-rattler-build as well (#1346)
## [0.35.0] - 2025-01-16

### 🚀 Features

- Add retry logic to upload functions (#1330)
- Continue py-rattler-build (#1326)
- Implement CEP-20 for Python ABI3 packages (#1320)
- Conda build config parser (#1334)
- Improve recipe generation (#1340)

### 🐛 Bug Fixes

- Allow schema version toplevel (#1332)

### 💼 Other

- Remove slice and batch filters, document slicing (#1323)

### ⚙️ CI

- Improve python bindings workflow (#1325)
- Move to pre-commit (#1333)

### ⚙️ Miscellaneous Tasks

- Use latest rust (#1322)
- Small source dl cleanup (#1337)
- Prepare release 0.35.0 (#1339)
## [0.34.1] - 2025-01-09

### 🚀 Features

- Improve recipe gen (#1313)

### 🐛 Bug Fixes

- Sandbox --network and disable on ppc64le (#1319)

### 📚 Documentation

- Add sandbox docs (#1314)
- Improve CLI reference (#1315)
## [0.34.0] - 2025-01-08

### 🚀 Features

- Remove rip and use pypi json API instead (#1310)
- Add experimental sandbox during builds (#1178)

### 🐛 Bug Fixes

- Expose `NormalizedKey` (#1307)

### ⚙️ Miscellaneous Tasks

- Release 0.34.0 (#1312)
## [0.33.3] - 2025-01-06

### 🐛 Bug Fixes

- Clean prefix, and restore pristine cache state (#1300)

### ⚙️ Miscellaneous Tasks

- Prepare release 0.33.3 (#1302)
## [0.33.2] - 2025-01-03

### 🐛 Bug Fixes

- Windows forward slash replacement (#1296)

### ⚙️ Miscellaneous Tasks

- *(ci)* Bump crate-ci/typos from 1.28.4 to 1.29.0 (#1294)
- Release 0.32.2 (#1298)
## [0.33.1] - 2024-12-23

### 🚀 Features

- Speed up prefix record loading (#1278)

### 🐛 Bug Fixes

- "the env Jinja functions" example not to ref PKG_HASH (#1279)
- Use tool_configuration.channel_priority in TestConfiguration (#1281)
- Use `UrlWithTrailingSlash` for upload, use bearer auth for Artifactory upload (#1280)

### ⚙️ Miscellaneous Tasks

- Update to latest rattler (#1282)
- Prepare release 0.33.1 (#1283)
## [0.33.0] - 2024-12-17

### 🐛 Bug Fixes

- Variant issue with `__unix` (#1272)
- Stricter input parsing and more lenient parsing of run exports from other packages (#1271)

### ⚙️ CI

- Ignore typos patch updates (#1269)

### ⚙️ Miscellaneous Tasks

- *(ci)* Bump crate-ci/typos from 1.28.3 to 1.28.4 (#1268)
- Update rattler and new rust toolchain (#1273)
- Prepare release 0.33.0 (#1274)
## [py-rattler-build-v0.1.0] - 2024-12-13

### 🚀 Features

- Add python release workflow (#1257)

### ⚙️ Miscellaneous Tasks

- *(ci)* Bump crate-ci/typos from 1.28.2 to 1.28.3 (#1256)
## [0.32.1] - 2024-12-12

### 🐛 Bug Fixes

- Typo in comment in Perl interpreter (#1253)
- Use the correct prefix for `os_vars` in test (#1255)
## [0.32.0] - 2024-12-10

### 🚀 Features

- Upgrade middleware and rip (#1219)
- Initialize rattler-build python bindings (#1221)
- Add `--channel-priority={strict|disabled}` (#1211)
- Refactor script and add `perl` as an interpreter (#1229)
- Add a `perl` test section (#1230)
- Expose python as struct (#1233)
- Cache source code too (#1226)
- Add more docs for the multi-output cache (#1235)
- Keep installed packages in environments (#1119)
- New variant resolving / rendering  (#1122)
- Use index map for env (#1251)

### 🐛 Bug Fixes

- Resolve file contents properly in test section (#1214)
- Reindex output cache after build (#1209)
- Create intermediate directories as well (#1225)
- Allow relinking on read-only files (#1231)
- Use read timeout, not global timeout (#1239)
- License from recipe folder instead of erroring if both are found (#1243)
- Invert pyc filter to check against old files and include all new `.pyc` files (#1246)
- Issues with new variant resolving and resolution (#1250)

### 📚 Documentation

- Fix experimental env (#1218)
- Fix wrong key name in doc (#1216)
- Mention automatic file extension in output/build/script (#1222)
- Add some perl docs (#1234)
- Fix another incorrect variant_config key in docs (#1237)

### ⚙️ Miscellaneous Tasks

- *(ci)* Bump crate-ci/typos from 1.27.3 to 1.28.0 (#1215)
- *(ci)* Bump crate-ci/typos from 1.28.0 to 1.28.1 (#1220)
- *(ci)* Bump crate-ci/typos from 1.28.1 to 1.28.2 (#1232)
- Prepare release 0.32.0 (#1252)
## [0.31.1] - 2024-11-21

### 🐛 Bug Fixes

- Never use parent `.gitignore` files when copying files

### ⚙️ Miscellaneous Tasks

- Use the repository instead of the homepage field in Cargo.toml (#1206)
- Prepare release 0.31.1 (#1207)
## [0.31.0] - 2024-11-18

### 🚀 Features

- Use gitignore when copying the recipe files (#1193)
- Skip noarch build if `--noarch-build-platform` != `build_platform` (#1192)
- Introduce `python_version` in `tests[0].python` (#1170)
- Add `--test={skip|native|native-and-emulated}` (#1190)

### 💼 Other

- Prepare release 0.31.0 (#1197)

### 📚 Documentation

- Add trusted publishing support (#1194)
- Remove 'pip install' options in recipes (#1198)
## [0.30.0] - 2024-11-13

### 🚀 Features

- Add mold for linux (#1033)
- Add oidc trusted publisher support for uploads to prefix (#1181)

### 🐛 Bug Fixes

- Url source extraction for file:/// urls (#1164)

### 💼 Other

- Prepare release 0.30.0 (#1189)

### 📚 Documentation

- Add `go` tutorial (#1032)

### ⚙️ Miscellaneous Tasks

- *(ci)* Bump crate-ci/typos from 1.27.0 to 1.27.2 (#1169)
- *(ci)* Bump crate-ci/typos from 1.27.2 to 1.27.3 (#1177)
## [0.29.0] - 2024-11-05

### 🚀 Features

- Implement CEP-17 and prepare release (#1161)

### 🐛 Bug Fixes

- Multiline script render (#1156)
- Relinking issues on macOS (#1159)

### ⚙️ Miscellaneous Tasks

- *(ci)* Bump crate-ci/typos from 1.26.8 to 1.27.0 (#1158)
- Add a test for the relinking fixes (#1160)
## [0.28.2] - 2024-10-29

### 🐛 Bug Fixes

- Missing recipe_dir.join() for relative git paths (#1140)
- Make default wrap log lines to None so that CI detection works (#1143)
- Add `BufReader` for zip extraction (#1144)

### ⚙️ Miscellaneous Tasks

- Prepare release 0.28.2 (#1145)
## [0.28.1] - 2024-10-26

### 🐛 Bug Fixes

- Used var detection for `match` function (#1128)
- Add shebang to bash preamble (#1132)
- Some regressions with pretty-printing and add no-wrap printing (#1136)

### ⚙️ Miscellaneous Tasks

- *(ci)* Bump crate-ci/typos from 1.26.0 to 1.26.8 (#1135)
- Prepare release 0.28.1 (#1137)
## [0.28.0] - 2024-10-21

### 🐛 Bug Fixes

- Npy version issue (#1126)

### ⚙️ Miscellaneous Tasks

- Release 0.28.0 (#1127)
## [0.27.0] - 2024-10-16

### 🚀 Features

- Extract run exports using object (#1114)
- Improve path display and replacement during script execution (#1115)

### 🐛 Bug Fixes

- Doc invalid pin expressions example (#1113)
- Dot in variant when exporting to environment (#1121)

### 💼 Other

- Insert `LIB` and `INCLUDE` (#1118)

### ⚙️ Miscellaneous Tasks

- Prepare release 0.27.0 (#1123)
## [0.26.0] - 2024-10-11

### 🚀 Features

- Add `@echo on` for windows(#1106)
- Add virtual packages to platforms (#1108)

### 🐛 Bug Fixes

- Re-enable rendering in test script section (#1091)
- Running python test cross-platform (#1110)
- Canonicalize the output directory before checks (#738)
- Copy permissions when reflinking files (#1111)

### ⚙️ Miscellaneous Tasks

- Prepare release 0.26 (#1112)
## [0.25.0] - 2024-10-09

### 🚀 Features

- Update minijinja to 2.x (#1095)

### 🐛 Bug Fixes

- Correct `PackageContentsTest` glob prefix on windows (#1094)
- Dashes / underscores in variants and env variables (#1096)

### ⚙️ Miscellaneous Tasks

- *(ci)* Bump crate-ci/typos from 1.25.0 to 1.26.0 (#1092)
- Release 0.25.0 (#1104)
## [0.24.0] - 2024-10-08

### 🚀 Features

- Add `build` argument for pin expressions (#1086)
- Support setting `host_platform` explicitly (#1087)

### 🐛 Bug Fixes

- Use proper `host_platform` when testing `noarch` packages (#1085)
- Checking out git branches that were already cloned (#1070)
- Improve env var handling (#1088)
- Avoid fetching all tags on git clone/fetch (#1089)

### ⚙️ Miscellaneous Tasks

- Update dependencies, prepare release 0.24.0 (#1090)
## [0.23.0] - 2024-10-02

### 🚀 Features

- Include `git` output error context on git failures (#1079)

### 🐛 Bug Fixes

- Actually install packages for `cache` (#1078)
- Detect virtual packages from environment (#1081)

### ⚙️ Miscellaneous Tasks

- *(ci)* Bump crate-ci/typos from 1.24.6 to 1.25.0 (#1074)
- Prepare 0.23.0 release (#1082)
## [0.22.0] - 2024-09-25

### 🚀 Features

- Separate solving from installing (#1030)
- Print detected virtual packages for debugging purposes (#1059)
- Delay jinja evaluation for script (#894)

### 🐛 Bug Fixes

- Latest_tag get oldest tag (#1062)
- Disallow dash in version (#1065)

### ⚙️ Miscellaneous Tasks

- *(ci)* Bump crate-ci/typos from 1.24.3 to 1.24.4 (#1044)
- *(ci)* Bump crate-ci/typos from 1.24.4 to 1.24.5 (#1046)
- *(ci)* Bump crate-ci/typos from 1.24.5 to 1.24.6 (#1061)
- Add a debug message when prefixes are not matching (#1058)
- Update dependencies (#1066)
- Prepare 0.22.0 release (#1069)
## [0.21.0] - 2024-09-03

### 🐛 Bug Fixes

- CLI docs (#1036)
- Ignore used vars for run dependencies (#1037)

### 🚜 Refactor

- Make build string optional in recipe (#1020)

### ⚙️ Miscellaneous Tasks

- *(ci)* Bump crate-ci/typos from 1.23.6 to 1.23.7 (#1029)
- *(ci)* Bump crate-ci/typos from 1.23.7 to 1.24.1 (#1031)
- *(ci)* Bump crate-ci/typos from 1.24.1 to 1.24.3 (#1035)
- Prepare release 0.21.0 (#1041)
## [0.20.0] - 2024-08-16

### 🚀 Features

- Add --extra-meta (#1019)

### 🐛 Bug Fixes

- Fix typo of feature recipe-generation (#1006)

### 💼 Other

- Typo of BasicHTTP result in json decode error (#1007)
- Rattler crates (#1018)
- 0.20.0 (#1021)

### 📚 Documentation

- Update numpy recipe in Python examples page (#1015)
- Clarify section on `run_exports` (#1013)

### ⚙️ CI

- Run `clippy` on the whole workspace (#1014)
- Always run on push and PR (#1016)
## [0.19.0] - 2024-08-01

### 🚀 Features

- Make recipe generation optional (#996)
- Expose fields needed for building an output directly (#997)

### 🐛 Bug Fixes

- Replace absolute symlinks from cache with correct version (#993)
- Filter cache run_exports with the right ignores (#989)

### 🚜 Refactor

- Parse version as version (#1001)

### ⚙️ Miscellaneous Tasks

- *(ci)* Bump crate-ci/typos from 1.23.3 to 1.23.5 (#990)
- *(ci)* Bump crate-ci/typos from 1.23.5 to 1.23.6 (#999)
- Prepare `0.19.0` and update all dependencies (#1002)
## [0.18.2] - 2024-07-25

### 🚀 Features

- Improvements to the R generator (#953)
- Add nushell completions (#966)
- Error when no recipe paths are detected (#968)
- Error when `recipe-dir` is not a directory (#969)
- Use implicit variants.yaml (#980)
- Include extra field in rendered recipe (#986)

### 🐛 Bug Fixes

- Update build.nu (#950)
- Cran recursive recipe generation fixes (#961)
- Sort packages (#987)

### 📚 Documentation

- Add docs for recent feature additions (#957)
- Fix markdown list (#973)
- Improve jinja documentation (#975)

### ⚙️ CI

- Check for typos with typos (#962)

### ⚙️ Miscellaneous Tasks

- Update images in the readme (#958)
- Update readme (#959)
- *(ci)* Bump crate-ci/typos from 1.22.9 to 1.23.1 (#963)
- Fix clippy warnings (#967)
- *(ci)* Bump crate-ci/typos from 1.23.1 to 1.23.2 (#965)
- *(ci)* Bump crate-ci/typos from 1.23.2 to 1.23.3 (#982)
- Use tempfile create instead of implementing it ourselves (#983)
- Prepare release 0.18.2 (#988)
## [0.18.1] - 2024-06-26

### 🚀 Features

- Add more env vars when testing (`PKG_NAME`, `PKG_VERSION`...) (#943)
- Implement downstream test (#942)
- Replace the prefix in cached files (#945)
- Improve R recipe generator (#949)

### 🐛 Bug Fixes

- Fix a serialization issue where a single-line script and cwd are set (#946)

### ⚙️ Miscellaneous Tasks

- Prepare release 0.18.1 (#947)
## [0.18.0] - 2024-06-19

### 🚀 Features

- Add commonly used go compilers to default compiler logic (#928)
- Serialize tests to YAML (#935)

### 🐛 Bug Fixes

- Do not print full url as it might contain the token (#931)
- Filter run_exports by direct specs more logically (#933)
- Use language as default compiler name (#936)

### 📚 Documentation

- Expand docs on build scripts and environment variables (#932)
- Update docs for post-link and pre-link scripts (#934)

### ⚙️ Miscellaneous Tasks

- Prepare release 0.18.0 (#937)
## [0.17.1] - 2024-06-17

### 🐛 Bug Fixes

- Update documentation for breaking change and improve error message (#923)
- Unify how script files are found and fix a regression with explicit paths (#924)

### ⚙️ Miscellaneous Tasks

- Prepare 0.17.1 release (#925)
## [0.17.0] - 2024-06-11

### 🚀 Features

- Better error messages on file overwrite (#856)
- Remove hash_input from rendered recipe (#882)
- Always use urls for channels (#886)
- `stdlib` implementation and compiler refactor (#892)
- Implement `lower_bound` and `upper_bound` and improve pinning implementation (#891)
- Use gateway and installer from latest rattler (#848)
- Add channel priority and solve strategy (#888)
- Make `jinja` filters and tests explicit, rename `cmp` to `match` (#902)
- Allow nushell for scripts (#907)
- Extract tar- and zip immediately and use copydir from cache (#911)
- Add channel specific test (#914)
- Change `env` implementation (#917)
- Pin expressions take only `lower` and `upper` bound (#918)
- Initial implementation of cache build step (#898)

### 🐛 Bug Fixes

- Cargo-edit example recipe.yaml (#860)
- Enable post-link script execution (#876)
- Rename rattler_build to rattler-build (#877)
- Fix test for zlink for macos (#885)
- Remove an unwrap in `skip_existing` (#895)
- Cpp tutorial (#905)
- Preserve permissions when streaming conda or tarbz2 (#920)

### 💼 Other

- Add `build.files` and negative globs to rattler-build (#819)

This PR adds more globbing options, e.g.:

```yaml
files:
  include:
    - foo/
  exclude:
    - foo/*.txt
```
- Add bitfurnace example (#908)
- Refactor internalrepr into enum (#916)

### 🚜 Refactor

- How dependencies are stored (#875)
- Rename constrains to constraints (#901)
- Remove compression threads (#904)
- Remove run exports from finalized deps (#910)

### 📚 Documentation

- Corrections to docs and GHA summary (#874)
- Add docs and use `Version` for parsing lower- and upper_bound (#893)
- Add a reference for the CLI (#887)
- Improve documentation `selectors` and `installation` (#912)
- Rename cmp to match (#913)

### ⚙️ Miscellaneous Tasks

- *(ci)* Bump prefix-dev/setup-pixi from 0.7.0 to 0.8.0 (#855)
- Update pixi project (#884)
- *(ci)* Bump prefix-dev/setup-pixi from 0.8.0 to 0.8.1 (#900)
- *(ci)* Bump prefix-dev/setup-pixi from 0.8.0 to 0.8.1 (#906)
- Move to python snapshot tests for end-to-end testing (#909)
- Simplify git object implementation (#919)
- Release 0.17.0 and update all dependencies (#921)
## [0.16.2] - 2024-05-23

### 🚀 Features

- Add support for WASM targets in the lib check (#849)
- Accept multiple urls for url source and use them as mirrors (#840)

### 🐛 Bug Fixes

- Filter files like conda-build does (#850)

### 💼 Other

- Release 0.16.2 (#853)

### 📚 Documentation

- Clarify docs for Anaconda.org upload (#844)
## [0.16.1] - 2024-05-18

### 🐛 Bug Fixes

- CLI parsing problem with Anaconda upload function (#838)

### 💼 Other

- Release rattler-build 0.16.1 (#841)

### ⚙️ Miscellaneous Tasks

- *(ci)* Bump prefix-dev/setup-pixi from 0.6.0 to 0.7.0 (#836)
- Update dependencies and rattler (#839)
## [0.16.0] - 2024-05-06

### 🚀 Features

- Make the recipe generation for PyPI much more robust (#823)
- Make binary smaller size (#827)
- Use system-tools from build prefix if they are available (#825)

### 🐛 Bug Fixes

- Do not modify cxxflags (#810)
- Correctly identify tarballs (#821) (#822)

### 💼 Other

- Release 0.16.0 (#832)

### 📚 Documentation

- Typo in compilers.md (#831)

### ⚙️ Miscellaneous Tasks

- Update dependencies & rattler (#813)
## [0.15.0] - 2024-04-17

### 🚀 Features

- Improve error messages when patching (#782)
- When rendering requirements, flatten `pin_subpackage` and `pin_compatible`  (#795)
- Include recipes as `recipe.yaml` independent of source recipe name (#797)
- Better help when script execution failed (#799)
- Build for powerpc64le (#807)
- Experimental "post-processing" of files with regex replacement (#798)

### 🐛 Bug Fixes

- Symlink handling when symlink points to a directory (#781)
- Ability to remove read-only files (e.g. from build caches) (#783)
- Use the correct recipe location when editing packages via TUI (#789)
- We are using `if` expressions, not statements (#800)
- Make noarch-unix and noarch-win work (#785)
- Improved overlink warnings and prepare for linking checks on Windows (#688)
- Remove todo about cygwin path and add remaining windows env vars (#802)

### 💼 Other

- Ignore `/dev/null` when guessing strip level (#796)

### 📚 Documentation

- Fix badge style (#787)

### ⚙️ Miscellaneous Tasks

- Sort info files to bottom (#777)
- Tweak the TUI logger settings (#788)
- *(ci)* Bump prefix-dev/setup-pixi from 0.5.1 to 0.5.2 (#791)
- *(ci)* Bump prefix-dev/setup-pixi from 0.5.2 to 0.6.0 (#804)
- More help text & labels (#805)
- Small unicode tweak (#803)
- Prepare release 0.15.0 (#808)
## [0.14.2] - 2024-04-05

### 🚀 Features

- Progress for copying source dir (#767)
- Implement faster skip-existing (#765)

### 🐛 Bug Fixes

- Guess strip level of patch (#762)
- Mamba on windows (#766)
- Fix the TUI package list (#769)
- The skip condition was not checking the name (#770)

### ⚙️ Miscellaneous Tasks

- New banner (#763)
- Update dependencies and prepare release (#772)
## [0.14.1] - 2024-04-03

### 🚀 Features

- Improve git-lfs handling (#756)
- Add --with-solve option (#758)

### 🐛 Bug Fixes

- Improve git source and add progress reporting (#755)
- Create empty build platform folder when noarch (#757)
- Apply patches in target_directory (#760)

### ⚡ Performance

- Reflink instead of copy from source (#754)

### ⚙️ Miscellaneous Tasks

- Fix the spacing issue in git-cliff config (#753)
- Prepare release 0.14.1 (#761)
## [0.14.0] - 2024-04-02

### 🚀 Features

- Create a TUI (#692)
- Support building multiple recipes (#720)
- Sort build outputs for TUI (#727)
- Support building all packages via TUI (#731)
- --render-only will make a dry-run and will not install resolved packages (#729)
- Make --render-only output more parsing friendly (#730)
- Add license warnings to warnings summary (#739)
- Support reading recipe from stdin (#735)
- Add build-platform to options (#744)
- Implement `--skip-existing` flag (#743)

### 🐛 Bug Fixes

- Ensure tagged releases are marked as latest when created (#702)
- Do not use progressbar for JSON output (#707)
- Do not take python variant when noarch:python (#723)
- Sort build outputs (#722)
- Topological sort order (#725)
- Sorting topological on empty output (#728)
- Add missing fields of render-only back (#740)
- Skip relinking checks for webassembly (#741)
- Take into consideration non-canonical variant also (#746)
- Fail build when no license files are found (#749)
- Docs CI (#752)

### 💼 Other

- Update cli_usage.md (#710)
- Improve spdx license error (#750)

### 🚜 Refactor

- Remove BuildOutput wrapper and use Output directly (#732)

### 📚 Documentation

- Use export for shell variables (#704)
- Use consistent name for recipe file (#736)

### ⚙️ Miscellaneous Tasks

- *(ci)* Bump softprops/action-gh-release from 1 to 2 (#705)
- Add pixi badge to the readme (#708)
- Log the name of the missing system tool (#703)
- Update dependencies, set release to 0.14.0 (#719)
- Add site directory to .gitignore (#721)
- Remove unused dependencies (#724)
- Use correct error message for expected fields (#737)
- Improve error messages for recipe dir building (#745)
- Update dependencies before tagging the release (#751)
## [0.13.0] - 2024-03-06

### 🚀 Features

- Added rattler-build authentication CLI (#685)
- Implement `schema_version` top level key (#695)
- Add user-agent to rattler-build client (#696)

### 💼 Other

- Release v0.13.0 (#697)

### 📚 Documentation

- Authentication and uploading packages (#682)
- Update the distro packages section in README.md (#683)
## [0.12.1] - 2024-02-27

### 🚀 Features

- Support scp-style SSH urls for Git source (#677)

### 🐛 Bug Fixes

- Set release as latest after CI succeeds (#674)
- Do not remove non-existing build dir (#681)

### ⚙️ Miscellaneous Tasks

- Add git-cliff config (#679)
## [0.12.0] - 2024-02-26

### 🚀 Features

- Implement skip as list (#653)
- Add color force handling and fix subtle bug with stdout / stderr (#656)
- Make prefix searching much faster by using mmap and memchr (#658)
- Use mmap for better performance while reading files (#661)

### 🐛 Bug Fixes

- *(ci)* Add missing checkout for post-build step (#648)
- Retain order of channels for proper strict priority handling (#659)
- Extraction bars proper length and add checksum verification to path source (#666)

### 💼 Other

- Make links easier to read in light mode (#668)

### 📚 Documentation

- Fix up example recipe (#651)
- Make examples follow some more best practices (#652)
- Add system default to dark light toggle (#654)
- Miscellaneous edits throughout for style and wording (#660)
- Fix license specifier and add hint to spdx (#664)
- Add repology packaging status to README (#663)
- Added LicenseRef option (#665)

### ⚙️ Miscellaneous Tasks

- Update to latest rattler and other dependencies (#667)
- Prepare release 0.12.0 (#673)
## [0.11.0] - 2024-02-20

### 🚀 Features

- Mark version as non-draft after release CI completes (#636)
- Fancy logging and github integration (#592)
- Change default package format to conda (#644)

### 🐛 Bug Fixes

- Forcibly add rpath on macos and make some warning -> info (#637)
- Always add python version as `$PY_VER` (#645)

### 🚜 Refactor

- `script` execution during build (#641)

### 📚 Documentation

- Add github action to documentation (#633)
- Add light mode and a toggle (#431)

### ⚙️ Miscellaneous Tasks

- Support for builds on linux-aarch64 (#630)
- *(ci)* Bump dacbd/create-issue-action from 1.2.1 to 2.0.0 (#643)
- Prepare release 0.11.0 (#646)
## [0.10.0] - 2024-02-13

### 🚀 Features

- Take system libs into account while checking links (#613)
- Docs add deploy workflow dispatch (#616)
- Add progress bars during compression (#620)
- Barebones PyPI (Python) and CRAN (R) recipe generation (#594)
- Linking checks on macOS (#627)

### 🐛 Bug Fixes

- Set fetch-depth 0 for dev docs (#615)
- Use only DSO packages during overdepending checks (#619)
- Order of builds and `recipe` key schema (#617)
- Use `VersionWithSource::fmt` instead of `Version::fmt` to determine package version (#626)

### 💼 Other

- Release 0.10.0 (#631)

### 🚜 Refactor

- Post-process and relink modules (#610)
- Separate printing and discovery phase during linking checks (#623)

### 📚 Documentation

- Write about variants, prioritization and mutex packages
- Simplify CLI documentation for autocompletion (#618)
- Advanced build options (#624)
- Start adding some examples (#625)
## [0.9.0] - 2024-02-08

### 🚀 Features

- Implement down-prioritize via track-features (#584)
- Add fix python shebang implementation (#551)
- Implement prefix detection parsing (#597)
- Implement prefix detection settings (#598)
- Add system tools configuration for reproducible builds (#587)
- Resolve rpath (#605)
- Resolve libraries using rpath / runpath (#606)
- Improve linking checks (#600)

### 🐛 Bug Fixes

- Remove noisy error message (#603)
- Cannot find content type of a directory (#607)

### 🚜 Refactor

- Metadata writing  (#585)
- Use a new `FileFinder` struct to encapsulate finding new files in the prefix (#588)
- Move code around and clean up function interfaces a bit (#595)

### 🧪 Testing

- Add test for parsing "null" values and missing jinja (#602)

### ⚙️ Miscellaneous Tasks

- *(ci)* Bump prefix-dev/setup-pixi from 0.4.3 to 0.5.0 (#582)
- *(ci)* Bump prefix-dev/setup-pixi from 0.5.0 to 0.5.1 (#591)
- Suppress noisy auth storage warnings (#599)
- Update all dependencies and release 0.9.0 (#609)
## [0.8.1] - 2024-02-01

### 🚀 Features

- Skip over any Null values in mappings (#569)
- Improve error message for invalid `noarch` (#571)
- Fix test command and use target_platform from package (#574)
- Simplify parser with macro (#572)
- Update docs, update rattler and release new version (#578)
- Forcibly compile python to pyc files if necessary (#549)
- Add `merge_build_and_host_envs` section (#545)
- Add variant options (#546)

### 🐛 Bug Fixes

- Release docs building and cleanup (#568)
- Release binaries (#567)
- Test_file function to ignore read_exact error on too small files (#580)

### 📚 Documentation

- Fix link to examples (#573)
- Fix references to boa (#581)
## [0.8.0] - 2024-01-31

### 🚀 Features

- Add `rpath_allowlist` to the recipe (#473)
- Add `binary_relocation` to the recipe (#479)
- Add `load_from_file` jinja function (#468)
- Expose and store compression level and compression threads (#484)
- Add conda-forge integration (#465)
- Implement `patchelf` and `install_name_tool` in Rust (#501)
- Perform post processing after package installation (#505)
- Documentation versioning (#504)
- Enable use of os certificate store (#530)
- Set default rpath for binaries (linux) (#531)
- Install build dependencies when running tests (#537)
- Add `GlobVec`, `always_copy_files` and `always_include_files` (#540)
- Move `rpath_allowlist` and `relocate_paths` to `GlobVec` (#542)
- Implement additional relink options for macOS (#536)
- Add many more package content tests and refactor/fix the implementation (#548)
- Add underlinking/overdepending checks (#458)

### 🐛 Bug Fixes

- Bug with ignore_run_exports_from in host_env & add tests (#477)
- Non deterministic git tests (#481)
- Update documentation and add docs for experimental features (#480)
- Flush files after download (#490)
- Use the correct target-platform for test (#491)
- Write out entry points for non-noarch packages (#482)
- Remove unwrap in entry_points code (#495)
- Add documentation for compression level and fix default case (#499)
- Update clap-verbosity to get rid of spurious comment (#500)
- Package field error message (#510)
- Use the correct default for rpath (#533)
- Also filter the stderr of the build process (#534)
- Use expect instead of unwrap (#535)
- Version check and create issue on failure (#541)
- Treat single-element build script as `CommandOrPath` (#543)
- Upload binaries for release (#544)
- Use actual revision for rev-parse instead of git head (#547)
- Script block parsing and add a test for script env (#556)
- Improve test running experience and test unicode file packages (#561)
- Using jinja in YAML maps and add tests (#563)

### 💼 Other

- Delete environment_linux.yaml (#552)
- Prepare release 0.8.0 (#565)

### 🚜 Refactor

- Use rattler index crate instead of local index functionality (#489)
- Package_tests (#514)

### 📚 Documentation

- Mention the Arch Linux package (#478)
- Install rattler-build from homebrew-core (#469)
- Fix minor errors and improve styling (#492)
- Document jinja functions (#483)
- Fix up cli invocation in docs (#503)

### 🧪 Testing

- Properly set stdout and stderr capture  in rust-tests (#538)
- Make sure rpath is inserted (#532)

### ⚙️ Miscellaneous Tasks

- Bump rattler to 0.16.2 (#488)
- Update all dependencies & rattler (#506)
- *(ci)* Bump prefix-dev/setup-pixi from 0.4.1 to 0.4.3 (#511)
- *(ci)* Bump prefix-dev/setup-pixi from 0.4.1 to 0.4.3 (#517)
- Fix CI badge (#564)
## [0.7.0] - 2024-01-10

### 🚀 Features

- Add auth_file option to read authentication information for repositories from a file (#413)
- Add upload command to upload to a `quetz` server (#429)
- Add completions generation for shells (#426)
- Upload to artifactory (#439)
- Tar & zip in rust (#438)
- Uploading to prefix.dev and add progress bars to other uploader (#432)
- Implement changes to source parsing (#454)
- Multiple tests and execution (#418)
- Progressbars for downloading and extracting (#449)
- Add support for uploading to anaconda.org (#464)
- Remove rpaths that point outside the prefix (#467)
- Add git jinja functions & experimental flag (#423)

### 🐛 Bug Fixes

- Rolling errors for render and try convert api (#414)
- Add `set -x` only with bash (#416)
- Cleanup code for render and try_convert api (#417)
- Add version assertion for release (#424)
- Ensure zips without root are handled (#425)
- Handling both forward and backward slash (#388)
- Deduplication issue in create_index_json function (#441)
- Skip relinking if it is not a shared library on linux (#443)
- Extraction for zip files (#447)
- Minor cleanup (#448)
- Make toml test more reproducible (#455)
- Create a temporary directory next to target (#456)
- Improve completions dx (#451)
- Use env for shell detection (#459)
- Ignore pyo and egg-info files for noarch (#470)
- Use ignore_run_exports for building env (#472)

### 💼 Other

- Release 0.7.0 (#475)

### 📚 Documentation

- Use new docs with mkdocs (#430)
- Improve upload help descriptions (#460)
- Fix test documentation (#466)

### ⚙️ Miscellaneous Tasks

- Refactor CI (#408)
- *(ci)* Bump actions/upload-artifact from 3 to 4 (#422)
- *(ci)* Bump actions/configure-pages from 3 to 4 (#434)
- *(ci)* Bump prefix-dev/setup-pixi from 0.3.0 to 0.4.1 (#437)
- *(ci)* Bump actions/deploy-pages from 2 to 4 (#435)
- *(ci)* Bump actions/upload-pages-artifact from 2 to 3 (#436)
- Upgrade all packages and get rid of git dependency on marked-yaml (#471)
- Add description in cargo for publishing (#476)
## [0.6.1] - 2023-12-12

### 🚀 Features

- Collect errors as vec (#407)

### 🐛 Bug Fixes

- Improve integer parsing error messages (#398)
- Skip serializing default fields (#401)
- Cross-platform building - use the proper platform when resolving (#411)
- Add test for tar source and fix filename for single file source (#412)

### 🚜 Refactor

- Add python object (#400)
- Serde Dependency instead of string magic (#405)

### ⚙️ Miscellaneous Tasks

- Add contributing and code of conduct (#397)
## [0.6.0] - 2023-12-05

### 🚀 Features

- Add option and env variable to disable zstd and bz2 (#364)
- Package content tests  (#359)

### 🐛 Bug Fixes

- Improve jinja err msg (#363)
- Remove all the unwraps from production code (#370)
- Text detection fallback & create empty directories (#372)
- Use which to find executables and return better error messages if they are missing (#387)
- File creation problems (#383)

### 💼 Other

- Implement pin-compatible (#368)
- Update docs slightly and release 0.6.0 (#395)

### 🚜 Refactor

- Move script_env into script (#392)
- Move run_exports to requirements (#394)

### 📚 Documentation

- Fix docs for correctness (#362)
- Fix jinja string interp instructions (#367)
- Add docs for package-contents test (#389)

### ⚙️ Miscellaneous Tasks

- *(ci)* Bump actions/deploy-pages from 2 to 3 (#385)
- *(ci)* Bump actions/configure-pages from 3 to 4 (#386)
- Update rattler and other dependencies (#396)
## [0.5.2] - 2023-11-27

### 🚀 Features

- Add use-gitignore to recipe (#358)

### 🐛 Bug Fixes

- Error when output directory in recipe folder (#351)
- Throw error if output missing (#352)
- More unwraps and cleanup (#342)
- Add docs for use_gitignore and lfs and only provide use_gitignore for PathSource (#361)

### 💼 Other

- Better error handling with string matcher parse error (#353)
- Update README.md (#354)

Fix banner reference typo
- Release 0.5.2 with latest rattler (#360)
## [0.5.1] - 2023-11-23

### 🚀 Features

- Add mamba example (#322)

### 🐛 Bug Fixes

- Remove raw recipe (#305)
- Cleanup unwraps (#327)
- Improve rendering of `DependencyInfo` (#319)
- Make url source a more explicit (#320)
- Render hash info with all information incl. hash_input (#338)
- Allow inline rendering in sequence (#339)
- Error if git rev and git depth are used together (#340)
- Handle symlinks as-is when copying data (#341)
- Noarch with multiple outputs (#347)

### 💼 Other

- Added docs everywhere and refactor some bits (#336)

* refactor: remove some unused code and use IndexMap exclusively
- 0.5.1 (#348)

### 🚜 Refactor

- Hash info (#337)

### 📚 Documentation

- Improve docs and completely remove `sel(...)` syntax (#332)
- More content for testing packages (#333)

### ⚙️ Miscellaneous Tasks

- Remove unused functions (#329)
## [0.5.0] - 2023-11-17

### 🚀 Features

- Make copy_dir() return copied paths (#199)
- New recipe parser (#205)
- *(recipe)* Improvements to error messages (#224)
- Add more explanatory error message when not copying license files (#221)
- Implement test file copying with copy dir and improve url source (#230)
- *(recipe)* Rendering yaml values before parsing (#234)
- Move `VariantConfig` to the new yaml parser and remove legacy code (#236)
- *(recipe)* Expand `TryConvertNode` implementations to include `Option` (#237)
- Add cdt function (#252)
- *(recipe)* Parse multiple outputs & skip conditions (#240)
- No-build-id aka static build dirs (#250)
- Rewrite copy_dir() fn into buildable object (#233)
- Add env support (#260)
- Store the original recipe and rendered recipe (#246)
- Add end-to-end tests in rust (#253)
- Make recipe storing optional (defaults to true) (#272)
- Use git on host computer for git cloning (replaces libgit2) (#269)
- Add --no-test CLI option (#289)
- Sort outputs when computing variant (#278)
- Improve label for jinja errors (#302)
- Add lfs option to git source (#296)

### 🐛 Bug Fixes

- Do not use PathBuf in arguments (#198)
- Some parsing panics and errors (#216)
- Some parsing panics and errors (#216)
- *(recipe)* Fix inline-if parsing dependencies (#223)
- Make sure that package hash can be rendered using `${{ hash }}` (#225)
- Align recipes with format repo and update readme (#235)
- Remove some unwraps (#257)
- Manual try expr implementation (#265)
- Assert & error on lfs (#280)
- Use `dunce::canonicalize` everywhere (#284)
- Error message for invalid sha256 (#291)
- Unit tests on windows (#297)
- Test schema more and make things more correct (#299)
- Make tests work better cross-platform (#292)
- Tests for windows (#300)
- Noarch entry-points and rework how we run tests (#301)
- Jinja variable extraction for cmp function in if expression (#308)
- Use latest rattler with secret redaction (#306)

### 💼 Other

- Use dtolnay/rust-toolchain to install rustfmt component (#200)
- Copy license files with globs (#201)
- Implement python post processing for INSTALLER and add PYTHON variable(#209)

Also do some dist-info wrangling and warn if package name / version does not match PyPI
- Run end-to-end test in CI (#210)
- Update all dependencies (#220)
- Simplify Display impl for ErrorKind (#238)
- Add reference to installation via homebrew (#242)
- Add SOURCE_DATE_EPOCH (#256)
- Update all dependencies (#259)
- Don't use cross for x86_64-unknown-linux-gnu (#275)
- Release 0.5.0 (#314)

### 🚜 Refactor

- Reduce code duplication (#217)
- Put git stuff behind a feature flag (#219)
- Reimplement `TryConvertNode` for `Test` (#241)
- *(recipe)* Change the `Rendered*Node` Debug impl to match the type names (#247)
- Some code cleanup (#258)

### 📚 Documentation

- Update and align with new format (#227)
- Fix a broken link and grammar issues (#251)
- Add env functions to docs (#268)
- Update for new git implementation (#279)
- Write more documentation on the new features  (#303)
- Fix up docs (#310)

### ⚙️ CI

- Also upload binaries on non-release (#271)

### ⚙️ Miscellaneous Tasks

- Update all dependencies and better caching in CI (#215)
- Fix clippy (#270)
- *(ci)* Bump prefix-dev/setup-pixi from 0.3.0 to 0.4.0 (#273)
- *(ci)* Bump prefix-dev/setup-pixi from 0.4.0 to 0.4.1 (#290)
- Update all dependencies and rattler (#294)
## [0.4.0] - 2023-10-06

### 🚀 Features

- Build .conda package right away (#190)
- Copy dir with globs (#189)
- Print package contents (#194)
- Use tracing (#195)

### 🐛 Bug Fixes

- Use Result::transpose instead of manual implementation (#181)
- Tiny enhancements for emscripten-wasm32 compatibility (#182)
- Fix bug when copying directory, fix bug when creating conda package (#192)
- Add perl and extra platforms (#191)
- Ignore missing about section and missing build.sh / build.bat (#196)

### 💼 Other

- Add support for local source file url scheme (#177)

Fix #176
- Update to the latest rattler version (#180)
- Update to latest rattler and add pixi project (#186)
- Update all dependencies of rattler-build (#187)
- Remove package before testing (#193)
- Release version 0.4.0 (#197)

### ⚙️ Miscellaneous Tasks

- *(ci)* Bump actions/checkout from 3 to 4 (#179)
- Run cargo update and replace `tempdir` with `tempfile` (#188)
## [0.3.1] - 2023-07-27

### 🐛 Bug Fixes

- Fix script env and release 0.3.1 (#175)
## [0.3.0] - 2023-07-25

### 🚀 Features

- Make it possible to use `rattler-build` as library (#158)

### 🐛 Bug Fixes

- Allow not finding arch platforms in channel (#164)

### 💼 Other

- Update all versions incl. minijinja to 1.0.0-alpha2 (#155)
- Update README.md (#160)
- Bump rich version in example (#162)
- Rattler 0.6.0 (#165)
- Add support for script_env in build section (#167)
- Release 0.3.0 (#171)

### 🚜 Refactor

- Put `source` in its own module (#168)

### ⚙️ Miscellaneous Tasks

- *(ci)* Bump actions/upload-pages-artifact from 1 to 2 (#166)
## [0.2.0] - 2023-06-09

### 🚀 Features

- Add source.path as a option or specifying source (#131)
- Simplify CLI by allowing for recipe path and defaulting to '.' (#132)

### 🐛 Bug Fixes

- Add noarch bin files to python-scripts (#135)
- Relative git_url paths (#142)
- Fix variants with version spec (#144)

### 💼 Other

- Add pre-commit config and ran it. (#133)
- Make git sources useable (#137)

* feat: improve handling of git errors and give it more options
* feat: Implement local git url's, fix fetching, fix copy_dir
* test: add test to git source and fix found issues
- Improve error reporting when parsing files and remove some unwrap (#143)
- Enable `--version` and `-V` flags (#145)
- Update all dependencies (#150)
- Fix recipe rendering for dictionaries and add more variables to script env (#146)
- Beautify readme a little bit (#152)
- Release rattler-build 0.2.0 (#153)
- Vendor openssl to get rid of build issues with libgit2 (#154)
## [0.1.3] - 2023-04-19

### 🐛 Bug Fixes

- Fix up selector docs (#126)

### 💼 Other

- Set M1 toolchain as default for cross builds (#127)
- Release 0.1.3 properly (#128)
## [0.1.2] - 2023-04-19

### 🐛 Bug Fixes

- Fix some typos (#119)

### 💼 Other

- Replace unmaintained actions-rs/* actions in CI workflows (#121)

Basically all of the `actions-rs/*` actions are unmaintained. See
<https://github.com/actions-rs/toolchain/issues/216> for more
information. Due to their age they generate several warnings in
CI runs.

To get rid of those warnings the occurrences of
`actions-rs/toolchain` are replaced by `dtolnay/rust-toolchain`,
and the occurrences of `actions-rs/cargo` are replaced by direct
invocations of `cargo`.
- Fix/platform folder repodata generation (#120)
- Update rattler and set timestamp (#122)
- Add dependency installation docs (#123)
- Keep `--force-rpath` and expand comment
- Merge pull request #124 from wolfv/keep_rpath

keep `--force-rpath` and expand comment
- Rename local_channel to output_dir and put source cache in there, too (#125)
## [0.1.1] - 2023-04-17

### 🚀 Features

- Add --channel to the cli so the users can add the needed channels (#117)

### 💼 Other

- Add cargo-edit example Rust project
- Add new snapshots after rename
- Update all dependencies
- Use configured output dir
- Add docs and release link to README
- Restore building docs
## [0.1.0] - 2023-04-16

### 🐛 Bug Fixes

- Fix most clippy lints
- Fix workflow
- Fix toolchain and remove random file
- Fix all clippy warnings
- Fix clippy
- Fix up env activation (#19)
- Fix tests
- Fix packaged pyc files and run tests (#78)
- Fix name of `run_constrained
- Fix workflow

### 💼 Other

- Initial commit
- Add some more stuff
- Continue working on the barebones stuff
- Make xtensor quote-build-endquote
- Add missing file
- Creates archive files now
- Run cargo fmt
- Produce indexable packages
- Make roar compile again
- Clippy & fmt
- Use rattler package types (#1)
- Add working selectors (#2)
- Make rendering work, use minijinja everywhere instead of starlark (#3)
- Clean deps and add github workflow
- Use 1.68.0 as toolchain
- Remove useless actions for now
- Add better source caching, make builds run again (#4)
- Make the first recipes build
- Add ability to apply patches
- Clean up directory difference code
- Use rattler_digest and remove hash functions
- Update to rust 1.68
- Add indexing (via conda index)
- Clippy fixes
- Add macOS relinking and noarch (python) package support (#5)
- Upgrade dependencies in Cargo.toml
- Add linux patchelf support, some default env vars (unused), update all deps (#6)
- Bump clap from 4.1.14 to 4.2.0 (#14)
- Implement initial variant pass and many other small improvements (#15)
- Remove conda index
- Use our index
- Add missing test file
- Use rattler for install, fix up symlink relativization, use rattler shell activation (#18)
- Upgrade to latest rattler
- Print dependencies as table and return them from solver function
- Add flattening for variant config files, docs, and use default env vars (#21)
- Bump rattler from `dde2f2f` to `fade382` (#48)
- Clean up variant computation and add zip key functionality (#49)
- Upgrade rattler
- Use new rattler api for activation
- Bump serde_yaml from 0.9.19 to 0.9.20 (#59)
- Bump serde_with from 2.3.1 to 2.3.2 (#60)
- Dependency list and add functions to collect run exports (#58)
- Bump serde_yaml from 0.9.20 to 0.9.21 (#62)
- More testcases (#61)
- Add more error types and docs and fix compiler target_platform (#63)
- Implement copying of license files
- Skip all non elf files and allow multiline build scripts from yaml (#64)
- Add testing and `test` subcommand to roar (#74)
- Bump serde from 1.0.159 to 1.0.160 (#76)
- Ignore all rattler updates for now (#89)
- Bump serde_json from 1.0.95 to 1.0.96 (#80)
- Remove PlatformOrNoarch (#88)
- Docs and error clean up (#92)
- Add docs and github action for it (#93)
- Enable docs workflow
- Use correct repo name
- Try building book first
- Build book
- Create dir
- Add reference docs as well
- Use proper flatten-toplevel for variant config file loading (#94)
- Try to fix docs build
- Emulate conda-build hash computation (#95)
- Improve build string computation
- Improve `noarch: python` writing for python-scripts (#97)
- Add tests and docs for selectors and cmp function (#98)
- Pin subpackage and better dependency tracking and run exports rendering (#100)
- Remove some dead code (#103)
- Implement long prefix placeholder (#104)
- Add more comments, get rid of source dir, use args that work on windows (#105)
- Refactor test function a bit more, and always create `noarch` channel & use local channel (#111)
- Implement better output for variants etc. (#114)
- Rename to `rattler-build`
- Remove some print statements
- Add README and examples
- Add release workflow
- Add workflow dispatch
- Try to improve release workflow
- Try building for aarch64-darwin
- Add license file, skip tests and extend Cargo metadata
- Disable musl, release 0.1.0

### 🚜 Refactor

- Refactor how the relinking looks like in preparation for also checking over and underlinking (#77)

Also add windows linking stuff, and introduce structs per dylib / dll /
