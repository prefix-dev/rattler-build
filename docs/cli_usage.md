# CLI Usage

## Shell Completions

We support shell completions through clap-complete.
You can generate them for your shell using the `completion` command.

E.g.,

```sh
rattler-build completion --shell=zsh > ${ZSH_COMPLETIONS_PATH:~/.zsh/completions}/_rattler-build
compinit
```

Ensure that whereever you install the is pointed to by your FPATH (for zsh or equivalent in other shells).
Now you can use TAB or your configured completion key. :3

```sh
$ rattler-build <TAB>
build    -- Build a package
help     -- Print this message or the help of the given subcommand(s)
rebuild  -- Rebuild a package
test     -- Test a package
```

Example for Fish Shell just generate the `completions.fish` and add to `~/.config/fish/completions`.
```sh
rattler-build completion --shell=fish > ${ZSH_COMPLETIONS_PATH:~/.config/fish/completions}/rattler-build.fish
```

## Package format

You can specify the package format (either `.tar.bz2` or `.conda`) by using the `--package-format` flag.
You can also set the compression level with `:<level>` after the package format. The `<level>` can be `max`, `min`, `default` or a number corresponding to the compression level.
`.tar.bz2` supports compression levels between `1` and `9` while `.conda` supports compression levels between `-7` and `22`.
For `.conda`, you can also set the `--compression-threads` flag to specify the number of threads to use for compression.

```sh
# default
rattler-build build --package-format tarbz2 -r recipe/recipe.yaml
# maximum compression
rattler-build build --package-format conda:max --compression-threads 10 -r recipe/recipe.yaml
```
