# Recipe

Recipe classes for parsing and working with conda recipes.

rattler-build uses a two-stage recipe system:

1. **Stage0** - Parsed YAML recipes before Jinja template evaluation
2. **Stage1** - Fully evaluated recipes ready for building

You can import the recipe classes from `rattler_build`:

```python
from rattler_build import Stage0Recipe, SingleOutputRecipe, MultiOutputRecipe, Stage1Recipe
```

## Stage0 - Parsed Recipes

### `Stage0Recipe`

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
            - schema_version
            - context
            - build
            - about

### `SingleOutputRecipe`

::: rattler_build.SingleOutputRecipe
    options:
        members:
            - schema_version
            - context
            - package
            - build
            - requirements
            - about

### `MultiOutputRecipe`

::: rattler_build.MultiOutputRecipe
    options:
        members:
            - schema_version
            - context
            - recipe
            - build
            - about
            - outputs

## Stage1 - Evaluated Recipes

### `Stage1Recipe`

::: rattler_build.stage1.Stage1Recipe
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

## Supporting Types

### Stage0 Types

::: rattler_build.stage0.Package
    options:
        members:
            - name
            - version
            - to_dict

::: rattler_build.stage0.PackageOutput
    options:
        members:
            - package
            - to_dict

::: rattler_build.stage0.StagingOutput
    options:
        members:
            - to_dict

::: rattler_build.stage0.RecipeMetadata
    options:
        members:
            - to_dict

::: rattler_build.stage0.Build
    options:
        members:
            - number
            - string
            - script
            - noarch
            - to_dict

::: rattler_build.stage0.Requirements
    options:
        members:
            - build
            - host
            - run
            - run_constraints
            - to_dict

::: rattler_build.stage0.About
    options:
        members:
            - homepage
            - license
            - license_family
            - summary
            - description
            - documentation
            - repository
            - to_dict

### Stage1 Types

::: rattler_build.stage1.Package
    options:
        members:
            - name
            - version
            - to_dict

::: rattler_build.stage1.Build
    options:
        members:
            - number
            - string
            - script
            - noarch
            - to_dict

::: rattler_build.stage1.Requirements
    options:
        members:
            - build
            - host
            - run
            - to_dict

::: rattler_build.stage1.About
    options:
        members:
            - homepage
            - repository
            - documentation
            - license
            - summary
            - description
            - to_dict

::: rattler_build.stage1.Source
    options:
        members:
            - to_dict

::: rattler_build.stage1.StagingCache
    options:
        members:
            - name
            - build
            - requirements
            - to_dict
