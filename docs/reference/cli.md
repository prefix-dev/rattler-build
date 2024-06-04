# Command-Line Help for `rattler-build`

This document contains the help content for the `rattler-build` command-line program.

## `rattler-build`

**Usage:** `rattler-build [OPTIONS] [COMMAND]`

##### **Subcommands:**

* `build` — Build a package from a recipe
* `test` — Run a test for a single package
* `rebuild` — Rebuild a package from a package file instead of a recipe
* `upload` — Upload a package
* `completion` — Generate shell completion script
* `generate-recipe` — Generate a recipe from PyPI or CRAN
* `auth` — Handle authentication to external channels

##### **Options:**

- `-v`, `--verbose`

	Increase logging verbosity


- `-q`, `--quiet`

	Decrease logging verbosity


- `--log-style <LOG_STYLE>`

	Logging style

	- Default value: `fancy`
	- Possible values:
		- `fancy`:
			Use fancy logging output
		- `json`:
			Use JSON logging output
		- `plain`:
			Use plain logging output


- `--color <COLOR>`

	Enable or disable colored output from rattler-build. Also honors the `CLICOLOR` and `CLICOLOR_FORCE` environment variable

	- Default value: `auto`
	- Possible values:
		- `always`:
			Always use colors
		- `never`:
			Never use colors
		- `auto`:
			Use colors when the output is a terminal





### `build`

Build a package from a recipe

**Usage:** `rattler-build build [OPTIONS]`

##### **Options:**

- `-r`, `--recipe <RECIPE>`

	The recipe file or directory containing `recipe.yaml`. Defaults to the current directory

	- Default value: `.`

- `--recipe-dir <RECIPE_DIR>`

	The directory that contains recipes


- `--up-to <UP_TO>`

	Build recipes up to the specified package


- `--build-platform <BUILD_PLATFORM>`

	The build platform to use for the build (e.g. for building with emulation, or rendering)

	- Default value: current platform

- `--target-platform <TARGET_PLATFORM>`

	The target platform for the build

	- Default value: current platform

- `-c`, `--channel <CHANNEL>`

	Add a channel to search for dependencies in

	- Default value: `conda-forge`

- `-m`, `--variant-config <VARIANT_CONFIG>`

	Variant configuration files for the build


- `--render-only`

	Render the recipe files without executing the build

	- Possible values: `true`, `false`


- `--with-solve`

	Render the recipe files with solving dependencies

	- Possible values: `true`, `false`


- `--keep-build`

	Keep intermediate build artifacts after the build

	- Possible values: `true`, `false`


- `--no-build-id`

	Don't use build id(timestamp) when creating build directory name

	- Possible values: `true`, `false`


- `--compression-threads <COMPRESSION_THREADS>`

	The number of threads to use for compression (only relevant when also using `--package-format conda`)


- `--use-zstd`

	Enable support for repodata.json.zst

	- Default value: `true`
	- Possible values: `true`, `false`


- `--use-bz2`

	Enable support for repodata.json.bz2

	- Default value: `true`
	- Possible values: `true`, `false`


- `--experimental`

	Enable experimental features

	- Possible values: `true`, `false`


- `--auth-file <AUTH_FILE>`

	Path to an auth-file to read authentication information from


- `--tui`

	Launch the terminal user interface

	- Default value: `false`
	- Possible values: `true`, `false`


###### **Modifying result**

- `--package-format <PACKAGE_FORMAT>`

	The package format to use for the build. Can be one of `tar-bz2` or `conda`.
You can also add a compression level to the package format, e.g. `tar-bz2:<number>` (from 1 to 9) or `conda:<number>` (from -7 to 22).

	- Default value: `conda`

- `--no-include-recipe`

	Don't store the recipe in the final package

	- Possible values: `true`, `false`


- `--no-test`

	Don't run the tests after building the package

	- Default value: `false`
	- Possible values: `true`, `false`


- `--color-build-log`

	Don't force colors in the output of the build script

	- Default value: `true`
	- Possible values: `true`, `false`


- `--output-dir <OUTPUT_DIR>`

	Output directory for build artifacts.

	- Default value: `./output`

- `--skip-existing <SKIP_EXISTING>`

	Whether to skip packages that already exist in any channel If set to `none`, do not skip any packages, default when not specified. If set to `local`, only skip packages that already exist locally, default when using `--skip-existing. If set to `all`, skip packages that already exist in any channel

	- Default value: `none`
	- Possible values:
		- `none`:
			Do not skip any packages
		- `local`:
			Skip packages that already exist locally
		- `all`:
			Skip packages that already exist in any channel





### `test`

Run a test for a single package

This creates a temporary directory, copies the package file into it, and then runs the indexing. It then creates a test environment that installs the package and any extra dependencies specified in the package test dependencies file.

With the activated test environment, the packaged test files are run:

* `info/test/run_test.sh` or `info/test/run_test.bat` on Windows * `info/test/run_test.py`

These test files are written at "package creation time" and are part of the package.

**Usage:** `rattler-build test [OPTIONS] --package-file <PACKAGE_FILE>`

##### **Options:**

- `-c`, `--channel <CHANNEL>`

	Channels to use when testing


- `-p`, `--package-file <PACKAGE_FILE>`

	The package file to test


- `--compression-threads <COMPRESSION_THREADS>`

	The number of threads to use for compression


- `--use-zstd`

	Enable support for repodata.json.zst

	- Default value: `true`
	- Possible values: `true`, `false`


- `--use-bz2`

	Enable support for repodata.json.bz2

	- Default value: `true`
	- Possible values: `true`, `false`


- `--experimental`

	Enable experimental features

	- Possible values: `true`, `false`


- `--auth-file <AUTH_FILE>`

	Path to an auth-file to read authentication information from


###### **Modifying result**

- `--output-dir <OUTPUT_DIR>`

	Output directory for build artifacts.

	- Default value: `./output`




### `rebuild`

Rebuild a package from a package file instead of a recipe

**Usage:** `rattler-build rebuild [OPTIONS] --package-file <PACKAGE_FILE>`

##### **Options:**

- `-p`, `--package-file <PACKAGE_FILE>`

	The package file to rebuild


- `--no-test`

	Do not run tests after building

	- Default value: `false`
	- Possible values: `true`, `false`


- `--compression-threads <COMPRESSION_THREADS>`

	The number of threads to use for compression


- `--use-zstd`

	Enable support for repodata.json.zst

	- Default value: `true`
	- Possible values: `true`, `false`


- `--use-bz2`

	Enable support for repodata.json.bz2

	- Default value: `true`
	- Possible values: `true`, `false`


- `--experimental`

	Enable experimental features

	- Possible values: `true`, `false`


- `--auth-file <AUTH_FILE>`

	Path to an auth-file to read authentication information from


###### **Modifying result**

- `--output-dir <OUTPUT_DIR>`

	Output directory for build artifacts.

	- Default value: `./output`




### `upload`

Upload a package

**Usage:** `rattler-build upload [OPTIONS] [PACKAGE_FILES]... <COMMAND>`

##### **Subcommands:**

* `quetz` — Upload to aQuetz server. Authentication is used from the keychain / auth-file
* `artifactory` — Options for uploading to a Artifactory channel. Authentication is used from the keychain / auth-file
* `prefix` — Options for uploading to a prefix.dev server. Authentication is used from the keychain / auth-file
* `anaconda` — Options for uploading to a Anaconda.org server

##### **Arguments:**

- `<PACKAGE_FILES>`

	The package file to upload



##### **Options:**

- `--use-zstd`

	Enable support for repodata.json.zst

	- Default value: `true`
	- Possible values: `true`, `false`


- `--use-bz2`

	Enable support for repodata.json.bz2

	- Default value: `true`
	- Possible values: `true`, `false`


- `--experimental`

	Enable experimental features

	- Possible values: `true`, `false`


- `--auth-file <AUTH_FILE>`

	Path to an auth-file to read authentication information from


###### **Modifying result**

- `--output-dir <OUTPUT_DIR>`

	Output directory for build artifacts.

	- Default value: `./output`




#### `quetz`

Upload to aQuetz server. Authentication is used from the keychain / auth-file

**Usage:** `rattler-build upload quetz [OPTIONS] --url <URL> --channel <CHANNEL>`

##### **Options:**

- `-u`, `--url <URL>`

	The URL to your Quetz server


- `-c`, `--channel <CHANNEL>`

	The URL to your channel


- `-a`, `--api-key <API_KEY>`

	The Quetz API key, if none is provided, the token is read from the keychain / auth-file





#### `artifactory`

Options for uploading to a Artifactory channel. Authentication is used from the keychain / auth-file

**Usage:** `rattler-build upload artifactory [OPTIONS] --url <URL> --channel <CHANNEL>`

##### **Options:**

- `-u`, `--url <URL>`

	The URL to your Artifactory server


- `-c`, `--channel <CHANNEL>`

	The URL to your channel


- `-r`, `--username <USERNAME>`

	Your Artifactory username


- `-p`, `--password <PASSWORD>`

	Your Artifactory password





#### `prefix`

Options for uploading to a prefix.dev server. Authentication is used from the keychain / auth-file

**Usage:** `rattler-build upload prefix [OPTIONS] --channel <CHANNEL>`

##### **Options:**

- `-u`, `--url <URL>`

	The URL to the prefix.dev server (only necessary for self-hosted instances)

	- Default value: `https://prefix.dev`

- `-c`, `--channel <CHANNEL>`

	The channel to upload the package to


- `-a`, `--api-key <API_KEY>`

	The prefix.dev API key, if none is provided, the token is read from the keychain / auth-file





#### `anaconda`

Options for uploading to a Anaconda.org server

**Usage:** `rattler-build upload anaconda [OPTIONS] --owner <OWNER>`

##### **Options:**

- `-o`, `--owner <OWNER>`

	The owner of the distribution (e.g. conda-forge or your username)


- `-c`, `--channel <CHANNEL>`

	The channel / label to upload the package to (e.g. main / rc)

	- Default value: `main`

- `-a`, `--api-key <API_KEY>`

	The Anaconda API key, if none is provided, the token is read from the keychain / auth-file


- `-u`, `--url <URL>`

	The URL to the Anaconda server

	- Default value: `https://api.anaconda.org`

- `-f`, `--force`

	Replace files on conflict

	- Default value: `false`
	- Possible values: `true`, `false`





### `completion`

Generate shell completion script

**Usage:** `rattler-build completion [OPTIONS]`

##### **Options:**

- `-s`, `--shell <SHELL>`

	Shell

	- Possible values: `bash`, `elvish`, `fish`, `powershell`, `zsh`





### `generate-recipe`

Generate a recipe from PyPI or CRAN

**Usage:** `rattler-build generate-recipe <SOURCE> <PACKAGE>`

##### **Arguments:**

- `<SOURCE>`

	Type of package to generate a recipe for

	- Possible values:
		- `pypi`:
			Generate a recipe for a Python package from PyPI
		- `cran`:
			Generate a recipe for an R package from CRAN


- `<PACKAGE>`

	Name of the package to generate





### `auth`

Handle authentication to external channels

**Usage:** `rattler-build auth <COMMAND>`

##### **Subcommands:**

* `login` — Store authentication information for a given host
* `logout` — Remove authentication information for a given host



#### `login`

Store authentication information for a given host

**Usage:** `rattler-build auth login [OPTIONS] <HOST>`

##### **Arguments:**

- `<HOST>`

	The host to authenticate with (e.g. repo.prefix.dev)



##### **Options:**

- `--token <TOKEN>`

	The token to use (for authentication with prefix.dev)


- `--username <USERNAME>`

	The username to use (for basic HTTP authentication)


- `--password <PASSWORD>`

	The password to use (for basic HTTP authentication)


- `--conda-token <CONDA_TOKEN>`

	The token to use on anaconda.org / quetz authentication





#### `logout`

Remove authentication information for a given host

**Usage:** `rattler-build auth logout <HOST>`

##### **Arguments:**

- `<HOST>`

	The host to remove authentication for





<hr/>

<small><i>
    This document was generated automatically by
    <a href="https://crates.io/crates/clap-markdown"><code>clap-markdown</code></a>.
</i></small>
