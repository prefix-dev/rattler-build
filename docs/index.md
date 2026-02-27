## A fast conda package builder: `rattler-build`

The `rattler-build` tool creates cross-platform relocatable packages from a simple recipe format. 
The recipe format is heavily inspired by `conda-build` and `boa`, and the output of `rattler-build`
is a standard "conda" package that can be installed using [`pixi`](https://pixi.sh), 
[`mamba`](https://github.com/mamba-org/mamba) or [`conda`](https://docs.conda.io).

`rattler-build` is implemented in Rust, does not have any dependencies on `conda-build` or Python and works as a standalone binary.

You can use `rattler-build` to publish packages to prefix.dev, anaconda.org, JFrog Artifactory, S3 buckets, or Quetz Servers.

![](https://user-images.githubusercontent.com/885054/244683824-fd1b3896-84c7-498c-b406-40ab2a9e450c.svg)

## Installation

The recommended way of installing `rattler-build`, being a conda-package builder, is through a conda package manager.
Next to `rattler-build` we are also building [`pixi`](https://pixi.sh).

With `pixi` you can install `rattler-build` globally:

```bash
pixi global install rattler-build
```

Other options are:

=== "Conda"
    ```bash
    conda install rattler-build -c conda-forge

    mamba install rattler-build -c conda-forge
    micromamba install rattler-build -c conda-forge

    pixi global install rattler-build
    pixi add rattler-build # To a pixi project
    ```

=== "Homebrew"
    ```bash
    brew install rattler-build
    ```
=== "Arch Linux"
    ```bash
    pacman -S rattler-build
    ```

=== "Binary"
    ```bash
    # Download the latest release from the GitHub releases page, for example the linux x86 version with curl:
    curl -SL --progress-bar https://github.com/prefix-dev/rattler-build/releases/latest/download/rattler-build-x86_64-unknown-linux-musl
    ```
    You can grab version of `rattler-build` from the [Github
    Releases](https://github.com/prefix-dev/rattler-build/releases/).

??? note "Shell Completion"
    When installing `rattler-build` you might want to enable shell completion.
    Afterwards, restart the shell or source the shell config file.

    === "Bash"
        ```bash
        echo 'eval "$(rattler-build completion --shell bash)"' >> ~/.bashrc
        ```

    === "Zsh"
        ```zsh
        echo 'eval "$(rattler-build completion --shell zsh)"' >> ~/.zshrc
        ```

    === "PowerShell"
        ```pwsh
        Add-Content -Path $PROFILE -Value '(& rattler-build completion --shell powershell) | Out-String | Invoke-Expression'
        ```

        !!! tip "Failure because no profile file exists"
            Make sure your profile file exists, otherwise create it with:
            ```PowerShell
            New-Item -Path $PROFILE -ItemType File -Force
            ```

    === "Fish"
        ```fish
        echo 'rattler-build completion --shell fish | source' >> ~/.config/fish/config.fish
        ```

    === "Nushell"
        Add the following to the end of your Nushell env file (find it by running `$nu.env-path` in Nushell):

        ```nushell
        mkdir ~/.cache/rattler-build
        rattler-build completion --shell nushell | save -f ~/.cache/rattler-build/completions.nu
        ```

        And add the following to the end of your Nushell configuration (find it by running `$nu.config-path`):

        ```nushell
        use ~/.cache/rattler-build/completions.nu *
        ```

    === "Elvish"
        ```elv
        echo 'eval (rattler-build completion --shell elvish | slurp)' >> ~/.elvish/rc.elv
        ```

### Dependencies

Currently `rattler-build` needs some dependencies on the host system which are
executed as subprocess. We plan to reduce the number of external dependencies
over time by writing what we need in Rust to make `rattler-build` fully
self-contained.

* `install_name_tool` is necessary on macOS to rewrite the `rpath` of shared
  libraries and executables to make it relative
* `patchelf` is required on Linux to rewrite the `rpath` and `runpath` of shared
  libraries and executables
* `git` to checkout Git repositories (not implemented yet, but will require `git`
  in the future)
* `msvc` on Windows because we cannot ship the MSVC compiler on conda-forge
  (needs to be installed on the host machine)

### GitHub Action

There is a GitHub Action for `rattler-build`. It can be used to install `rattler-build` in CI/CD workflows and run a build command. Please check out the [GitHub Action documentation](https://github.com/prefix-dev/rattler-build-action) for more information.

## The Recipe Format

> **Note** You can find all examples below in the [`examples`](https://github.com/prefix-dev/rattler-build/tree/main/examples)
> folder in the codebase and run them with `rattler-build`.

A simple example recipe for the `xtensor` header-only C++ library:

```yaml title="recipe.yaml"
--8<-- "docs/snippets/recipes/xtensor-index.yaml"
```

A recipe for the `rich` Python package (using `noarch`):

```yaml title="recipe.yaml"
--8<-- "docs/snippets/recipes/rich-index.yaml"
```

A recipe for the `curl` library:

```yaml title="recipe.yaml"
--8<-- "docs/snippets/recipes/curl-index.yaml"
```

For the `curl` library recipe, two additional script files (`build.sh` and `build.bat`) are needed.

```bash title="build.sh"
#!/bin/bash

# Get an updated config.sub and config.guess
cp $BUILD_PREFIX/share/libtool/build-aux/config.* .

if [[ $target_platform =~ linux.* ]]; then
    USESSL="--with-openssl=${PREFIX}"
else
    USESSL="--with-secure-transport"
fi;

./configure \
    --prefix=${PREFIX} \
    --host=${HOST} \
    ${USESSL} \
    --with-ca-bundle=${PREFIX}/ssl/cacert.pem \
    --disable-static --enable-shared

make -j${CPU_COUNT} ${VERBOSE_AT}
make install

# Includes man pages and other miscellaneous.
rm -rf "${PREFIX}/share"
```

Or on Windows:

```cmd title="build.bat"
mkdir build

cmake -GNinja ^
      -DCMAKE_BUILD_TYPE=Release ^
      -DBUILD_SHARED_LIBS=ON ^
      -DCMAKE_INSTALL_PREFIX=%LIBRARY_PREFIX% ^
      -DCMAKE_PREFIX_PATH=%LIBRARY_PREFIX% ^
      -DCURL_USE_SCHANNEL=ON ^
      -DCURL_USE_LIBSSH2=OFF ^
      -DUSE_ZLIB=ON ^
      -DENABLE_UNICODE=ON ^
      %SRC_DIR%

IF %ERRORLEVEL% NEQ 0 exit 1

ninja install --verbose
```
