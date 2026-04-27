# Changelog

All notable changes to this project will be documented in this file.

The format is based on [Keep a Changelog](https://keepachangelog.com/en/1.0.0/),
and this project adheres to [Semantic Versioning](https://semver.org/spec/v2.0.0.html).

## [Unreleased]

## [0.2.5](https://github.com/prefix-dev/rattler-build/compare/rattler_build_core-v0.2.4...rattler_build_core-v0.2.5) - 2026-04-27

### Fixed

- remove PKG_* environment variables in staging cache, set variant values ([#2449](https://github.com/prefix-dev/rattler-build/pull/2449))
- use force directory removal to handle Windows file locks ([#2444](https://github.com/prefix-dev/rattler-build/pull/2444))
- prevent stacking of pending-rm suffixes on Windows cleanup ([#2439](https://github.com/prefix-dev/rattler-build/pull/2439))
- set CONDA_BUILD env var in build_env.sh to fix env-isolation none ([#2433](https://github.com/prefix-dev/rattler-build/pull/2433))
- include default env vars also in test environment ([#2425](https://github.com/prefix-dev/rattler-build/pull/2425))

## [0.2.4](https://github.com/prefix-dev/rattler-build/compare/rattler_build_core-v0.2.3...rattler_build_core-v0.2.4) - 2026-04-13

### Added

- enforce clean build environment with configurable isolation modes ([#2372](https://github.com/prefix-dev/rattler-build/pull/2372))

### Fixed

- resolve overlinking false positives for staging outputs ([#2402](https://github.com/prefix-dev/rattler-build/pull/2402))
- exclude new files from strip level guessing in patch application ([#2400](https://github.com/prefix-dev/rattler-build/pull/2400))

## [0.2.3](https://github.com/prefix-dev/rattler-build/compare/rattler_build_core-v0.2.2...rattler_build_core-v0.2.3) - 2026-03-31

### Fixed

- skip empty command tests during packaging ([#2390](https://github.com/prefix-dev/rattler-build/pull/2390))
- "built with" metadata ([#2377](https://github.com/prefix-dev/rattler-build/pull/2377))
- reindex all platform subdirs in build_reindexed_channels ([#2383](https://github.com/prefix-dev/rattler-build/pull/2383))
- set variant and platform env vars in test scripts ([#2365](https://github.com/prefix-dev/rattler-build/pull/2365))

### Other

- Fix obsolete `min_pin` and `max_pin` references ([#2380](https://github.com/prefix-dev/rattler-build/pull/2380))

## [0.2.2](https://github.com/prefix-dev/rattler-build/compare/rattler_build_core-v0.2.1...rattler_build_core-v0.2.2) - 2026-03-25

### Added

- Add Python API for `debug` ([#2337](https://github.com/prefix-dev/rattler-build/pull/2337))

### Fixed

- use platform-specific script extensions for test scripts ([#2354](https://github.com/prefix-dev/rattler-build/pull/2354))

### Other

- improve table output ([#2369](https://github.com/prefix-dev/rattler-build/pull/2369))

## [0.2.1](https://github.com/prefix-dev/rattler-build/compare/rattler_build_core-v0.2.0...rattler_build_core-v0.2.1) - 2026-03-18

### Fixed

- expected commit usage ([#2335](https://github.com/prefix-dev/rattler-build/pull/2335))

## [0.2.0](https://github.com/prefix-dev/rattler-build/compare/rattler_build_core-v0.1.1...rattler_build_core-v0.2.0) - 2026-03-17

### Added

- update crates, move to workspace, drop tui ([#2331](https://github.com/prefix-dev/rattler-build/pull/2331))

### Fixed

- [**breaking**] remove `--debug` from CLI and Python API ([#2329](https://github.com/prefix-dev/rattler-build/pull/2329))

## [0.1.1](https://github.com/prefix-dev/rattler-build/compare/rattler_build_core-v0.1.0...rattler_build_core-v0.1.1) - 2026-03-13

### Added

- enable arwen-codesign experimentally ([#2269](https://github.com/prefix-dev/rattler-build/pull/2269))

### Fixed

- honor run-export ignore from staging output ([#2282](https://github.com/prefix-dev/rattler-build/pull/2282))
- always set `SP_DIR` during script execution ([#2142](https://github.com/prefix-dev/rattler-build/pull/2142))

### Other

- use Rattler-Build for program name in doc comments and docstrings ([#2289](https://github.com/prefix-dev/rattler-build/pull/2289))
- add readme for all crates ([#2292](https://github.com/prefix-dev/rattler-build/pull/2292))
- *(rattler_build_core)* release v0.1.0 ([#2281](https://github.com/prefix-dev/rattler-build/pull/2281))

## [0.1.0](https://github.com/prefix-dev/rattler-build/releases/tag/rattler_build_core-v0.1.0) - 2026-03-09

### Added

- WASM playground ([#2218](https://github.com/prefix-dev/rattler-build/pull/2218))
- create a TUI ([#692](https://github.com/prefix-dev/rattler-build/pull/692))

### Fixed

- crates versioning ([#2280](https://github.com/prefix-dev/rattler-build/pull/2280))
- markdown links in README.md ([#2092](https://github.com/prefix-dev/rattler-build/pull/2092))
- stricter input parsing and more lenient parsing of run exports from other packages ([#1271](https://github.com/prefix-dev/rattler-build/pull/1271))
- align recipes with format repo and update readme ([#235](https://github.com/prefix-dev/rattler-build/pull/235))

### Other

- *(rattler_build_core)* release v0.58.5 ([#2274](https://github.com/prefix-dev/rattler-build/pull/2274))
- set up release-plz ([#2273](https://github.com/prefix-dev/rattler-build/pull/2273))
- move most of `rattler-build` crate to new `rattler_build_core` ([#2271](https://github.com/prefix-dev/rattler-build/pull/2271))
- update playground URL to playground.rattler.build ([#2228](https://github.com/prefix-dev/rattler-build/pull/2228))
- improve banner image in README ([#2050](https://github.com/prefix-dev/rattler-build/pull/2050))
- Update README banner image and link ([#2049](https://github.com/prefix-dev/rattler-build/pull/2049))
- update documentation style and improve docs ([#2013](https://github.com/prefix-dev/rattler-build/pull/2013))
- *(README)* remove outdated requirement on `patch` command ([#1777](https://github.com/prefix-dev/rattler-build/pull/1777))
- need git to checkout repo ([#1683](https://github.com/prefix-dev/rattler-build/pull/1683))
- do not need tar on host system ([#1659](https://github.com/prefix-dev/rattler-build/pull/1659))
- remove schema link ([#1408](https://github.com/prefix-dev/rattler-build/pull/1408))
- Remove 'pip install' options in recipes ([#1198](https://github.com/prefix-dev/rattler-build/pull/1198))
- update readme ([#959](https://github.com/prefix-dev/rattler-build/pull/959))
- update images in the readme ([#958](https://github.com/prefix-dev/rattler-build/pull/958))
- Fix badge style ([#787](https://github.com/prefix-dev/rattler-build/pull/787))
- new banner ([#763](https://github.com/prefix-dev/rattler-build/pull/763))
- add pixi badge to the readme ([#708](https://github.com/prefix-dev/rattler-build/pull/708))
- update the distro packages section in README.md ([#683](https://github.com/prefix-dev/rattler-build/pull/683))
- add repology packaging status to README ([#663](https://github.com/prefix-dev/rattler-build/pull/663))
- miscellaneous edits throughout for style and wording ([#660](https://github.com/prefix-dev/rattler-build/pull/660))
- make examples follow some more best practices ([#652](https://github.com/prefix-dev/rattler-build/pull/652))
- fix up example recipe ([#651](https://github.com/prefix-dev/rattler-build/pull/651))
- add github action to documentation ([#633](https://github.com/prefix-dev/rattler-build/pull/633))
- fix CI badge ([#564](https://github.com/prefix-dev/rattler-build/pull/564))
- install rattler-build from homebrew-core ([#469](https://github.com/prefix-dev/rattler-build/pull/469))
- mention the Arch Linux package ([#478](https://github.com/prefix-dev/rattler-build/pull/478))
- fix test documentation ([#466](https://github.com/prefix-dev/rattler-build/pull/466))
- Update README.md ([#354](https://github.com/prefix-dev/rattler-build/pull/354))
- Add reference to installation via homebrew ([#242](https://github.com/prefix-dev/rattler-build/pull/242))
- update and align with new format ([#227](https://github.com/prefix-dev/rattler-build/pull/227))
- Update README.md ([#160](https://github.com/prefix-dev/rattler-build/pull/160))
- beautify readme a little bit ([#152](https://github.com/prefix-dev/rattler-build/pull/152))
- add pre-commit config and ran it. ([#133](https://github.com/prefix-dev/rattler-build/pull/133))
- add dependency installation docs ([#123](https://github.com/prefix-dev/rattler-build/pull/123))
- add docs and release link to README
- add release workflow
- add README and examples

## [0.58.5](https://github.com/prefix-dev/rattler-build/compare/rattler_build_core-v0.58.4...rattler_build_core-v0.58.5) - 2026-03-06

### Added

- WASM playground ([#2218](https://github.com/prefix-dev/rattler-build/pull/2218))
- create a TUI ([#692](https://github.com/prefix-dev/rattler-build/pull/692))

### Fixed

- markdown links in README.md ([#2092](https://github.com/prefix-dev/rattler-build/pull/2092))
- stricter input parsing and more lenient parsing of run exports from other packages ([#1271](https://github.com/prefix-dev/rattler-build/pull/1271))
- align recipes with format repo and update readme ([#235](https://github.com/prefix-dev/rattler-build/pull/235))

### Other

- set up release-plz ([#2273](https://github.com/prefix-dev/rattler-build/pull/2273))
- move most of `rattler-build` crate to new `rattler_build_core` ([#2271](https://github.com/prefix-dev/rattler-build/pull/2271))
- update playground URL to playground.rattler.build ([#2228](https://github.com/prefix-dev/rattler-build/pull/2228))
- improve banner image in README ([#2050](https://github.com/prefix-dev/rattler-build/pull/2050))
- Update README banner image and link ([#2049](https://github.com/prefix-dev/rattler-build/pull/2049))
- update documentation style and improve docs ([#2013](https://github.com/prefix-dev/rattler-build/pull/2013))
- *(README)* remove outdated requirement on `patch` command ([#1777](https://github.com/prefix-dev/rattler-build/pull/1777))
- need git to checkout repo ([#1683](https://github.com/prefix-dev/rattler-build/pull/1683))
- do not need tar on host system ([#1659](https://github.com/prefix-dev/rattler-build/pull/1659))
- remove schema link ([#1408](https://github.com/prefix-dev/rattler-build/pull/1408))
- Remove 'pip install' options in recipes ([#1198](https://github.com/prefix-dev/rattler-build/pull/1198))
- update readme ([#959](https://github.com/prefix-dev/rattler-build/pull/959))
- update images in the readme ([#958](https://github.com/prefix-dev/rattler-build/pull/958))
- Fix badge style ([#787](https://github.com/prefix-dev/rattler-build/pull/787))
- new banner ([#763](https://github.com/prefix-dev/rattler-build/pull/763))
- add pixi badge to the readme ([#708](https://github.com/prefix-dev/rattler-build/pull/708))
- update the distro packages section in README.md ([#683](https://github.com/prefix-dev/rattler-build/pull/683))
- add repology packaging status to README ([#663](https://github.com/prefix-dev/rattler-build/pull/663))
- miscellaneous edits throughout for style and wording ([#660](https://github.com/prefix-dev/rattler-build/pull/660))
- make examples follow some more best practices ([#652](https://github.com/prefix-dev/rattler-build/pull/652))
- fix up example recipe ([#651](https://github.com/prefix-dev/rattler-build/pull/651))
- add github action to documentation ([#633](https://github.com/prefix-dev/rattler-build/pull/633))
- fix CI badge ([#564](https://github.com/prefix-dev/rattler-build/pull/564))
- install rattler-build from homebrew-core ([#469](https://github.com/prefix-dev/rattler-build/pull/469))
- mention the Arch Linux package ([#478](https://github.com/prefix-dev/rattler-build/pull/478))
- fix test documentation ([#466](https://github.com/prefix-dev/rattler-build/pull/466))
- Update README.md ([#354](https://github.com/prefix-dev/rattler-build/pull/354))
- Add reference to installation via homebrew ([#242](https://github.com/prefix-dev/rattler-build/pull/242))
- update and align with new format ([#227](https://github.com/prefix-dev/rattler-build/pull/227))
- Update README.md ([#160](https://github.com/prefix-dev/rattler-build/pull/160))
- beautify readme a little bit ([#152](https://github.com/prefix-dev/rattler-build/pull/152))
- add pre-commit config and ran it. ([#133](https://github.com/prefix-dev/rattler-build/pull/133))
- add dependency installation docs ([#123](https://github.com/prefix-dev/rattler-build/pull/123))
- add docs and release link to README
- add release workflow
- add README and examples
