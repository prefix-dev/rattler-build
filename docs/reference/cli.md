# Command-Line Help for `rattler-build`

This document contains the help content for the `rattler-build` command-line program.

## `rattler-build`

**Usage:** `rattler-build [OPTIONS] [COMMAND]`

##### **Subcommands:**

* `build` — Build a package from a recipe
* `publish` — Publish packages to a channel. This command builds packages from recipes (or uses already built packages), uploads them to a channel, and runs indexing
* `test` — Run a test for a single package
* `rebuild` — Rebuild a package from a package file instead of a recipe
* `upload` — Upload a package
* `completion` — Generate shell completion script
* `generate-recipe` — Generate a recipe from PyPI, CRAN, CPAN, or LuaRocks
* `auth` — Handle authentication to external channels
* `debug` — Debug a recipe by setting up the environment without running the build script
* `create-patch` — Create a patch for a directory
* `debug-shell` — Open a debug shell in the build environment
* `package` — Package-related subcommands
* `bump-recipe` — Bump a recipe to a new version

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


- `--wrap-log-lines <WRAP_LOG_LINES>`

	Wrap log lines at the terminal width. This is automatically disabled on CI (by detecting the `CI` environment variable)

	- Possible values: `true`, `false`


- `--config-file <CONFIG_FILE>`

	The rattler-build configuration file to use


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

- `-r`, `--recipe <RECIPES>`

	The recipe file or directory containing `recipe.yaml`. Defaults to the current directory

	- Default value: `.`

- `--recipe-dir <RECIPE_DIR>`

	The directory that contains recipes


- `--up-to <UP_TO>`

	Build recipes up to the specified package


- `--build-platform <BUILD_PLATFORM>`

	The build platform to use for the build (e.g. for building with emulation, or rendering)


- `--target-platform <TARGET_PLATFORM>`

	The target platform for the build


- `--host-platform <HOST_PLATFORM>`

	The host platform for the build. If set, it will be used to determine also the target_platform (as long as it is not noarch)


- `-c`, `--channel <CHANNELS>`

	Add a channel to search for dependencies in


- `-m`, `--variant-config <VARIANT_CONFIG>`

	Variant configuration files for the build


- `--variant <VARIANT_OVERRIDES>`

	Override specific variant values (e.g. --variant python=3.12 or --variant python=3.12,3.11). Multiple values separated by commas will create multiple build variants


- `--ignore-recipe-variants`

	Do not read the `variants.yaml` file next to a recipe


- `--render-only`

	Render the recipe files without executing the build


- `--with-solve`

	Render the recipe files with solving dependencies


- `--keep-build`

	Keep intermediate build artifacts after the build


- `--no-build-id`

	Don't use build id(timestamp) when creating build directory name


- `--compression-threads <COMPRESSION_THREADS>`

	The number of threads to use for compression (only relevant when also using `--package-format conda`)


- `--io-concurrency-limit <IO_CONCURRENCY_LIMIT>`

	The maximum number of concurrent I/O operations to use when installing packages This can be controlled by the `RATTLER_IO_CONCURRENCY_LIMIT` environment variable Defaults to 8 times the number of CPUs


- `--experimental`

	Enable experimental features


- `--allow-insecure-host <ALLOW_INSECURE_HOST>`

	List of hosts for which SSL certificate verification should be skipped


- `--channel-priority <CHANNEL_PRIORITY>`

	Channel priority to use when solving


- `--extra-meta <EXTRA_META>`

	Extra metadata to include in about.json


- `--continue-on-failure`

	Continue building even if (one) of the packages fails to build. This is useful when building many packages with `--recipe-dir`.`


###### **Modifying result**

- `--package-format <PACKAGE_FORMAT>`

	The package format to use for the build. Can be one of `tar-bz2` or
`conda`. You can also add a compression level to the package format,
e.g. `tar-bz2:<number>` (from 1 to 9) or `conda:<number>` (from -7 to
22).


- `--no-include-recipe`

	Don't store the recipe in the final package


- `--test <TEST>`

	The strategy to use for running tests

	- Possible values:
		- `skip`:
			Skip the tests
		- `native`:
			Run the tests only if the build platform is the same as the host platform. Otherwise, skip the tests. If the target platform is noarch, the tests are always executed
		- `native-and-emulated`:
			Always run the tests


- `--color-build-log`

	Don't force colors in the output of the build script


- `--output-dir <OUTPUT_DIR>`

	Output directory for build artifacts.


- `--skip-existing <SKIP_EXISTING>`

	Whether to skip packages that already exist in any channel If set to `none`, do not skip any packages, default when not specified. If set to `local`, only skip packages that already exist locally, default when using `--skip-existing. If set to `all`, skip packages that already exist in any channel

	- Possible values:
		- `none`:
			Do not skip any packages
		- `local`:
			Skip packages that already exist locally
		- `all`:
			Skip packages that already exist in any channel


- `--noarch-build-platform <NOARCH_BUILD_PLATFORM>`

	Define a "noarch platform" for which the noarch packages will be built for. The noarch builds will be skipped on the other platforms


- `--debug`

	Enable debug output in build scripts


- `--error-prefix-in-binary`

	Error if the host prefix is detected in any binary files


- `--allow-symlinks-on-windows`

	Allow symlinks in packages on Windows (defaults to false - symlinks are forbidden on Windows)


- `--exclude-newer <EXCLUDE_NEWER>`

	Exclude packages newer than this date from the solver, in RFC3339 format (e.g. 2024-03-15T12:00:00Z)


- `--build-num <BUILD_NUM>`

	Override the build number for all outputs (defaults to the build number in the recipe)


###### **Sandbox arguments**

- `--sandbox`

	Enable the sandbox


- `--allow-network`

	Allow network access during build (default: false if sandbox is enabled)


- `--allow-read <ALLOW_READ>`

	Allow read access to the specified paths


- `--allow-read-execute <ALLOW_READ_EXECUTE>`

	Allow read and execute access to the specified paths


- `--allow-read-write <ALLOW_READ_WRITE>`

	Allow read and write access to the specified paths


- `--overwrite-default-sandbox-config`

	Overwrite the default sandbox configuration





### `publish`

Publish packages to a channel. This command builds packages from recipes (or uses already built packages), uploads them to a channel, and runs indexing

**Usage:** `rattler-build publish [OPTIONS] --to <TO> [PACKAGE_OR_RECIPE]...`

##### **Arguments:**

- `<PACKAGE_OR_RECIPE>`

	Package files (*.conda, *.tar.bz2) to publish directly, or recipe files (*.yaml) to build and publish. If .conda or .tar.bz2 files are provided, they will be published directly without building. If .yaml files are provided, they will be built first, then published. Use --recipe-dir (from build options below) to scan a directory for recipes instead. Defaults to "recipe.yaml" in the current directory if not specified

	- Default value: `recipe.yaml`


##### **Options:**

- `-r`, `--recipe <RECIPES>`

	The recipe file or directory containing `recipe.yaml`. Defaults to the current directory

	- Default value: `.`

- `--recipe-dir <RECIPE_DIR>`

	The directory that contains recipes


- `--up-to <UP_TO>`

	Build recipes up to the specified package


- `--build-platform <BUILD_PLATFORM>`

	The build platform to use for the build (e.g. for building with emulation, or rendering)


- `--target-platform <TARGET_PLATFORM>`

	The target platform for the build


- `--host-platform <HOST_PLATFORM>`

	The host platform for the build. If set, it will be used to determine also the target_platform (as long as it is not noarch)


- `-c`, `--channel <CHANNELS>`

	Add a channel to search for dependencies in


- `-m`, `--variant-config <VARIANT_CONFIG>`

	Variant configuration files for the build


- `--variant <VARIANT_OVERRIDES>`

	Override specific variant values (e.g. --variant python=3.12 or --variant python=3.12,3.11). Multiple values separated by commas will create multiple build variants


- `--ignore-recipe-variants`

	Do not read the `variants.yaml` file next to a recipe


- `--render-only`

	Render the recipe files without executing the build


- `--with-solve`

	Render the recipe files with solving dependencies


- `--keep-build`

	Keep intermediate build artifacts after the build


- `--no-build-id`

	Don't use build id(timestamp) when creating build directory name


- `--compression-threads <COMPRESSION_THREADS>`

	The number of threads to use for compression (only relevant when also using `--package-format conda`)


- `--io-concurrency-limit <IO_CONCURRENCY_LIMIT>`

	The maximum number of concurrent I/O operations to use when installing packages This can be controlled by the `RATTLER_IO_CONCURRENCY_LIMIT` environment variable Defaults to 8 times the number of CPUs


- `--experimental`

	Enable experimental features


- `--allow-insecure-host <ALLOW_INSECURE_HOST>`

	List of hosts for which SSL certificate verification should be skipped


- `--channel-priority <CHANNEL_PRIORITY>`

	Channel priority to use when solving


- `--extra-meta <EXTRA_META>`

	Extra metadata to include in about.json


- `--continue-on-failure`

	Continue building even if (one) of the packages fails to build. This is useful when building many packages with `--recipe-dir`.`


###### **Modifying result**

- `--package-format <PACKAGE_FORMAT>`

	The package format to use for the build. Can be one of `tar-bz2` or
`conda`. You can also add a compression level to the package format,
e.g. `tar-bz2:<number>` (from 1 to 9) or `conda:<number>` (from -7 to
22).


- `--no-include-recipe`

	Don't store the recipe in the final package


- `--test <TEST>`

	The strategy to use for running tests

	- Possible values:
		- `skip`:
			Skip the tests
		- `native`:
			Run the tests only if the build platform is the same as the host platform. Otherwise, skip the tests. If the target platform is noarch, the tests are always executed
		- `native-and-emulated`:
			Always run the tests


- `--color-build-log`

	Don't force colors in the output of the build script


- `--output-dir <OUTPUT_DIR>`

	Output directory for build artifacts.


- `--skip-existing <SKIP_EXISTING>`

	Whether to skip packages that already exist in any channel If set to `none`, do not skip any packages, default when not specified. If set to `local`, only skip packages that already exist locally, default when using `--skip-existing. If set to `all`, skip packages that already exist in any channel

	- Possible values:
		- `none`:
			Do not skip any packages
		- `local`:
			Skip packages that already exist locally
		- `all`:
			Skip packages that already exist in any channel


- `--noarch-build-platform <NOARCH_BUILD_PLATFORM>`

	Define a "noarch platform" for which the noarch packages will be built for. The noarch builds will be skipped on the other platforms


- `--debug`

	Enable debug output in build scripts


- `--error-prefix-in-binary`

	Error if the host prefix is detected in any binary files


- `--allow-symlinks-on-windows`

	Allow symlinks in packages on Windows (defaults to false - symlinks are forbidden on Windows)


- `--exclude-newer <EXCLUDE_NEWER>`

	Exclude packages newer than this date from the solver, in RFC3339 format (e.g. 2024-03-15T12:00:00Z)


- `--build-num <BUILD_NUM>`

	Override the build number for all outputs (defaults to the build number in the recipe)


###### **Publishing**

- `--to <TO>`

	The channel or URL to publish the package to.
	
	Examples: - prefix.dev: https://prefix.dev/my-channel - anaconda.org: https://anaconda.org/my-org - S3: s3://my-bucket - Filesystem: file:///path/to/channel or /path/to/channel - Quetz: quetz://server.company.com/channel - Artifactory: artifactory://server.company.com/channel
	
	Note: This channel is also used as the highest priority channel when solving dependencies.


- `--build-number <BUILD_NUMBER>`

	Override the build number for all outputs. Use an absolute value (e.g., `--build-number=12`) or a relative bump (e.g., `--build-number=+1`). When using a relative bump, the highest build number from the target channel is used as the base


- `--force`

	Force upload even if the package already exists (not recommended - may break lockfiles). Only works with S3, filesystem, Anaconda.org, and prefix.dev channels


- `--generate-attestation`

	Automatically generate attestations when uploading to prefix.dev channels. Only works when uploading to prefix.dev channels with trusted publishing enabled


###### **Sandbox arguments**

- `--sandbox`

	Enable the sandbox


- `--allow-network`

	Allow network access during build (default: false if sandbox is enabled)


- `--allow-read <ALLOW_READ>`

	Allow read access to the specified paths


- `--allow-read-execute <ALLOW_READ_EXECUTE>`

	Allow read and execute access to the specified paths


- `--allow-read-write <ALLOW_READ_WRITE>`

	Allow read and write access to the specified paths


- `--overwrite-default-sandbox-config`

	Overwrite the default sandbox configuration





### `test`

Run a test for a single package

This creates a temporary directory, copies the package file into it, and then runs the indexing. It then creates a test environment that installs the package and any extra dependencies specified in the package test dependencies file.

With the activated test environment, the packaged test files are run:

* `info/test/run_test.sh` or `info/test/run_test.bat` on Windows * `info/test/run_test.py`

These test files are written at "package creation time" and are part of the package.

**Usage:** `rattler-build test [OPTIONS] --package-file <PACKAGE_FILE>`

##### **Options:**

- `-c`, `--channel <CHANNELS>`

	Channels to use when testing


- `-p`, `--package-file <PACKAGE_FILE>`

	The package file to test


- `--compression-threads <COMPRESSION_THREADS>`

	The number of threads to use for compression


- `--test-index <TEST_INDEX>`

	The index of the test to run. This is used to run a specific test from the package


- `--debug`

	Build test environment and output debug information for manual debugging


- `--experimental`

	Enable experimental features


- `--allow-insecure-host <ALLOW_INSECURE_HOST>`

	List of hosts for which SSL certificate verification should be skipped


- `--channel-priority <CHANNEL_PRIORITY>`

	Channel priority to use when solving


###### **Modifying result**

- `--output-dir <OUTPUT_DIR>`

	Output directory for build artifacts.





### `rebuild`

Rebuild a package from a package file instead of a recipe

**Usage:** `rattler-build rebuild [OPTIONS] --package-file <PACKAGE_FILE>`

##### **Options:**

- `-p`, `--package-file <PACKAGE_FILE>`

	The package file to rebuild (can be a local path or URL)


- `--compression-threads <COMPRESSION_THREADS>`

	The number of threads to use for compression


- `--io-concurrency-limit <IO_CONCURRENCY_LIMIT>`

	The number of threads to use for I/O operations when installing packages


- `--experimental`

	Enable experimental features


- `--allow-insecure-host <ALLOW_INSECURE_HOST>`

	List of hosts for which SSL certificate verification should be skipped


- `--channel-priority <CHANNEL_PRIORITY>`

	Channel priority to use when solving


###### **Modifying result**

- `--test <TEST>`

	The strategy to use for running tests

	- Possible values:
		- `skip`:
			Skip the tests
		- `native`:
			Run the tests only if the build platform is the same as the host platform. Otherwise, skip the tests. If the target platform is noarch, the tests are always executed
		- `native-and-emulated`:
			Always run the tests


- `--output-dir <OUTPUT_DIR>`

	Output directory for build artifacts.





### `upload`

Upload a package

**Usage:** `rattler-build upload [OPTIONS] [PACKAGE_FILES]... <COMMAND>`

##### **Subcommands:**

* `quetz` — Upload to a Quetz server. Authentication is used from the keychain / auth-file
* `artifactory` — Options for uploading to a Artifactory channel. Authentication is used from the keychain / auth-file
* `prefix` — Options for uploading to a prefix.dev server. Authentication is used from the keychain / auth-file
* `anaconda` — Options for uploading to a Anaconda.org server
* `s3` — Options for uploading to S3

##### **Arguments:**

- `<PACKAGE_FILES>`

	The package file to upload



##### **Options:**

- `--allow-insecure-host <ALLOW_INSECURE_HOST>`

	List of hosts for which SSL certificate verification should be skipped





#### `quetz`

Upload to a Quetz server. Authentication is used from the keychain / auth-file

**Usage:** `rattler-build upload quetz [OPTIONS] --url <URL> --channel <CHANNELS>`

##### **Options:**

- `-u`, `--url <URL>`

	The URL to your Quetz server


- `-c`, `--channel <CHANNELS>`

	The URL to your channel


- `-a`, `--api-key <API_KEY>`

	The Quetz API key, if none is provided, the token is read from the keychain / auth-file





#### `artifactory`

Options for uploading to a Artifactory channel. Authentication is used from the keychain / auth-file

**Usage:** `rattler-build upload artifactory [OPTIONS] --url <URL> --channel <CHANNELS>`

##### **Options:**

- `-u`, `--url <URL>`

	The URL to your Artifactory server


- `-c`, `--channel <CHANNELS>`

	The URL to your channel


- `-t`, `--token <TOKEN>`

	Your Artifactory token





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


- `--attestation <ATTESTATION>`

	Upload an attestation file alongside the package. Note: if you add an attestation, you can _only_ upload a single package. Mutually exclusive with --generate-attestation


- `--generate-attestation`

	Automatically generate attestation using cosign in CI. Mutually exclusive with --attestation


- `-s`, `--skip-existing`

	Skip upload if package already exists


- `--force`

	Force overwrite existing packages





#### `anaconda`

Options for uploading to a Anaconda.org server

**Usage:** `rattler-build upload anaconda [OPTIONS] --owner <OWNER>`

##### **Options:**

- `-o`, `--owner <OWNER>`

	The owner of the distribution (e.g. conda-forge or your username)


- `-c`, `--channel <CHANNELS>`

	The channel / label to upload the package to (e.g. main / rc)


- `-a`, `--api-key <API_KEY>`

	The Anaconda API key, if none is provided, the token is read from the keychain / auth-file


- `-u`, `--url <URL>`

	The URL to the Anaconda server


- `-f`, `--force`

	Replace files on conflict





#### `s3`

Options for uploading to S3

**Usage:** `rattler-build upload s3 [OPTIONS] --channel <CHANNEL>`

##### **Options:**

- `-c`, `--channel <CHANNEL>`

	The channel URL in the S3 bucket to upload the package to, e.g., `s3://my-bucket/my-channel`


- `--force`

	Replace files if it already exists


###### **S3 Credentials**

- `--endpoint-url <ENDPOINT_URL>`

	The endpoint URL of the S3 backend


- `--region <REGION>`

	The region of the S3 backend


- `--access-key-id <ACCESS_KEY_ID>`

	The access key ID for the S3 bucket


- `--secret-access-key <SECRET_ACCESS_KEY>`

	The secret access key for the S3 bucket


- `--session-token <SESSION_TOKEN>`

	The session token for the S3 bucket


- `--addressing-style <ADDRESSING_STYLE>`

	How to address the bucket

	- Default value: `virtual-host`
	- Possible values: `virtual-host`, `path`





### `completion`

Generate shell completion script

**Usage:** `rattler-build completion --shell <SHELL>`

##### **Options:**

- `-s`, `--shell <SHELL>`

	Specifies the shell for which the completions should be generated

	- Possible values:
		- `bash`:
			Bourne Again SHell (bash)
		- `elvish`:
			Elvish shell
		- `fish`:
			Friendly Interactive SHell (fish)
		- `nushell`:
			Nushell
		- `powershell`:
			PowerShell
		- `zsh`:
			Z SHell (zsh)





### `generate-recipe`

Generate a recipe from PyPI, CRAN, CPAN, or LuaRocks

**Usage:** `rattler-build generate-recipe <COMMAND>`

##### **Subcommands:**

* `pypi` — Generate a recipe for a Python package from PyPI
* `cran` — Generate a recipe for an R package from CRAN
* `cpan` — Generate a recipe for a Perl package from CPAN
* `luarocks` — Generate a recipe for a Lua package from LuaRocks



#### `pypi`

Generate a recipe for a Python package from PyPI

**Usage:** `rattler-build generate-recipe pypi [OPTIONS] <PACKAGE>`

##### **Arguments:**

- `<PACKAGE>`

	Name of the package to generate



##### **Options:**

- `--version <VERSION>`

	Select a version of the package to generate (defaults to latest)


- `-w`, `--write`

	Whether to write the recipe to a folder


- `-u`, `--use-mapping`

	Whether to use the conda-forge PyPI name mapping


- `-t`, `--tree`

	Whether to generate recipes for all dependencies





#### `cran`

Generate a recipe for an R package from CRAN

**Usage:** `rattler-build generate-recipe cran [OPTIONS] <PACKAGE>`

##### **Arguments:**

- `<PACKAGE>`

	Name of the package to generate



##### **Options:**

- `-u`, `--universe <UNIVERSE>`

	The R Universe to fetch the package from (defaults to `cran`)


- `-t`, `--tree`

	Whether to create recipes for the whole dependency tree or not


- `-w`, `--write`

	Whether to write the recipe to a folder





#### `cpan`

Generate a recipe for a Perl package from CPAN

**Usage:** `rattler-build generate-recipe cpan [OPTIONS] <PACKAGE>`

##### **Arguments:**

- `<PACKAGE>`

	Name of the package to generate



##### **Options:**

- `--version <VERSION>`

	Select a version of the package to generate (defaults to latest)


- `-w`, `--write`

	Whether to write the recipe to a folder


- `-t`, `--tree`

	Whether to generate recipes for all dependencies





#### `luarocks`

Generate a recipe for a Lua package from LuaRocks

**Usage:** `rattler-build generate-recipe luarocks [OPTIONS] <ROCK>`

##### **Arguments:**

- `<ROCK>`

	Luarocks package to generate recipe for. Can be specified as: - module (fetches latest version) - module/version - author/module/version - Direct rockspec URL



##### **Options:**

- `-w`, `--write-to <WRITE_TO>`

	Where to write the recipe to

	- Default value: `.`




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

	The host to authenticate with (e.g. prefix.dev)



##### **Options:**

- `--token <TOKEN>`

	The token to use (for authentication with prefix.dev)


- `--username <USERNAME>`

	The username to use (for basic HTTP authentication)


- `--password <PASSWORD>`

	The password to use (for basic HTTP authentication)


- `--conda-token <CONDA_TOKEN>`

	The token to use on anaconda.org / quetz authentication


- `--s3-access-key-id <S3_ACCESS_KEY_ID>`

	The S3 access key ID


- `--s3-secret-access-key <S3_SECRET_ACCESS_KEY>`

	The S3 secret access key


- `--s3-session-token <S3_SESSION_TOKEN>`

	The S3 session token





#### `logout`

Remove authentication information for a given host

**Usage:** `rattler-build auth logout <HOST>`

##### **Arguments:**

- `<HOST>`

	The host to remove authentication for





### `debug`

Debug a recipe by setting up the environment without running the build script

**Usage:** `rattler-build debug [OPTIONS] --recipe <RECIPE>`

##### **Options:**

- `-r`, `--recipe <RECIPE>`

	Recipe file to debug


- `-o`, `--output <OUTPUT>`

	Output directory for build artifacts


- `--target-platform <TARGET_PLATFORM>`

	The target platform to build for


- `--host-platform <HOST_PLATFORM>`

	The host platform to build for (defaults to target_platform)


- `--build-platform <BUILD_PLATFORM>`

	The build platform to build for (defaults to current platform)


- `-c`, `--channel <CHANNELS>`

	Channels to use when building


- `--experimental`

	Enable experimental features


- `--allow-insecure-host <ALLOW_INSECURE_HOST>`

	List of hosts for which SSL certificate verification should be skipped


- `--channel-priority <CHANNEL_PRIORITY>`

	Channel priority to use when solving


- `--output-name <OUTPUT_NAME>`

	Name of the specific output to debug


###### **Modifying result**

- `--output-dir <OUTPUT_DIR>`

	Output directory for build artifacts.





### `create-patch`

Create a patch for a directory

**Usage:** `rattler-build create-patch [OPTIONS]`

##### **Options:**

- `-d`, `--directory <DIRECTORY>`

	Directory where we want to create the patch. Defaults to current directory if not specified


- `--name <NAME>`

	The name for the patch file to create

	- Default value: `changes`

- `--overwrite`

	Whether to overwrite the patch file if it already exists


- `--patch-dir <DIR>`

	Optional directory where the patch file should be written. Defaults to the recipe directory determined from `.source_info.json` if not provided


- `--exclude <EXCLUDE>`

	Comma-separated list of file names (or glob patterns) that should be excluded from the diff


- `--add <ADD>`

	Include new files matching these glob patterns (e.g., "*.txt", "src/**/*.rs")


- `--include <INCLUDE>`

	Only include modified files matching these glob patterns (e.g., "*.c", "src/**/*.rs") If not specified, all modified files are included (subject to --exclude)


- `--dry-run`

	Perform a dry-run: analyze changes and log the diff, but don't write the patch file





### `debug-shell`

Open a debug shell in the build environment

**Usage:** `rattler-build debug-shell [OPTIONS]`

##### **Options:**

- `--work-dir <WORK_DIR>`

	Work directory to use (reads from last build in rattler-build-log.txt if not specified)


- `-o`, `--output-dir <OUTPUT_DIR>`

	Output directory containing rattler-build-log.txt

	- Default value: `./output`




### `package`

Package-related subcommands

**Usage:** `rattler-build package <COMMAND>`

##### **Subcommands:**

* `inspect` — Inspect and display information about a built package
* `extract` — Extract a conda package to a directory



#### `inspect`

Inspect and display information about a built package

**Usage:** `rattler-build package inspect [OPTIONS] <PACKAGE_FILE>`

##### **Arguments:**

- `<PACKAGE_FILE>`

	Path to the package file (.conda, .tar.bz2)



##### **Options:**

- `--paths`

	Show detailed file listing with hashes and sizes


- `--about`

	Show extended about information


- `--run-exports`

	Show run exports


- `--all`

	Show all available information


- `--json`

	Output as JSON





#### `extract`

Extract a conda package to a directory

**Usage:** `rattler-build package extract [OPTIONS] <PACKAGE_FILE>`

##### **Arguments:**

- `<PACKAGE_FILE>`

	Path to the package file (.conda, .tar.bz2) or a URL to download from



##### **Options:**

- `-d`, `--dest <DEST>`

	Destination directory for extraction (defaults to package name without extension)





### `bump-recipe`

Bump a recipe to a new version

This command updates the version and SHA256 checksum(s) in a recipe file. It can either use a specified version or auto-detect the latest version from supported providers (GitHub, PyPI, crates.io).

**Usage:** `rattler-build bump-recipe [OPTIONS]`

##### **Options:**

- `-r`, `--recipe <RECIPE>`

	Path to the recipe file (recipe.yaml). Defaults to current directory

	- Default value: `.`

- `--version <VERSION>`

	The new version to bump to. If not specified, will auto-detect the latest version from the source URL's provider (GitHub, PyPI, crates.io)


- `--include-prerelease`

	Include pre-release versions when auto-detecting (e.g., alpha, beta, rc)


- `--check-only`

	Only check for updates without modifying the recipe


- `--dry-run`

	Perform a dry-run: show what would be changed without writing to the file


- `--keep-build-number`

	Keep the current build number instead of resetting it to 0





<hr/>

<small><i>
    This document was generated automatically by
    <a href="https://crates.io/crates/clap-markdown"><code>clap-markdown</code></a>.
</i></small>
