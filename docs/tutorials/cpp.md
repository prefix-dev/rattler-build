# Packaging a C++ package

This tutorial will guide you though making a C++ package with rattler-build.

## Header only library

Here we will build a package for the header-only library `xtensor`. The package
depends on `cmake` and `ninja` for building.

The main "trick" is to instruct `CMake` to install the headers in the right
prefix, by using the `CMAKE_INSTALL_PREFIX` setting. On Unix, conda packages
follow the regular "unix" prefix standard ($PREFIX/include and $PREFIX/lib
etc.). On Windows, it also looks like a "unix" prefix but it's nested in a
`Library` folder ($PREFIX/Library/include and $PREFIX/Library/lib etc.). For
this reason, there are some handy variables (`%LIBRARY_PREFIX%` and
`%LIBRARY_BIN%`) that can be used in the `CMake` command to install the headers
and libraries in the right place.

```yaml title="recipe.yaml"
context:
  version: "0.24.6"

package:
  name: xtensor
  version: ${{ version }}

source:
  url: https://github.com/xtensor-stack/xtensor/archive/${{ version }}.tar.gz
  sha256: f87259b51aabafdd1183947747edfff4cff75d55375334f2e81cee6dc68ef655

build:
  number: 0
  script:
    - if: win
      then: |
        cmake -GNinja \
            -D BUILD_TESTS=OFF -DCMAKE_INSTALL_PREFIX=%LIBRARY_PREFIX% \
            %SRC_DIR%
        ninja install
      else: |
        cmake ${CMAKE_ARGS} -DBUILD_TESTS=OFF \
              -DCMAKE_INSTALL_PREFIX=$PREFIX \
              $SRC_DIR
        make install

requirements:
  build:
    - ${{ compiler('cxx') }}
    - cmake
    - ninja
  host:
    - xtl >=0.7,<0.8
  run:
    - xtl >=0.7,<0.8
  run_constraints:
    - xsimd >=8.0.3,<10

tests:
  - package_contents:
      include:
        - xtensor/xarray.hpp
      files:
        - share/cmake/xtensor/xtensorConfig.cmake
        - share/cmake/xtensor/xtensorConfigVersion.cmake

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

## A C++ application

In this example we will build `poppler`, a C++ application to manipulate PDF
files from the command line. The final package will install a few tools into the
`bin/` folder.

```yaml title="recipe.yaml"
context:
  version: "24.01.0"

package:
  name: poppler
  version: ${{ version }}

source:
  url: https://poppler.freedesktop.org/poppler-${{ version }}.tar.xz
  sha256: c7def693a7a492830f49d497a80cc6b9c85cb57b15e9be2d2d615153b79cae08

build:
  script: poppler-build.sh

requirements:
  build:
    - ${{ compiler('c') }} # (1)!
    - ${{ compiler('cxx') }}
    - pkg-config
    - cmake
    - ninja
  host:
    - cairo # (2)!
    - fontconfig
    - freetype
    - glib
    - libboost-headers
    - libjpeg-turbo
    - lcms2
    - libiconv
    - libpng
    - libtiff
    - openjpeg
    - zlib

tests:
  - script:
      - pdfinfo -listenc  # (3)!
      - pdfunite --help
      - pdftocairo --help
```

1. We use the `compiler` function to get the compiler for C and C++.
2. These are all the dependencies that we link against
3. The script test just executes some of the installed tools to check if they
   are working. You could run some more complex tests if you want.

We've defined an external build script in the recipe. This will be searched next
to the recipe by the file name given (or by the default name `build.sh` or
`build.bat`).

```bash title="poppler-build.sh"
#! /bin/bash

extra_cmake_args=(
    -GNinja
    -DCMAKE_INSTALL_LIBDIR=lib
    -DENABLE_UNSTABLE_API_ABI_HEADERS=ON
    -DENABLE_GPGME=OFF
    -DENABLE_LIBCURL=OFF
    -DENABLE_LIBOPENJPEG=openjpeg2
    -DENABLE_QT6=OFF
    -DENABLE_QT5=OFF
    -DENABLE_NSS3=OFF
)

mkdir build && cd build

cmake ${CMAKE_ARGS} "${extra_cmake_args[@]}" \
    -DCMAKE_PREFIX_PATH=$PREFIX \
    -DCMAKE_INSTALL_PREFIX=$PREFIX \
    -DTIFF_INCLUDE_DIR=$PREFIX/include \
    $SRC_DIR

ninja

# The `install` command will take care of copying the files to the right place
ninja install
```

When you look at the output of the `rattler-build` command you might see some
interesting information:

Our package will have some `run` dependencies (even though we did not specify
any). These run-dependencies come from the "run-exports" of the packages we
depend on in the `host` section of the recipe.  This is shown in the output of
`rattler-build` with a little `"RE of [host: package]"`.

Basically, `libcurl` defines: if you depend on me in the host section, then you
should also depend on me during runtime with the following version ranges. This
is important to make linking to shared libraries work correctly.

```
Run dependencies:
╭───────────────────────┬──────────────────────────────────────────────╮
│ Name                  ┆ Spec                                         │
╞═══════════════════════╪══════════════════════════════════════════════╡
│ libcurl               ┆ >=8.5.0,<9.0a0 (RE of [host: libcurl])       │
│ fontconfig            ┆ >=2.14.2,<3.0a0 (RE of [host: fontconfig])   │
│ fonts-conda-ecosystem ┆ (RE of [host: fontconfig])                   │
│ lcms2                 ┆ >=2.16,<3.0a0 (RE of [host: lcms2])          │
│ gettext               ┆ >=0.21.1,<1.0a0 (RE of [host: gettext])      │
│ freetype              ┆ >=2.12.1,<3.0a0 (RE of [host: freetype])     │
│ openjpeg              ┆ >=2.5.0,<3.0a0 (RE of [host: openjpeg])      │
│ libiconv              ┆ >=1.17,<2.0a0 (RE of [host: libiconv])       │
│ cairo                 ┆ >=1.18.0,<2.0a0 (RE of [host: cairo])        │
│ libpng                ┆ >=1.6.42,<1.7.0a0 (RE of [host: libpng])     │
│ libzlib               ┆ >=1.2.13,<1.3.0a0 (RE of [host: zlib])       │
│ libtiff               ┆ >=4.6.0,<4.7.0a0 (RE of [host: libtiff])     │
│ libjpeg-turbo         ┆ >=3.0.0,<4.0a0 (RE of [host: libjpeg-turbo]) │
│ libglib               ┆ >=2.78.3,<3.0a0 (RE of [host: glib])         │
│ libcxx                ┆ >=16 (RE of [build: clangxx_osx-arm64])      │
╰───────────────────────┴──────────────────────────────────────────────╯
```

We can also observe some "linking" information in the output, for example on
macOS:

```txt
[lib/libpoppler-glib.8.26.0.dylib] links against:
 ├─ @rpath/libgio-2.0.0.dylib
 ├─ @rpath/libgobject-2.0.0.dylib
 ├─ /usr/lib/libSystem.B.dylib
 ├─ @rpath/libglib-2.0.0.dylib
 ├─ @rpath/libpoppler.133.dylib
 ├─ @rpath/libfreetype.6.dylib
 ├─ @rpath/libc++.1.dylib
 ├─ @rpath/libpoppler-glib.8.dylib
 └─ @rpath/libcairo.2.dylib
```

rattler-build performs these checks to make sure that:

1. All shared libraries that are linked against are present in the run
   dependencies. If you link against a library that is not explicitly mentioned
   in your recipe, you will get an "overlinking" warning.
2. You don't require any packages in host that you are _not_ linking against. If this is the case, you
   will get an "overdepending" warning.
