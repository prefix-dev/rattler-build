# Publishing packages to your own channel

Rattler-build comes with an intuitive `publish` subcommand, that will publish a package to a channel.

You can either point to an already built package, or to a recipe and `publish` them into your channel.
The channel can be either on prefix.dev, anaconda.org, an S3 bucket, a local filesystem folder (or network mount), or a Quetz or Artifactory instance.

Publishing a package is a convenience short hand for:

1. Building the package from a recipe.yaml
   1. Optionally, automatically increment the build number by passing in `--build-number=+1` to set a _relative_ value, or `--build-number=12` to set an absolute value for all packages being built.
2. Uploading the package to a channel
3. Running `rattler-index` for a S3 bucket or a filesystem channel to produce `repodata.json`.

To publish a package you can use:

```bash
rattler-build publish ./my-recipe.yaml --to https://prefix.dev/my-channel

rattler-build publish ./output/linux-64/my-package-0.1.2-h123_0.conda --to s3://my-bucket

rattler-build publish ./some/recipe.yaml --to artifactory://my-secret.company.com/package-channel

# on prefix.dev you can also automatically add a sigstore attestation
rattler-build publish ./my-recipe.yaml --to https://prefix.dev/my-channel --generate-attestation
```

The following schema is used:

- prefix.dev: `https://prefix.dev/<channel-name>`
- anaconda.org: `https://anaconda.org/<owner>/<label (optional)>` (e.g. `https://anaconda.org/foobar`)
- S3: `s3://bucket-name` (note: we read the standard S3 configuration / environment variables for region, authentication, etc.)
- Filesystem: `file:///path/to/channel`
- Quetz: `quetz://server.my-company.com/<channel>`
- Articatory: `artifactory://server.my-company.com/<channel>`

## Options

The `--to` option selects the channel to publish the package into. This channel will also be used as the _highest priority_ channel in the list of channels. Other channels can be added using the usual `-c conda-forge -c bioconda ...` syntax or configured using a `config.toml` file.

When using `publish` with a recipe, you can use the same options as when normally building packages.

## Bumping the build number

Sometimes you want to package the same package again, but rebuild it with the latest dependencies. There are two ways to achieve this:

- By using a variant file, and updating it at certain times
- By bumping the build number and re-publishing the package again

The publish command makes it easy to "bump the buildnumber" either by setting an absolute build number for all packages the recipe builds (e.g. `--build-number=12`) or a _relative_ bump for all packages (e.g. `--build-number=+1` to add 1 to the _highest_ build number found in the publish channel).
When bumping by a relative amount, we download the repodata and determine for each subdir/package combination that you are building the highest build number.

When the recipe does not specify a build number, the build number is automatically bumped on `publish` to the _next available build number_.

If the recipe does specify a build number, you have to manually trigger an override using the `--build-number` CLI flag. Alternatively, you can use the `--force` upload option on S3, your local filesystem, Anaconda and prefix channels to forcibly replace the previous build. Please note that this is heavily discouraged as lockfiles will get out of date and the old build is irrevocably deleted.

## Authentication

Rattler-build uses the same authentication as other tools in the prefix family. It's easiest to login using the `auth` subcommand: `rattler-build auth login`. Note: if you are already logged in with `pixi`, you are also logged in with `rattler-build` - they share credentials.

Otherwise you can also use the same options as with `upload`, and supply tokens as environment variables or CLI arguments.

## Channel Initialization

When publishing to local filesystem or S3 channels, rattler-build automatically handles channel initialization:

### New channels

If the target channel doesn't exist yet, rattler-build will:

1. Create the channel directory (for filesystem channels)
2. Initialize it with an empty `noarch/repodata.json`
3. Upload your package
4. Run indexing to update the repodata

```bash
# This will create /path/to/my-channel if it doesn't exist
rattler-build publish ./my-package.conda --to file:///path/to/my-channel

# Same for S3 buckets
rattler-build publish ./my-package.conda --to s3://my-bucket/channel
```

### Existing channels

If the channel directory exists but is not properly initialized (missing `noarch/repodata.json`), rattler-build will fail with a helpful error message. This prevents accidentally treating a random directory as a conda channel.

To initialize an existing directory as a channel, you can either:

- Let rattler-build create it fresh (remove the directory first)
- Manually create `noarch/repodata.json` with content `{"packages": {}, "packages.conda": {}}`

## Indexing S3 and Filesystem channels

Since S3 and Filesystem channels don't know anything about "indexing" (producing repodata.json), rattler-build will internally use `rattler-index` to run the indexing step after a successful upload. This will ensure that the repodata in the channel is up to date and users can start downloading the new packages.
