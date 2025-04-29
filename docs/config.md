# Rattler-build configuration

`rattler-build` can be configured by specifying `--config-file ~/.pixi/config.toml`.
The config file is of the same format as pixi's [global configuration file](https://pixi.sh/latest/reference/pixi_configuration/).

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

By specifying the `mirrors` section, you can instruct rattler-build to use mirrors when building.
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
