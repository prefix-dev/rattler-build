# Changelog

All notable changes to this project will be documented in this file.

The format is based on [Keep a Changelog](https://keepachangelog.com/en/1.0.0/),
and this project adheres to [Semantic Versioning](https://semver.org/spec/v2.0.0.html).

## [Unreleased]

## [0.1.7](https://github.com/prefix-dev/rattler-build/compare/rattler_build_recipe-v0.1.6...rattler_build_recipe-v0.1.7) - 2026-04-27

### Added

- remove experimental guard for staging outputs ([#2432](https://github.com/prefix-dev/rattler-build/pull/2432))

### Fixed

- disable build file auto-discovery for multi-output recipes ([#2436](https://github.com/prefix-dev/rattler-build/pull/2436))

## [0.1.6](https://github.com/prefix-dev/rattler-build/compare/rattler_build_recipe-v0.1.5...rattler_build_recipe-v0.1.6) - 2026-04-13

### Added

- Allow for build_string_prefix passed into packages ([#2384](https://github.com/prefix-dev/rattler-build/pull/2384))

### Fixed

- reset origin URL and sync submodules before update ([#2401](https://github.com/prefix-dev/rattler-build/pull/2401))
- integers in skip/match conditions from variants.yaml ([#2395](https://github.com/prefix-dev/rattler-build/pull/2395))

## [0.1.5](https://github.com/prefix-dev/rattler-build/compare/rattler_build_recipe-v0.1.4...rattler_build_recipe-v0.1.5) - 2026-03-31

### Other

- updated the following local packages: rattler_build_types, rattler_build_jinja, rattler_build_yaml_parser, rattler_build_variant_config

## [0.1.4](https://github.com/prefix-dev/rattler-build/compare/rattler_build_recipe-v0.1.3...rattler_build_recipe-v0.1.4) - 2026-03-25

### Other

- adapt recipe stage 0 to fit Pixi's needs ([#2373](https://github.com/prefix-dev/rattler-build/pull/2373))

## [0.1.3](https://github.com/prefix-dev/rattler-build/compare/rattler_build_recipe-v0.1.2...rattler_build_recipe-v0.1.3) - 2026-03-18

### Other

- update Cargo.toml dependencies

## [0.1.2](https://github.com/prefix-dev/rattler-build/compare/rattler_build_recipe-v0.1.1...rattler_build_recipe-v0.1.2) - 2026-03-17

### Added

- update crates, move to workspace, drop tui ([#2331](https://github.com/prefix-dev/rattler-build/pull/2331))

### Other

- update mininjinja and remove custom undefined variable handling ([#2300](https://github.com/prefix-dev/rattler-build/pull/2300))

## [0.1.1](https://github.com/prefix-dev/rattler-build/compare/rattler_build_recipe-v0.1.0...rattler_build_recipe-v0.1.1) - 2026-03-13

### Other

- enable topological sorting of build outputs ([#2268](https://github.com/prefix-dev/rattler-build/pull/2268))
- use Rattler-Build for program name in doc comments and docstrings ([#2289](https://github.com/prefix-dev/rattler-build/pull/2289))
- add readme for all crates ([#2292](https://github.com/prefix-dev/rattler-build/pull/2292))
