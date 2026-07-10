# Rattler-Build configuration

Rattler-Build shares its configuration format with pixi: the config file is of the same format as pixi's [global configuration file](https://pixi.sh/latest/reference/pixi_configuration/).

By default (when no `--config-file` is passed), Rattler-Build automatically loads and merges configuration from the standard locations shared by all rattler based tools. Discovery is provided by `rattler_config`'s common `locations` helper, so the search order is identical to the one pixi and other tools use. The locations, in ascending order of precedence (values from later files override values from earlier files):

1. The system-wide configuration of each tool: `/etc/pixi/config.toml` followed by `/etc/rattler-build/config.toml` (on Windows: `C:\ProgramData\<tool>\config.toml`)
2. The per-user configuration of each tool: the platform config directory (`$XDG_CONFIG_HOME/<tool>/config.toml`, e.g. `~/.config/pixi/config.toml`) and the tool home (`$PIXI_HOME` / `$RATTLER_BUILD_HOME`, defaulting to `~/.pixi/config.toml` / `~/.rattler-build/config.toml`), for `pixi` first and then `rattler-build`

In other words all system-wide files are read before any per-user file, and within each group pixi's files are overridden by Rattler-Build's own files. This means that settings such as default channels, mirrors, or S3 options that you have configured for pixi are picked up by Rattler-Build automatically, and can be overridden in Rattler-Build's own configuration files.

Alternatively, a single configuration file can be specified explicitly with `--config-file` (e.g. `--config-file ~/.pixi/config.toml`), which disables the automatic discovery and loads only that file. To disable configuration entirely — so that only built-in defaults and command-line arguments apply — pass `--no-config` (mutually exclusive with `--config-file`).

!!! note "Behavior change"
    Earlier versions of Rattler-Build only loaded configuration when `--config-file` was passed. Automatic discovery of the locations above is new: if you already have a pixi configuration, Rattler-Build now picks it up by default. Pass `--no-config` to restore the old behavior and skip all configuration loading, or `--config-file` to load only a specific file.

## Seeing which configuration was loaded

On startup Rattler-Build logs its version and the configuration files it actually loaded, so you can always trace where a setting came from:

```
rattler-build 0.68.0
Loaded configuration from: /etc/pixi/config.toml, /home/user/.pixi/config.toml
```

If no configuration file was found — either because none of the default locations exist, or because `--config-file` was not given — it logs `No configuration file loaded` instead. These lines appear at the default log level; run with `-v` to additionally see the full list of candidate paths that were considered (useful when a file you expected is not picked up).

## Programmatic use

Automatic discovery happens **only when you run the `rattler-build` command-line tool**. When Rattler-Build is used as a library (for example from [pixi](https://pixi.sh), or through the Python bindings), no configuration is loaded implicitly — the embedding application constructs the configuration and passes it in. This guarantees that using Rattler-Build programmatically never silently reads your global pixi or Rattler-Build configuration from disk.

## Channels

You can specify custom channels via the `default-channels` option.

```toml title="config.toml"
default-channels = ["conda-forge", "bioconda"]
```

## Package format

You can define the default package format to use for builds.
It can be one of `tar-bz2` or `conda`.
You can also add a compression level to the package format, e.g. `tar-bz2:<number>` (from 1 to 9) or `conda:<number>` (from -7 to 22).

```toml title="config.toml"
[build]
package-format = "conda:22"
```

## Mirror configuration

By specifying the `mirrors` section, you can instruct Rattler-Build to use mirrors when building.
For more information, see [pixi's documentation](https://pixi.sh/latest/reference/pixi_configuration/#mirror-configuration).

```toml title="config.toml"
[mirrors]
"https://conda.anaconda.org/conda-forge" = ["https://prefix.dev/conda-forge"]
```

## S3 configuration

You can configure your S3 buckets that are used during build by specifying `s3-options`. For more information, consult [pixi's documentation](https://pixi.sh/latest/deployment/s3/).

```toml title="config.toml"
[s3-options.my-bucket]
endpoint-url = "https://fsn1.your-objectstorage.com"
region = "US"
force-path-style = false
```
