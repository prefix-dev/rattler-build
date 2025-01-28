# Python bindings

These are the API docs for the `rattler_build` Python bindings.
The full parameter list can be found be expanding the source code.
Every function corresponds to a CLI command.
For a description of its parameters and behavior we therefore refer to the [CLI reference](./cli.md).


### `build_recipes(recipes, **kwargs)`
::: rattler_build.build_recipes

### `test_package(package_file, **kwargs)`
::: rattler_build.test_package

### `upload_package_to_quetz(package_files, url, channels, **kwargs)`
::: rattler_build.upload_package_to_quetz

### `upload_package_to_artifactory(package_files, url, channels, **kwargs)`
::: rattler_build.upload_package_to_artifactory

### `upload_package_to_prefix(package_files, url, channels, **kwargs)`
::: rattler_build.upload_package_to_prefix

### `upload_package_to_anaconda(package_files, owner, **kwargs)`
::: rattler_build.upload_package_to_anaconda

### `upload_packages_to_conda_forge(package_files, staging_token, feedstock, feedstock_token, **kwargs)`
::: rattler_build.upload_packages_to_conda_forge
