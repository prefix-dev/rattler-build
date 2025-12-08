# Rendering

Recipe rendering converts Stage0 recipes to Stage1 recipes with variant configurations.

```python
from rattler_build import RenderConfig, RenderedVariant
```

::: rattler_build.RenderConfig

::: rattler_build.RenderedVariant
    options:
        members:
            - variant
            - recipe
            - hash_info
            - pin_subpackages
            - run_build
