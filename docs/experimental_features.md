# Experimental Features

```{warning}
These are experimental features of rattler-build and may change or go away completely.
```

Currently only `build` & `rebuild` command supports the following experimental features.

To enable them use the `--experimental` flag with the command.
Or, use the environment variable, `RATTLER_BUILD_EXPERIMENTAL=1`.

Jinja functions
---------------

### Load from files 

The jinja function `load_from_file` allows loading from files, specifically, it allows loading from `toml`, `json`
and `yaml` to an object to allow to fetch things directly from it. 
While it loads all other files as strings.

#### Usage 

This is useful when you have the project description in a well defined project file, such as, `Cargo.toml`, `package.json`, `pyproject.toml`, `package.yaml`, or `stack.yaml`. And would like to keep the recipe as simple as possible, while not worrying about keeping changes in sync, perhaps using it with CI/CD.

Or, from some other source that provides a well-defined output format. 

Example against `Cargo.toml` inside `rattler-build` github repository:

```yaml
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
  tag: ${{ source_tag }}}}

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

### Git functions

Git functions are useful for getting the latest tag and commit hash.
These can be used in the `context` section of the recipe, to fetch version information
from the git repository.

```
git.latest_tag( git_repo_url ) : latest tag in the repository
git.latest_tag_rev( git_repo_url ) : latest tag revision(aka, hash of tag commit) in the repository
git.head_rev( git_repo_url  ) : latest commit revision(aka, hash of head commit) in the repository
```

#### Usage

These can be useful for automating minor things inside the recipe itself.
Such as if the current version is the latest version, if the current hash is the latest hash, etc.

```yaml
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

Though it's important to understand currently we don't guarantee caching for repo fetch for git functions
this may lead to some performance issues.