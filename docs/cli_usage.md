# CLI Usage

## Shell Completions

We support shell completions through clap-complete.
You can generate them for your shell using the `completion` command.

You can add the completions to your shell by adding the following to your shell's configuration file.

```sh
# For bash (add this to ~/.bashrc)
eval "$(rattler-build completion --shell=bash)"
# For zsh (add this to ~/.zshrc)
eval "$(rattler-build completion --shell=zsh)"
# For fish (add this to ~/.config/fish/config.fish)
rattler-build completion --shell=fish | source
```

Ensure that whereever you install the is pointed to by your FPATH (for zsh or equivalent in other shells).
Now you can use TAB or your configured completion key. :3

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

Rattler-build knows 3 different log-styles: `fancy`, `plain` and `json`.
You can configure them with the `--log-style=<style>` flag.

```sh
# default
rattler-build build --log-style fancy -r recipe/recipe.yaml
```

### Github integration

Rattler-build also has a github integration. With this integration, warnings are automatically
emitted in the github actions log and a summary is generated that is posted to the Github Actions summary page.

To make use of the integration we recommend to use our Github action: [`rattler-build-action`](https://github.com/prefix-dev/rattler-build-action). To manually enable it, you can set the environment variable `RATTLER_BUILD_ENABLE_GITHUB_INTEGRATION=true`.
