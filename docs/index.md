<h1>
  <a href="https://github.com/prefix-dev/rattler-build/">
    <img alt="banner" src="https://github.com/prefix-dev/rattler-build/assets/885054/3bad9a38-939d-4513-8c61-dcc4ddb7fb51">
  </a>
</h1>

# `rattler-build`: A Fast Conda Package Builder

The `rattler-build` tooling and library creates cross-platform relocatable
binaries / packages from a simple recipe format. The recipe format is heavily
inspired by `conda-build` and `boa`, and the output of a regular `rattler-build`
run is a package that can be installed using `mamba`, `rattler` or `conda`.

`rattler-build` does not have any dependencies on `conda-build` or Python and
works as a standalone binary.

![](https://user-images.githubusercontent.com/885054/244683824-fd1b3896-84c7-498c-b406-40ab2a9e450c.svg)

### Installation

The recommended way of installing `rattler-build`, being a conda-package builder, is through a conda package manager.
Next to `rattler-build` we are also building [`pixi`](https://pixi.sh).

With `pixi` you can install `rattler-build` globally:

```bash
pixi global install rattler-build
```

Other options are:
=== "Conda"
    ```shell
    conda install rattler-build -c conda-forge

    mamba install rattler-build -c conda-forge
    micromamba install rattler-build -c conda-forge

    pixi global install rattler-build
    pixi add rattler-build # To a pixi project
    ```

=== "Homebrew"
    ```shell
    brew install rattler-build
    ```
=== "Arch Linux"
    ```shell
    pacman -S rattler-build
    ```
=== "Binary"
    ```shell
    # Download the latest release from the GitHub releases page, for example the linux x86 version with curl:
    curl -SL --progress-bar https://github.com/prefix-dev/rattler-build/releases/latest/download/rattler-build-x86_64-unknown-linux-musl
    ```
    You can grab version of `rattler-build` from the [Github
    Releases](https://github.com/prefix-dev/rattler-build/releases/).

### Completion

When installing `rattler-build` you might want to enable shell completion.
Afterwards, restart the shell or source the shell config file.

### Bash (default on most Linux systems)

```bash
echo 'eval "$(rattler-build completion --shell bash)"' >> ~/.bashrc
```
### Zsh (default on macOS)

```zsh
echo 'eval "$(rattler-build completion --shell zsh)"' >> ~/.zshrc
```

### PowerShell (pre-installed on all Windows systems)

```pwsh
Add-Content -Path $PROFILE -Value '(& rattler-build completion --shell powershell) | Out-String | Invoke-Expression'
```

!!! tip "Failure because no profile file exists"
    Make sure your profile file exists, otherwise create it with:
    ```PowerShell
    New-Item -Path $PROFILE -ItemType File -Force
    ```


### Fish

```fish
echo 'rattler-build completion --shell fish | source' >> ~/.config/fish/config.fish
```

### Nushell

Add the following to the end of your Nushell env file (find it by running `$nu.env-path` in Nushell):

```nushell
mkdir ~/.cache/rattler-build
rattler-build completion --shell nushell | save -f ~/.cache/rattler-build/completions.nu
```

And add the following to the end of your Nushell configuration (find it by running `$nu.config-path`):

```nushell
use ~/.cache/rattler-build/completions.nu *
```

### Elvish

```elv
echo 'eval (rattler-build completion --shell elvish | slurp)' >> ~/.elvish/rc.elv
```

### Dependencies

Currently `rattler-build` needs some dependencies on the host system which are
executed as subprocess. We plan to reduce the number of external dependencies
over time by writing what we need in Rust to make `rattler-build` fully
self-contained.

* `tar` to unpack tarballs downloaded from the internet in a variety of formats.
  `.gz`, `.bz2` and `.xz` are widely used and one might have to install the
  compression packages as well (e.g. `gzip`, `bzip2`, ...)
* `patch` to patch source code after downloading
* `install_name_tool` is necessary on macOS to rewrite the `rpath` of shared
  libraries and executables to make it relative
* `patchelf` is required on Linux to rewrite the `rpath` and `runpath` of shared
  libraries and executables
* `git` to checkout Git repositories (not implemented yet, but will require `git`
  in the future)
* `msvc` on Windows because we cannot ship the MSVC compiler on conda-forge
  (needs to be installed on the host machine)

On Windows, to obtain these dependencies from conda-forge, one can install
`m2-patch`, `m2-bzip2`, `m2-gzip`, `m2-tar`.


### GitHub Action

There is a GitHub Action for `rattler-build`. It can be used to install `rattler-build` in CI/CD workflows and run a build command. Please check out the [GitHub Action documentation](https://github.com/prefix-dev/rattler-build-action) for more information.

### The recipe format

> **Note** You can find all examples below in the [`examples`](https://github.com/prefix-dev/rattler-build/tree/main/examples)
> folder in the codebase and run them with `rattler-build`.

A simple example recipe for the `xtensor` header-only C++ library:

```yaml


context:
  name: xtensor
  version: 0.24.6

package:
  name: ${{ name|lower }}
  version: ${{ version }}

source:
  url: https://github.com/xtensor-stack/xtensor/archive/${{ version }}.tar.gz
  sha256: f87259b51aabafdd1183947747edfff4cff75d55375334f2e81cee6dc68ef655

build:
  number: 0
  script:
    - if: win
      then: |
        cmake -G "NMake Makefiles" -D BUILD_TESTS=OFF -D CMAKE_INSTALL_PREFIX=%LIBRARY_PREFIX% %SRC_DIR%
        nmake
        nmake install
      else: |
        cmake ${CMAKE_ARGS} -DBUILD_TESTS=OFF -DCMAKE_INSTALL_PREFIX=$PREFIX $SRC_DIR -DCMAKE_INSTALL_LIBDIR=lib
        make install

requirements:
  build:
    - ${{ compiler('cxx') }}
    - cmake
    - if: unix
      then: make
  host:
    - xtl >=0.7,<0.8
  run:
    - xtl >=0.7,<0.8
  run_constraints:
    - xsimd >=8.0.3,<10

tests:
  - script:
    - if: unix or emscripten
      then:
        - test -d ${PREFIX}/include/xtensor
        - test -f ${PREFIX}/include/xtensor/xarray.hpp
        - test -f ${PREFIX}/share/cmake/xtensor/xtensorConfig.cmake
        - test -f ${PREFIX}/share/cmake/xtensor/xtensorConfigVersion.cmake
    - if: win
      then:
        - if not exist %LIBRARY_PREFIX%\include\xtensor\xarray.hpp (exit 1)
        - if not exist %LIBRARY_PREFIX%\share\cmake\xtensor\xtensorConfig.cmake (exit 1)
        - if not exist %LIBRARY_PREFIX%\share\cmake\xtensor\xtensorConfigVersion.cmake (exit 1)

about:
  homepage: https://github.com/xtensor-stack/xtensor
  license: BSD-3-Clause
  license_file: LICENSE
  summary: The C++ tensor algebra library
  description: Multi dimensional arrays with broadcasting and lazy computing
  documentation: https://xtensor.readthedocs.io
  repository: https://github.com/xtensor-stack/xtensor

extra:
  recipe-maintainers:
    - some-maintainer
```

A recipe for the `rich` Python package (using `noarch`):

```yaml
context:
  version: "13.4.2"

package:
  name: "rich"
  version: ${{ version }}

source:
  - url: https://pypi.io/packages/source/r/rich/rich-${{ version }}.tar.gz
    sha256: d653d6bccede5844304c605d5aac802c7cf9621efd700b46c7ec2b51ea914898

build:
  # Thanks to `noarch: python` this package works on all platforms
  noarch: python
  script:
    - python -m pip install . -vv

requirements:
  host:
    - pip
    - poetry-core >=1.0.0
    - python 3.10.*
  run:
    # sync with normalized deps from poetry-generated setup.py
    - markdown-it-py >=2.2.0
    - pygments >=2.13.0,<3.0.0
    - python 3.10.*
    - typing_extensions >=4.0.0,<5.0.0

tests:
  - python:
      imports:
        - rich
      pip_check: true

about:
  homepage: https://github.com/Textualize/rich
  license: MIT
  license_file: LICENSE
  summary: Render rich text, tables, progress bars, syntax highlighting, markdown and more to the terminal
  description: |
    Rich is a Python library for rich text and beautiful formatting in the terminal.

    The Rich API makes it easy to add color and style to terminal output. Rich
    can also render pretty tables, progress bars, markdown, syntax highlighted
    source code, tracebacks, and more — out of the box.
  documentation: https://rich.readthedocs.io
  repository: https://github.com/Textualize/rich
```

A recipe for the `curl` library:

```yaml
context:
  version: "8.0.1"

package:
  name: curl
  version: ${{ version }}

source:
  url: http://curl.haxx.se/download/curl-${{ version }}.tar.bz2
  sha256: 9b6b1e96b748d04b968786b6bdf407aa5c75ab53a3d37c1c8c81cdb736555ccf

build:
  number: 0

requirements:
  build:
    - ${{ compiler('c') }}
    - if: win
      then:
        - cmake
        - ninja
    - if: unix
      then:
        - make
        - perl
        - pkg-config
        - libtool
  host:
    - if: linux
      then:
        - openssl

about:
  homepage: http://curl.haxx.se/
  license: MIT/X derivate (http://curl.haxx.se/docs/copyright.html)
  license_file: COPYING
  summary: tool and library for transferring data with URL syntax
  description: |
    Curl is an open source command line tool and library for transferring data
    with URL syntax. It is used in command lines or scripts to transfer data.
  documentation: https://curl.haxx.se/docs/
  repository: https://github.com/curl/curl
```

For the `curl` library recipe, two additional script files (`build.sh` and `build.bat`) are needed.

**`build.sh`**

```bash
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

**`build.bat`**

```cmd
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
