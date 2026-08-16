# Rattler-Build configuration

Rattler-Build shares its configuration format with pixi: the config file is of the same format as pixi's [global configuration file](https://pixi.sh/latest/reference/pixi_configuration/).

By default (when no `--config-file` is passed), Rattler-Build automatically loads and merges configuration from the standard locations. Discovery is provided by `rattler_config`'s common `locations` helper, which combines two layers: the configuration shared by all rattler based tools, and Rattler-Build's own files. The locations, in ascending order of precedence (values from later files override values from earlier files):

1. The system-wide shared configuration: `/etc/rattler/config.toml` (on Windows: `C:\ProgramData\rattler\config.toml`)
2. The system-wide Rattler-Build configuration: `/etc/rattler-build/config.toml`
3. The per-user shared configuration: `$XDG_CONFIG_HOME/rattler/config.toml`, plus `$RATTLER_HOME/config.toml` when the variable is set
4. The per-user Rattler-Build configuration: the platform config directory (`$XDG_CONFIG_HOME/rattler-build/config.toml`) followed by the tool home (`$RATTLER_BUILD_HOME`, defaulting to `~/.rattler-build/config.toml`)

The shared files may only contain the keys every rattler based tool understands — default channels, mirrors, S3 options, and so on. Tool-specific keys in a shared file are ignored with a warning. Settings meant for every tool (pixi and Rattler-Build alike) belong in the shared files; settings meant only for Rattler-Build belong in its own files, which override the shared ones.

Alternatively, a single configuration file can be specified explicitly with `--config-file`, which disables the automatic discovery and loads only that file. To disable configuration entirely — so that only built-in defaults and command-line arguments apply — pass `--no-config` (mutually exclusive with `--config-file`).

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
