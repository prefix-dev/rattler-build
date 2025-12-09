# Stage0 Recipe

Parsed recipe types before Jinja evaluation.

Stage0 represents a recipe as parsed from YAML, where Jinja templates and conditionals have not yet been evaluated.

```python
from rattler_build import Stage0Recipe, SingleOutputRecipe, MultiOutputRecipe
```

## Recipe Classes

::: rattler_build.Stage0Recipe
    options:
        members:
            - from_yaml
            - from_file
            - from_dict
            - as_single_output
            - as_multi_output
            - to_dict
            - render
            - run_build

::: rattler_build.SingleOutputRecipe
    options:
        members:
            - schema_version
            - context
            - package
            - build
            - requirements
            - about

::: rattler_build.MultiOutputRecipe
    options:
        members:
            - schema_version
            - context
            - recipe
            - build
            - about
            - outputs
