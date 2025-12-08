# Stage1 Recipe

Evaluated recipe types ready for building.

Stage1 represents a fully evaluated recipe where all Jinja templates have been resolved and conditionals evaluated.

```python
from rattler_build import Stage1Recipe
```

::: rattler_build.Stage1Recipe
    options:
        members:
            - package
            - build
            - requirements
            - about
            - context
            - used_variant
            - sources
            - staging_caches
            - inherits_from
            - to_dict
:::
