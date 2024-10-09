# Enabling Automatic Linting in VSCode

Our new recipe format adheres to a strict JSON schema, which you can access [here](https://github.com/prefix-dev/recipe-format).

This schema is implemented using `pydantic` and can be rendered into a JSON schema file. The [YAML language server extension in VSCode](https://marketplace.visualstudio.com/items?itemName=redhat.vscode-yaml) is capable of recognizing this schema, providing useful hints during the editing process.

To enable automatic linting with the YAML language server, you need to add the following line at the beginning of your recipe file:

```yaml
# yaml-language-server: $schema=https://raw.githubusercontent.com/prefix-dev/recipe-format/main/schema.json
```

**Alternatively**, if you prefer not to add this line to your file, you can install the [JSON Schema Store Catalog extension](https://marketplace.visualstudio.com/items?itemName=remcohaszing.schemastore). This extension will also enable automatic linting for your recipe files.
