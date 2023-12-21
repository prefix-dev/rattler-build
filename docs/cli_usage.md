# CLI Usage

## Shell Completions

We support shell completions through clap-complete.
You can generate them for your shell using the `--generate` command.

Eg,
```sh
rattler-build --generate=zsh > ${ZSH_COMPLETIONS_PATH:~/.zsh/completions}/_rattler-build
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
rattler-build --generate=fish > ${ZSH_COMPLETIONS_PATH:~/.config/fish/completions}/rattler-build.fish
```
