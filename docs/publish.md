# Publishing packages to your own channel

Rattler-build comes with an intuitive `publish` subcommand, that will publish a package to a channel.

You can either point to an already built package, or to a recipe and `publish` them into your channel.
The channel can be either on prefix.dev, anaconda.org, an S3 bucket, a local filesystem folder (or network mount), or a Quetz or Artifactory instance.

Publishing a package is a convenience short hand for:

1. Building the package from a recipe.yaml
2. Uploading the package to a channel
3. Running `rattler-index` for a S3 bucket or a filesystem channel to produce `repodata.json`.

To publish a package you can use:

```bash
rattler-build publish ./my-recipe.yaml --to https://prefix.dev/my-channel

rattler-build publish ./output/linux-64/my-package-0.1.2-h123_0.conda --to s3://my-bucket

rattler-build publish ./some/recipe.yaml --to artifactory://my-secret.company.com/package-channel
```

The following schema is used:

- prefix.dev: `https://prefix.dev/<channel-name>`
- anaconda.org: `https://anaconda.org/<owner>/<label (optional)>` (e.g. https://anaconda.org/foobar)
- S3: `s3://bucket-name` (note: we read the standard S3 configuration / environment variables for region, authentication, etc.)
- Filesystem: `file:///path/to/channel`
- Quetz: `quetz://server.my-company.com/<channel>`
- Articatory: `artifactory://server.my-company.com/<channel>`

## Options

The `--to` option selects the channel to publish the package into. This channel will also be used as the _highest priority_ channel in the list of channels. Other channels can be added using the usual `-c conda-forge -c bioconda ...` syntax or configured using a `config.toml` file.

When using `publish` with a recipe, you can use the same options as when normally building packages.

## Authentication

Rattler-build uses the same authentication as other tools in the prefix family. It's easiest to login using the `auth` subcommand: `rattler-build auth login`. Note: if you are already logged in with `pixi`, you are also logged in with `rattler-build` - they share credentials.

Otherwise you can also use the same options as with `upload`, and supply tokens as environment variables or CLI arguments.

## Indexing S3 and Filesystem channels

Since S3 and Filesystem channels don't know anything about "indexing" (producing repodata.json), rattler-build will internally use `rattler-index` to run the indexing step after a successful upload. This will ensure that the repodata in the channel is up to date and users can start downloading the new packages.
