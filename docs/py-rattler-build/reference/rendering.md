# Rendering

Render recipes with variant configurations to produce buildable outputs.

You can import the rendering classes from `rattler_build`:

```python
from rattler_build import VariantConfig, RenderConfig, RenderedVariant
```

## `VariantConfig`

::: rattler_build.VariantConfig
    options:
        members:
            - __init__
            - from_file
            - from_file_with_context
            - from_conda_build_config
            - from_yaml
            - from_yaml_with_context
            - keys
            - zip_keys
            - get_values
            - to_dict
            - combinations
            - get
            - items
            - values

## `RenderConfig`

::: rattler_build.RenderConfig
    options:
        members:
            - __init__
            - get_context
            - get_all_context
            - target_platform
            - build_platform
            - host_platform
            - experimental
            - recipe_path

## `RenderedVariant`

::: rattler_build.RenderedVariant
    options:
        members:
            - variant
            - recipe
            - hash_info
            - pin_subpackages
            - run_build

## Supporting Types

### `HashInfo`

::: rattler_build.render.HashInfo
    options:
        members:
            - hash
            - prefix

### `PinSubpackageInfo`

::: rattler_build.render.PinSubpackageInfo
    options:
        members:
            - name
            - version
            - build_string
            - exact
