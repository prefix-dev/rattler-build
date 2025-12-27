# Reference

Here's the reference documentation for the `rattler_build` Python bindings API.

If you want to **learn how to use** rattler-build's Python API, check out the
[Tutorials](../tutorials/recipe_rendering_basics.md).

## Version

Get the version of rattler-build:

```python
from rattler_build import rattler_build_version

print(rattler_build_version())
```

::: rattler_build.rattler_build_version

## Core API

- **[Recipe](recipe.md)** - Parse and work with conda recipes (`Stage0Recipe`, `Stage1Recipe`)
- **[Rendering](rendering.md)** - Render recipes with variants (`RenderedVariant`, `VariantConfig`)
- **[Package](package.md)** - Inspect packages and run tests (`Package`, `PackageTest`)
- **[Build Result](build_result.md)** - Build output information (`BuildResult`)

## Configuration

- **[Configuration](configuration.md)** - Build and platform settings (`ToolConfiguration`, `PlatformConfig`, `JinjaConfig`, `RenderConfig`)

## Utilities

- **[Upload](upload.md)** - Upload packages to various servers
- **[Recipe Generation](recipe_generation.md)** - Generate recipes from PyPI, CRAN, CPAN, LuaRocks
- **[Progress](progress.md)** - Progress callbacks for monitoring builds

## Exceptions

- **[Exceptions](exceptions.md)** - Error types raised by rattler-build
