# CLI usage

## Shell Completions

We support shell completions through [`clap_complete`](https://crates.io/crates/clap_complete). You can generate them for your shell using the `completion` command.

You can add the completions to your shell by adding the following to your shell's configuration file:

```sh
# For bash (add this to ~/.bashrc)
eval "$(rattler-build completion --shell=bash)"
# For zsh (add this to ~/.zshrc)
eval "$(rattler-build completion --shell=zsh)"
# For fish (add this to ~/.config/fish/config.fish)
rattler-build completion --shell=fish | source
```

Ensure that wherever you install `rattler-build` is pointed to by your `FPATH` (for `zsh` or equivalent in other shells), after which point you can use TAB or any configured completion key of choice.

```sh
$ rattler-build <TAB>
build    -- Build a package
help     -- Print this message or the help of the given subcommand(s)
rebuild  -- Rebuild a package
test     -- Test a package

## Package format

You can specify the package format (either `.tar.bz2` or `.conda`) by using the `--package-format` flag.
You can also set the compression level with `:<level>` after the package format. The `<level>` can be `max`, `min`, `default` or a number corresponding to the compression level.
`.tar.bz2` supports compression levels between `1` and `9` while `.conda` supports compression levels between `-7` and `22`.
For `.conda`, you can also set the `--compression-threads` flag to specify the number of threads to use for compression.

```sh
# default
rattler-build build --package-format tarbz2 -r recipe/recipe.yaml
# maximum compression with 10 threads
rattler-build build --package-format conda:max --compression-threads 10 -r recipe/recipe.yaml
```

## Logs

`rattler-build` knows three different log styles: `fancy`, `plain`, and `json`.
You can configure them with the `--log-style=<style>` flag:

```sh
# default
rattler-build build --log-style fancy -r recipe/recipe.yaml
```

### GitHub integration

`rattler-build` also has a GitHub integration. With this integration, warnings are automatically emitted in the GitHub Actions log and a summary is generated and posted to the GitHub Actions summary page.

To make use of this integration, we recommend using our custom GitHub action: [`rattler-build-action`](https://github.com/prefix-dev/rattler-build-action). To manually enable it, you can set the environment variable `RATTLER_BUILD_ENABLE_GITHUB_INTEGRATION=true`.
