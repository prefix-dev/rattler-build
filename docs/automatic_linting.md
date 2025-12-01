# Automatic Linting in VSCode

Our new recipe format adheres to a strict JSON schema, which you can access [on Github](https://github.com/prefix-dev/recipe-format).

This schema is implemented using `pydantic` and can be rendered into a JSON schema file. The [YAML language server extension in VSCode](https://marketplace.visualstudio.com/items?itemName=redhat.vscode-yaml) is capable of recognizing this schema, providing useful hints during the editing process.

We have published the schema on [schemastore.org](https://www.schemastore.org/) which means, it will be automatically picked up when the file is called `recipe.yaml`. If you give your recipe files different names, you can use the following in the first line of your recipe to get schema hints:

```yaml
# yaml-language-server: $schema=https://raw.githubusercontent.com/prefix-dev/recipe-format/main/schema.json
```