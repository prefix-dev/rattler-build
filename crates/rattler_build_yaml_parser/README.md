<h1>
  <a href="https://prefix.dev/tools/rattler-build">
    <img alt="banner" src="https://github.com/user-attachments/assets/456f8ef1-1c7b-463d-ad88-de3496b05db2">
  </a>
</h1>

# rattler_build_yaml_parser

YAML parser with Jinja2 template support for rattler-build, providing shared parsing infrastructure for conditional structures and template values.

## Core Types

- `Value<T>` - A value that can be either concrete or a Jinja2 template
- `ConditionalList<T>` - A list that may contain conditional if/then/else items
- `Item<T>` - An item in a conditional list (either a value or a conditional)
- `ListOrItem<T>` - Either a single item or a list of items
- `Conditional<T>` - An if/then/else conditional structure
