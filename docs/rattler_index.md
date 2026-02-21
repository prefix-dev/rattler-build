`rattler-index` is a rattler-based tool which allows you to index your channels.
It can create `repodata.json`, `repodata.json.zst` as well as sharded repodata ([CEP 16](https://github.com/conda/ceps/blob/main/cep-0016.md)).
It can index both conda channels on a local file system as well as conda channels on S3.

## Installation

You can install `rattler-index` using pixi:

```bash
pixi global install rattler-index
```

## Usage

`rattler-index` has two subcommands for indexing channels on different storage backends:

### Indexing a local filesystem channel

```bash
rattler-index fs /path/to/channel
```

### Indexing an S3 channel

```bash
rattler-index s3 s3://my-bucket/my-channel
```

For S3 channels, you can provide credentials via command-line options or they will be resolved from the AWS SDK (environment variables, AWS config files, etc.).

## Global Options

| Option | Default | Description |
|--------|---------|-------------|
| `--write-zst` | `true` | Write compressed `repodata.json.zst` files |
| `--write-shards` | `true` | Write sharded repodata ([CEP 16](https://github.com/conda/ceps/blob/main/cep-0016.md)) |
| `-f, --force` | `false` | Force re-indexing of all packages, creating a new `repodata.json` instead of updating the existing one |
| `--max-parallel <N>` | 10 | Maximum number of packages to process in-memory simultaneously. Useful for limiting memory usage when indexing large channels |
| `--target-platform <PLATFORM>` | all | Index only a specific platform (e.g., `linux-64`, `osx-arm64`). By default, all platforms in the channel are indexed |
| `--repodata-patch <PACKAGE>` | none | Name of a conda package (in the `noarch` subdir) to use for repodata patching. See [repodata patching](https://prefix.dev/blog/repodata_patching) for more information |
| `--config <PATH>` | none | Path to a config file (uses the same format as [pixi configuration](https://pixi.sh/latest/reference/pixi_configuration)) |
| `-v, -vv, -vvv` | none | Increase verbosity level |

### S3-specific Options

| Option | Description |
|--------|-------------|
| `--disable-precondition-checks` | Disable ETag and timestamp checks during file operations. Use if your S3 backend doesn't fully support conditional requests, or if you're certain no concurrent indexing processes are running. **Warning:** Disabling this removes protection against concurrent modifications |
| `--region <REGION>` | AWS region for the S3 bucket |
| `--endpoint-url <URL>` | Custom S3 endpoint URL (for S3-compatible storage like MinIO) |

S3 credentials can also be configured in the config file under the `s3_options` section.

## Examples

Index a local channel with default settings:
```bash
rattler-index fs ./my-channel
```

Index only the `linux-64` platform:
```bash
rattler-index fs ./my-channel --target-platform linux-64
```

Force a full re-index (ignoring existing repodata):
```bash
rattler-index fs ./my-channel --force
```

Index without sharded repodata:
```bash
rattler-index fs ./my-channel --write-shards false
```

Index an S3 channel with a custom endpoint (e.g., MinIO):
```bash
rattler-index s3 s3://my-bucket/channel --endpoint-url http://localhost:9000 --region us-east-1
```

Apply repodata patches from a package:
```bash
rattler-index fs ./my-channel --repodata-patch my-repodata-patches
```
