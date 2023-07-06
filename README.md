<h1>
  <a href="https://https://github.com/prefix-dev/rattler-build/">
    <img alt="banner" src="https://user-images.githubusercontent.com/885054/244679299-f7dbf3a4-fcfd-46cd-b619-720848790c9e.svg">
  </a>
</h1>

<h1 align="center">

![License][license-badge]
[![Build Status][build-badge]][build]
[![Project Chat][chat-badge]][chat-url]

[license-badge]: https://img.shields.io/badge/license-BSD--3--Clause-blue?style=flat-square
[build-badge]: https://img.shields.io/github/actions/workflow/status/prefix-dev/rattler-build/rust-compile.yml?style=flat-square&branch=main
[build]: https://github.com/prefix-dev/rattler-build/actions/
[chat-badge]: https://img.shields.io/discord/1082332781146800168.svg?label=&logo=discord&logoColor=ffffff&color=7389D8&labelColor=6A7EC2&style=flat-square
[chat-url]: https://discord.gg/kKV8ZxyzY4

</h1>

# rattler-build: a fast conda-package builder

The `rattler-build` tooling and library creates cross-platform relocatable
binaries / packages from a simple recipe format. The recipe format is heavily
inspired by `conda-build` and `boa`, and the output of a regular `rattler-build`
run is a package that can be installed using `mamba`, `rattler` or `conda`.

`rattler-build` does not have any dependencies on `conda-build` or `Python` and
works as a standalone binary.

![](https://user-images.githubusercontent.com/885054/244683824-fd1b3896-84c7-498c-b406-40ab2a9e450c.svg)

### Installation

You can grab a prerelease version of `rattler-build` from the [Github
Releases](https://github.com/prefix-dev/rattler-build/releases/).

#### Dependencies

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
* `git` to checkout Git repositories (not implemented yet, but will require git
  in the future)
* `msvc` on Windows because we cannot ship the MSVC compiler on conda-forge
  (needs to be installed on the host machine)

On Windows, to obtain these dependencies from conda-forge, one can install
`m2-patch`, `m2-bzip2`, `m2-gzip`, `m2-tar`.

### Documentation

We have extensive documentation for `rattler-build`. You can find the [book
here](https://prefix-dev.github.io/rattler-build).

### Usage

`rattler-build` comes with two commands: `build` and `test`.

The `build` command takes a `--recipe recipe.yaml` as input and produces a
package as output. The `test` subcommand can be used to test existing packages
(tests are shipped with the package).

### The recipe format

> **Note** You can find all examples below in the [`examples`](./examples/)
> folder and run them with `rattler-build`.

A simple example recipe for the `xtensor` header-only C++ library:

```yaml
context:
  name: xtensor
  version: "0.24.6"

package:
  name: "{{ name|lower }}"
  version: "{{ version }}"

source:
  url: https://github.com/xtensor-stack/xtensor/archive/{{ version }}.tar.gz
  sha256: f87259b51aabafdd1183947747edfff4cff75d55375334f2e81cee6dc68ef655

build:
  number: 0
  script:
    - sel(unix): |
        cmake ${CMAKE_ARGS} -DBUILD_TESTS=OFF -DCMAKE_INSTALL_PREFIX=$PREFIX $SRC_DIR -DCMAKE_INSTALL_LIBDIR=lib
        make install
    - sel(win): |
        cmake -G "NMake Makefiles" -D BUILD_TESTS=OFF -D CMAKE_INSTALL_PREFIX=%LIBRARY_PREFIX% %SRC_DIR%
        nmake
        nmake install

requirements:
  build:
    - "{{ compiler('cxx') }}"
    - cmake
    - sel(unix): make
  host:
    - xtl >=0.7,<0.8
  run:
    - xtl >=0.7,<0.8
  run_constrained:
    - xsimd >=8.0.3,<10

test:
  commands:
    - sel(unix): test -f ${PREFIX}/include/xtensor/xarray.hpp
    - sel(win): if not exist %LIBRARY_PREFIX%\include\xtensor\xarray.hpp (exit 1)

about:
  home: https://github.com/xtensor-stack/xtensor
  license: BSD-3-Clause
  license_file: LICENSE
  summary: The C++ tensor algebra library
```

<details>
  <summary>
    A recipe for the `rich` Python package (using `noarch`)
  </summary>

```yaml
context:
  version: "13.3.3"

package:
  name: rich
  version: "{{ version }}"

source:
  - url: https://pypi.io/packages/source/r/rich/rich-{{ version }}.tar.gz
    sha256: dc84400a9d842b3a9c5ff74addd8eb798d155f36c1c91303888e0a66850d2a15

build:
  # Thanks to `noarch: python` this package works on all platforms
  noarch: python
  script:
    - python -m pip install . -vv --no-deps --no-build-isolation

requirements:
  host:
    - pip
    - poetry-core >=1.0.0
    - python 3.11
  run:
    # sync with normalized deps from poetry-generated setup.py
    - markdown-it-py >=2.2.0,<3.0.0
    - pygments >=2.13.0,<3.0.0
    - python 3.11
    - typing_extensions >=4.0.0,<5.0.0

test:
  imports:
    - rich
  commands:
    - pip check
  requires:
    - pip

about:
  home: https://github.com/Textualize/rich
  license: MIT
  license_family: MIT
  license_file: LICENSE
  summary: Render rich text, tables, progress bars, syntax highlighting, markdown and more to the terminal
  description: |
    Rich is a Python library for rich text and beautiful formatting in the terminal.

    The Rich API makes it easy to add color and style to terminal output. Rich
    can also render pretty tables, progress bars, markdown, syntax highlighted
    source code, tracebacks, and more — out of the box.
  doc_url: https://rich.readthedocs.io
  dev_url: https://github.com/Textualize/rich
```
</details>

<details>
<summary>A recipe for the `curl` library</summary>

```yaml
context:
  version: "8.0.1"

package:
  name: curl
  version: "{{ version }}"

source:
  url: http://curl.haxx.se/download/curl-{{ version }}.tar.bz2
  sha256: 9b6b1e96b748d04b968786b6bdf407aa5c75ab53a3d37c1c8c81cdb736555ccf

build:
  number: 0

requirements:
  build:
    - "{{ compiler('c') }}"
    - sel(win): cmake
    - sel(win): ninja
    - sel(unix): make
    - sel(unix): perl
    - sel(unix): pkg-config
    - sel(unix): libtool
  host:
    - sel(linux): openssl

about:
  home: http://curl.haxx.se/
  license: MIT/X derivate (http://curl.haxx.se/docs/copyright.html)
  license_family: MIT
  license_file: COPYING
  summary: tool and library for transferring data with URL syntax
  description: |
    Curl is an open source command line tool and library for transferring data
    with URL syntax. It is used in command lines or scripts to transfer data.
  doc_url: https://curl.haxx.se/docs/
  dev_url: https://github.com/curl/curl
  doc_source_url: https://github.com/curl/curl/tree/master/docs
```

For this recipe, two additional script files (`build.sh` and `build.bat`) are
needed.

**build.sh**

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

**build.bat**

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
</details>
