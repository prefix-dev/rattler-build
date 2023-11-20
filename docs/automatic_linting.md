# Automatic linting in VSCode

The enw recipe format comes with a strict JSON scheme. You can find the scheme
[in this repository](https://github.com/prefix-dev/recipe-format).

It's implemented with `pydantic` and renders to a JSON schema file. The [YAML
language server extension in
VSCode](https://marketplace.visualstudio.com/items?itemName=redhat.vscode-yaml)
can recognize the scheme and give helpful hints during editing.

With the YAML language server installed the automatic linting can be enabled by
adding the following line to the top of the recipe file:

```yaml
# yaml-language-server: $schema=https://raw.githubusercontent.com/prefix-dev/recipe-format/main/schema.json
```