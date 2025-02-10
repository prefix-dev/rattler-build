# Enabling Automatic Linting in VSCode

Our new recipe format adheres to a strict JSON schema, which you can access [here](https://github.com/prefix-dev/recipe-format).

This schema is implemented using `pydantic` and can be rendered into a JSON schema file. The [YAML language server extension in VSCode](https://marketplace.visualstudio.com/items?itemName=redhat.vscode-yaml) is capable of recognizing this schema, providing useful hints during the editing process.
