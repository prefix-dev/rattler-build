# Reference

API reference for the `rattler_build` Python bindings.

For tutorials on how to use these APIs, see the [Tutorials](../tutorials/recipe_rendering_basics.ipynb) section.

## Recipe Generation

- [`generate_pypi_recipe`](recipe_generation.md) - Generate recipes from PyPI
- [`generate_cran_recipe`](recipe_generation.md) - Generate recipes from CRAN
- [`generate_cpan_recipe`](recipe_generation.md) - Generate recipes from CPAN
- [`generate_luarocks_recipe`](recipe_generation.md) - Generate recipes from LuaRocks

## Recipe Types

- [`Stage0Recipe`](stage0.md) - Parsed recipe (before Jinja evaluation)
- [`Stage1Recipe`](stage1.md) - Evaluated recipe (ready for building)

## Rendering

- [`RenderConfig`](rendering.md) - Configuration for rendering recipes
- [`RenderedVariant`](rendering.md) - Result of rendering with a variant

## Package Inspection

- [`Package`](package.md) - Load and inspect conda packages

## Configuration

- [`VariantConfig`](configuration.md) - Variant configuration for builds
- [`ToolConfiguration`](configuration.md) - Build tool settings
- [`PlatformConfig`](configuration.md) - Platform settings

## Upload

- [`upload_package_to_prefix`](upload.md) - Upload to prefix.dev
- [`upload_package_to_anaconda`](upload.md) - Upload to Anaconda.org
- [`upload_package_to_quetz`](upload.md) - Upload to Quetz
- [`upload_package_to_artifactory`](upload.md) - Upload to Artifactory
- [`upload_packages_to_conda_forge`](upload.md) - Upload to conda-forge

## Exceptions

- [`RattlerBuildError`](exceptions.md) - Base exception class
- [Other exceptions](exceptions.md) - Specific error types
