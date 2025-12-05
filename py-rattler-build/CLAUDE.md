# CLAUDE.md

This file provides guidance to Claude Code (claude.ai/code) when working with code in this repository.

## Overview

py-rattler-build provides Python bindings for [rattler-build](https://github.com/prefix-dev/rattler-build), a tool for building conda packages. It uses PyO3 to wrap the Rust `rattler-build` crate and expose it as a Python API.

## Development Commands

All commands are run via `pixi run`:

```bash
# Run tests (includes type-checking first)
pixi run test

# Type-check only
pixi run type-check

# Run a specific test
pixi run pytest tests/unit/test_basic.py -k test_version_match

# Format code
pixi run fmt            # Format both Python and Rust
pixi run fmt-python     # Format Python only
pixi run fmt-rust       # Format Rust only

# Lint code
pixi run lint           # Lint both Python and Rust
pixi run lint-python    # Python (ruff)
pixi run lint-rust      # Rust (clippy)

# Check cargo lock is up to date
pixi run check-cargo-lock
```

## Architecture

### Two-Stage Recipe System

The codebase implements a two-stage recipe processing pipeline:

1. **Stage0** (`src/rattler_build/stage0.py`, `rust/src/stage0.rs`): Parsed YAML recipes before Jinja template evaluation. Types may contain template strings like `${{ name }}`.

2. **Stage1** (`src/rattler_build/stage1.py`, `rust/src/stage1.rs`): Fully evaluated recipes with all templates resolved, ready for building.

### Recipe Workflow

```
Recipe.from_yaml() -> Stage0Recipe
        |
        v
recipe.render(variant_config) -> list[RenderedVariant]
        |
        v
variant.run_build() -> BuildResult
```

### Key Python Modules

- `stage0.py`: Recipe parsing - `Recipe.from_yaml()`, `Recipe.from_file()`, `Recipe.from_dict()`
- `stage1.py`: Evaluated recipe types with resolved package names, versions, requirements
- `render.py`: `RenderConfig`, `RenderedVariant` for variant-based recipe rendering
- `variant_config.py`: `VariantConfig` for specifying build variants (e.g., python versions)
- `tool_config.py`: `ToolConfiguration`, `PlatformConfig` for build configuration
- `cli_api.py`: High-level functions: `build_recipes()`, `test_package()`, `upload_*` functions
- `package.py`: Package inspection and testing API (`Package`, `PackageTest`, etc.)
- `progress.py`: Progress callbacks for build monitoring

### Rust Structure

- `rust/src/lib.rs`: PyO3 module definition, `build_recipes_py()`, `build_rendered_variant_py()`
- `rust/src/stage0.rs`, `rust/src/stage1.rs`: Python bindings for recipe types
- `rust/src/render.rs`: Recipe rendering bindings
- `rust/src/tool_config.rs`: Configuration bindings
- `rust/Cargo.toml`: Dependencies on `rattler-build` (parent crate) and related `rattler_*` crates

The Rust code lives in `rust/` and is built via maturin. The parent `rattler-build` crate is referenced via path: `../../`.

## Testing

Tests are in `tests/unit/`. The `conftest.py` provides a `recipes_dir` fixture pointing to test recipes.

Run specific test files:
```bash
pixi run pytest tests/unit/test_render.py -v
```

Use `-k` for test name patterns:
```bash
pixi run pytest -k "test_variant" -v
```
