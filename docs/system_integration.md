# System integration for packages

When you are building packages, you might want to integrate with the system to install shortcuts, desktop icons, etc.

In the Conda ecosystem, this is the job of "menuinst" - originally a Python project, that we ported to our underlying `rattler` library.
To install a menuitem, you need to place a specially crafted JSON file in the right location in the package / conda environment.

```yaml title="recipe.yaml"
build:
  script:
    # ... build the package
    # Install the menu item
    - mkdir -p $PREFIX/Menu
    - cp $RECIPE_DIR/menu/menu.json $PREFIX/Menu/pixi-editor.json
    - cp $RECIPE_DIR/icons/pixi-icon.* $PREFIX/Menu/
```

To learn more about installing menu items, please take a look at the [`menuinst` documentation](https://conda.github.io/menuinst/).

## Installing shell completion scripts

Shell completion scripts are scripts that are sourced by the shell to provide tab-completion for commands.
They are automatically picked up by `pixi` and other tools when they appear in the right location in your package.

These locations are:

- `bash`: `$PREFIX/share/bash-completion/completions/`
- `zsh`: `$PREFIX/share/zsh/site-functions/`
- `fish`: `$PREFIX/share/fish/vendor_completions.d/`

Following is an example of how to ship shell completions for `ripgrep` in a package:

```yaml title="recipe.yaml"
package:
  name: ripgrep
  version: "1.24.3"

# ... other fields omitted for brevity

build:
  number: 1
  noarch: generic
  script:
    # Build and install ripgrep ...
    # Then generate the completions
    # ZSH completions
    - mkdir -p $PREFIX/share/zsh/site-functions
    - rg --generate complete-zsh > $PREFIX/share/zsh/site-functions/_rg
    # Bash completions
    - mkdir -p $PREFIX/share/bash-completion/completions
    - rg --generate complete-bash > $PREFIX/share/bash-completion/completions/rg
    # Fish completions
    - mkdir -p $PREFIX/share/fish/vendor_completions.d
    - rg --generate complete-fish > $PREFIX/share/fish/vendor_completions.d/rg.fish

# ... continue recipe
```

Note that tools like `pixi global install` will expect completion script name to match the binary name. The pattern is as follows:

- `bash`: `<binary-name>`
- `zsh`: `_<binary-name>`
- `fish`: `<binary-name>.fish`
