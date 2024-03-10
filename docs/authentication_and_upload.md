# Server authentication

## Authenticating with a server

You may want to use private channels for which you need to be authenticated. To
do this ephemerally you can use the `RATTLER_AUTH_FILE` environment variable to
point to a JSON file with the following structure:

```json
{
    "*.prefix.dev": {
        "BearerToken": "your_token"
    },
    "otherhost.com": {
        "BasicHttp": {
            "username": "your_username",
            "password": "your_password"
        }
    },
    "anaconda.org": {
        "CondaToken": "your_token"
    }
}
```

The keys are the host names. You can use wildcard specifiers here (e.g.
`*.prefix.dev` to match all subdomains of `prefix.dev`, such as
`repo.prefix.dev`). This will allow you to also obtain packages from any private
channels that you have access to.

The following known authentication methods are supported:

- `BearerToken`: prefix.dev
- `CondaToken`: anaconda.org, quetz
- `BasicHttp`: artifactory

## Uploading packages

If you want to upload packages, then rattler-build comes with a built-in
`upload` command. There are 4 options:

- `prefix.dev`: you can create public or private channels on the prefix.dev
  hosted server
- `anaconda.org`: you can upload packages to the free anaconda.org server
- `quetz`: you can host your own quetz server and upload packages to it
- `artifactory`: you can upload packages to a JFrog Artifactory server

The command is:

```bash
rattler-build upload <server> <package_files>
```

Note: you can also use the `RATTLER_AUTH_FILE` environment variable to
authenticate with the server.

### prefix.dev

To upload to [prefix.dev](https://prefix.dev), you need to have an account and a
token. You can create a token in the settings of your account. The token is used
to authenticate the upload.

```bash
export PREFIX_API_KEY=<your_token>
rattler-build upload prefix -c <channel> <package_files>
```

You can also use the `--api-key=$PREFIX_API_KEY` option to pass the token
directly to the command. Note that you need to have created the channel on the
prefix.dev website before you can upload to it.

### Quetz

You need to pass a token and API key to upload to a channel on your own Quetz
server. The token is used to authenticate the upload.

```bash
export QUETZ_API_KEY=<your_token>
rattler-build upload quetz -u <url> -c <channel> <package_files>
```

### Artifactory

To upload to an Artifactory server, you need to pass a username and password.
The username and password are used to authenticate the upload.

```bash
export ARTIFACTORY_USERNAME=<your_username>
export ARTIFACTORY_PASSWORD=<your_password>
rattler-build upload artifactory -u <url> -c <channel> <package_files>
```

### anaconda.org

To upload to [anaconda.org](https://anaconda.org), you need to pass a owner,
channel (with optional label) and API key. The API key is used to authenticate
the upload.

The owner is the is owner of the distribution, for example, your user name or
"conda-forge". The channel is `main` by default (which means something like
`owner/main`), but optionally you can also specify labels, such as `main/rc` for
release candidates.

You can also add the `--force` argument to forcibly upload a new package (and
overwrite any existing ones).

```bash
export ANACONDA_OWNER=<your_username>
export ANACONDA_API_KEY=<your_token>
rattler-build upload anaconda -c <channel> <package_files>
```
