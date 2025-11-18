# Publishing packages to your own channel

Rattler-build comes with an intuitive `publish` subcommand, that will publish a package to a channel.

You can either point to an already built package, or to a recipe and `publish` them into your channel.
The channel can be either on prefix.dev, anaconda.org, an S3 bucket, a local filesystem folder (or network mount), or a Quetz or Artifactory instance.

To publish a package to prefix.dev, you can use:

```bash
rattler-build publish --to https://prefix.dev/my-channel

rattler-build publish --to s3://my-bucket

rattler-build publish --to artifactory://my-secret.company.com/package-channel
```

The following schema is used:

- prefix.dev: `https://prefix.dev/<channel-name>`
- anaconda.org: `https://anaconda.org/<owner>/<label (optional)>` (e.g. https://anaconda.org/foobar)
- S3: `s3://bucket-name` (note: we read the standard S3 configuration / environment variables for region, authentication, etc.)
- Filesystem: `file:///path/to/channel`
- Quetz: `quetz://server.my-company.com/<channel>`
- Articatory: `artifactory://server.my-company.com/<channel>`

