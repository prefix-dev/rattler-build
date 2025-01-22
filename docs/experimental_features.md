# Experimental features

!!! warning
    These are experimental features of `rattler-build` and may change or go away completely.


Currently only the `build` and `rebuild` commands support the following experimental features.

To enable them, use the `--experimental` flag with the command.
Or, use the environment variable, `RATTLER_BUILD_EXPERIMENTAL=true`.

## Jinja functions

### `load_from_file(<file_path>)`

The Jinja function `load_from_file` allows loading from files; specifically, it allows loading from `toml`, `json`,
and `yaml` file types to an object to allow it to fetch things directly from the file.
It loads all other files as strings.

#### Usage

`load_from_file` is useful when there is a project description in a well-defined project file such as `Cargo.toml`, `package.json`, `pyproject.toml`, `package.yaml`, or `stack.yaml`. It enables the recipe to be preserved in as simple a state as possible, especially when there is no need to keep the changes in sync; some example use cases for this are with CI/CD infrastructure or when there is a well-defined output format.

Below is an example loading a `Cargo.toml` inside of the `rattler-build` GitHub repository:

``` yaml title="recipe.yaml"
context:
  name: ${{ load_from_file("Cargo.toml").package.name }}
  version: ${{ load_from_file("Cargo.toml").package.version }}
  source_url: ${{ load_from_file("Cargo.toml").package.homepage }}
  rust_toolchain: ${{ load_from_file("rust-toolchains") }}

package:
  name: ${{ name }}
  version: ${{ version }}

source:
  git: ${{ source_url }}
  tag: ${{ source_tag }}

requirements:
  build:
    - rust ==${{ rust_toolchain }}

build:
  script: cargo build --release -p ${{ name }}

test:
  - script: cargo test -p ${{ name }}
  - script: cargo test -p rust-test -- --test-threads=1

about:
  home: ${{ source_url }}
  repository: ${{ source_url }}
  documentation: ${{ load_from_file("Cargo.toml").package.documentation }}
  summary: ${{ load_from_file("Cargo.toml").package.description }}
  license: ${{ load_from_file("Cargo.toml").package.license }}
```

### `git` functions

`git` functions are useful for getting the latest tag and commit hash.
These can be used in the `context` section of the recipe, to fetch version information
from a repository.

???+ example "Examples"
    ```python
    # latest tag in the repo
    git.latest_tag(<git_repo_url>)

    # latest tag revision(aka, hash of tag commit) in the repo
    git.latest_tag_rev(<git_repo_url>)

    # latest commit revision(aka, hash of head commit) in the repo
    git.head_rev(<git_repo_url>)
    ```

#### Usage

These can be useful for automating minor things inside of the recipe itself, such as if the current version is the latest version or if the current hash is the latest hash, etc.

``` yaml title="recipe.yaml"
context:
  git_repo_url: "https://github.com/prefix-dev/rattler-build"
  latest_tag: ${{ git.latest_tag( git_repo_url ) }}

package:
  name: "rattler-build"
  version: ${{ latest_tag }}

source:
  git: ${{ git_repo_url }}
  tag: ${{ latest_tag }}
```

There is currently no guarantee of caching for repo fetches when using `git` functions. This may lead to some performance issues.
